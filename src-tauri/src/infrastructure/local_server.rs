use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, RwLock};

use tauri::AppHandle;

use crate::adapters::http::PendingBatchIntent;
use crate::adapters::http::{build_router, LOCAL_API_PORT};
use crate::adapters::http::state::SharedState;
use crate::adapters::pkcs11::token::Pkcs11TokenManager;
use crate::domain::allowed_origins::AllowedOrigins;

/// Arranca el servidor Axum en segundo plano (`127.0.0.1:14500`).
pub fn spawn_local_api(
    handle: AppHandle,
    origins: Arc<RwLock<AllowedOrigins>>,
    pkcs11: Arc<Pkcs11TokenManager>,
    batch_cancel: std::sync::Arc<
        std::sync::Mutex<std::collections::HashMap<String, tokio_util::sync::CancellationToken>>,
    >,
    pending_batch_intents: Arc<Mutex<HashMap<String, PendingBatchIntent>>>,
) {
    let (tx, rx) = tokio::sync::mpsc::channel(16);
    let batch_signed_outputs = Arc::new(Mutex::new(HashMap::new()));
    let batch_job_snapshots = Arc::new(Mutex::new(HashMap::new()));
    crate::adapters::worker::batch::spawn_batch_worker(
        rx,
        pkcs11.clone(),
        Some(handle.clone()),
        batch_cancel.clone(),
        batch_signed_outputs.clone(),
        batch_job_snapshots.clone(),
    );

    let state = SharedState::new(
        origins,
        Some(handle),
        Some(tx),
        batch_cancel,
        Some(pkcs11),
        pending_batch_intents,
        batch_signed_outputs,
        batch_job_snapshots,
    );
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
