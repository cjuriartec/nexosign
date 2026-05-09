use std::net::SocketAddr;
use std::sync::{Arc, RwLock};

use tauri::AppHandle;

use crate::adapters::http::{build_router, LOCAL_API_PORT};
use crate::adapters::http::state::SharedState;
use crate::domain::allowed_origins::AllowedOrigins;

/// Arranca el servidor Axum en segundo plano (`127.0.0.1:14500`).
pub fn spawn_local_api(handle: AppHandle, origins: Arc<RwLock<AllowedOrigins>>) {
    let state = SharedState::new(origins, Some(handle));
    let router = build_router(state);
    let addr = SocketAddr::from(([127, 0, 0, 1], LOCAL_API_PORT));

    tauri::async_runtime::spawn(async move {
        let listener = match tokio::net::TcpListener::bind(addr).await {
            Ok(l) => l,
            Err(e) => {
                tracing::error!(
                    error = %e,
                    "No se pudo enlazar la API local; ¿puerto {} ocupado?",
                    LOCAL_API_PORT
                );
                return;
            }
        };

        tracing::info!(%addr, "NexoSign API local escuchando");

        if let Err(e) = axum::serve(listener, router).await {
            tracing::error!(error = %e, "servidor Axum terminó con error");
        }
    });
}
