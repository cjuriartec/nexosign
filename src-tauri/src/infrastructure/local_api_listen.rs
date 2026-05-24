//! Puerto y estado del listener HTTP en loopback (fail-fast, sin rango de fallback).

use std::sync::{Arc, RwLock};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::domain::allowed_origins::AllowedOrigins;

pub const LOCAL_API_DEFAULT_PORT: u16 = 14500;

const ENV_LOCAL_API_PORT: &str = "NEXOSIGN_LOCAL_API_PORT";

/// Estado observable de la API local (UI e integradores).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalApiListenSnapshot {
    pub listening: bool,
    pub port: u16,
    pub base_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub locked_by_env: bool,
}

#[derive(Debug, Clone)]
enum RuntimeState {
    Pending { port: u16 },
    Listening { port: u16 },
    Failed { port: u16, error: String },
}

/// Fuente de verdad en runtime para bind y diagnóstico.
#[derive(Clone)]
pub struct LocalApiRuntime {
    inner: Arc<RwLock<RuntimeState>>,
    locked_by_env: bool,
}

impl LocalApiRuntime {
    pub fn new() -> Self {
        let port = effective_listen_port();
        Self {
            inner: Arc::new(RwLock::new(RuntimeState::Pending { port })),
            locked_by_env: std::env::var(ENV_LOCAL_API_PORT).is_ok(),
        }
    }

    pub fn configured_port(&self) -> u16 {
        match *self.inner.read().expect("local api runtime lock") {
            RuntimeState::Pending { port }
            | RuntimeState::Listening { port }
            | RuntimeState::Failed { port, .. } => port,
        }
    }

    /// Puerto usado por gates HTTP (Host/Referer/URI en loopback).
    pub fn gate_listen_port(&self) -> u16 {
        self.configured_port()
    }

    pub fn is_locked_by_env(&self) -> bool {
        self.locked_by_env
    }

    pub fn set_listening(&self, port: u16) {
        *self.inner.write().expect("local api runtime lock") = RuntimeState::Listening { port };
    }

    pub fn set_failed(&self, port: u16, error: String) {
        *self.inner.write().expect("local api runtime lock") =
            RuntimeState::Failed { port, error };
    }

    pub fn snapshot(&self) -> LocalApiListenSnapshot {
        let locked_by_env = self.locked_by_env;
        match self.inner.read().expect("local api runtime lock").clone() {
            RuntimeState::Pending { port } => LocalApiListenSnapshot {
                listening: false,
                port,
                base_url: base_url_for_port(port),
                error: None,
                locked_by_env,
            },
            RuntimeState::Listening { port } => LocalApiListenSnapshot {
                listening: true,
                port,
                base_url: base_url_for_port(port),
                error: None,
                locked_by_env,
            },
            RuntimeState::Failed { port, error } => LocalApiListenSnapshot {
                listening: false,
                port,
                base_url: base_url_for_port(port),
                error: Some(error),
                locked_by_env,
            },
        }
    }
}

pub fn effective_listen_port() -> u16 {
    match std::env::var(ENV_LOCAL_API_PORT) {
        Ok(raw) => match raw.trim().parse::<u16>() {
            Ok(p) if p > 0 => p,
            _ => {
                tracing::warn!(
                    value = %raw,
                    "NEXOSIGN_LOCAL_API_PORT inválido; usando {LOCAL_API_DEFAULT_PORT}"
                );
                LOCAL_API_DEFAULT_PORT
            }
        },
        Err(_) => LOCAL_API_DEFAULT_PORT,
    }
}

pub fn base_url_for_port(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

/// Si el puerto efectivo difiere del default (override por env), registra orígenes CORS equivalentes.
pub fn merge_custom_port_origins(origins: &mut AllowedOrigins, port: u16) {
    if port == LOCAL_API_DEFAULT_PORT {
        return;
    }
    origins.add_if_absent(&format!("http://127.0.0.1:{port}"));
    origins.add_if_absent(&format!("http://localhost:{port}"));
}

pub fn emit_listen_changed<R: tauri::Runtime>(app: &AppHandle<R>, snapshot: &LocalApiListenSnapshot) {
    let _ = app.emit("local_api_listen_changed", snapshot);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_listen_port_env_override_and_invalid_fallback() {
        let prev = std::env::var(ENV_LOCAL_API_PORT).ok();

        std::env::set_var(ENV_LOCAL_API_PORT, "not-a-port");
        assert_eq!(effective_listen_port(), LOCAL_API_DEFAULT_PORT);

        std::env::set_var(ENV_LOCAL_API_PORT, "15099");
        assert_eq!(effective_listen_port(), 15099);

        match prev {
            Some(v) => std::env::set_var(ENV_LOCAL_API_PORT, v),
            None => std::env::remove_var(ENV_LOCAL_API_PORT),
        }
    }

    #[test]
    fn base_url_for_port_formats_loopback() {
        assert_eq!(base_url_for_port(14500), "http://127.0.0.1:14500");
    }
}
