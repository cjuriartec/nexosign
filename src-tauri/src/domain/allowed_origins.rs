//! Lista de orígenes permitidos (capa dominio). La persistencia llegará en fases posteriores.

use serde::{Deserialize, Serialize};

/// Orígenes HTTPS/HTTP permitidos para el header `Origin` (CORS).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AllowedOrigins {
    origins: Vec<String>,
}

impl AllowedOrigins {
    /// Valores por defecto para desarrollo (Vite/Tauri según `vite.config.js`: puerto 1420).
    pub fn development_defaults() -> Self {
        Self {
            origins: vec![
                "http://localhost:1420".to_string(),
                "http://127.0.0.1:1420".to_string(),
                "http://localhost:5173".to_string(),
                "http://127.0.0.1:5173".to_string(),
            ],
        }
    }

    /// Carga defaults y añade entradas desde `NEXOSIGN_ALLOWED_ORIGINS` (coma-separado).
    pub fn from_env() -> Self {
        let mut s = Self::development_defaults();
        if let Ok(extra) = std::env::var("NEXOSIGN_ALLOWED_ORIGINS") {
            for part in extra.split(',') {
                let p = part.trim();
                if p.is_empty() {
                    continue;
                }
                let normalized = crate::domain::origin_policy::normalize_origin(p);
                if !s.origins.iter().any(|x| x == &normalized) {
                    s.origins.push(normalized);
                }
            }
        }
        s
    }

    pub fn origins(&self) -> &[String] {
        &self.origins
    }

    pub fn replace_all(&mut self, origins: Vec<String>) {
        self.origins = origins;
    }

    pub fn is_allowed_origin(&self, origin_header: &str) -> bool {
        crate::domain::origin_policy::is_origin_allowed(origin_header, &self.origins)
    }

    /// Añade un origen normalizado si no estaba (persistencia SQLite / UI).
    pub fn add_if_absent(&mut self, raw: &str) {
        let n = crate::domain::origin_policy::normalize_origin(raw);
        if n.is_empty() {
            return;
        }
        if !self.origins.iter().any(|x| crate::domain::origin_policy::normalize_origin(x) == n) {
            self.origins.push(n);
        }
    }

    /// Quita un origen de la lista en memoria (tras borrar en SQLite).
    pub fn remove_matching(&mut self, raw: &str) {
        let n = crate::domain::origin_policy::normalize_origin(raw);
        self.origins
            .retain(|x| crate::domain::origin_policy::normalize_origin(x) != n);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn development_defaults_contains_localhost_1420() {
        let a = AllowedOrigins::development_defaults();
        assert!(a.is_allowed_origin("http://localhost:1420"));
        assert!(a.is_allowed_origin("http://127.0.0.1:1420"));
    }

    #[test]
    fn replace_all_updates_policy() {
        let mut a = AllowedOrigins::development_defaults();
        a.replace_all(vec!["https://solo.estos".to_string()]);
        assert!(a.is_allowed_origin("https://solo.estos"));
        assert!(!a.is_allowed_origin("http://localhost:1420"));
    }
}
