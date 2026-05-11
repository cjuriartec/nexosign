use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use tauri::AppHandle;

use crate::domain::pending_batch_intent::PendingBatchIntent;
use crate::adapters::http::{build_router, state::SharedState, LOCAL_API_PORT};
use crate::adapters::pkcs11::token::Pkcs11TokenManager;
use crate::domain::allowed_origins::AllowedOrigins;

/// Construye el estado compartido de la API local (cola batch, snapshots) y arranca worker + vigía.
/// Debe llamarse una sola vez; el mismo [`SharedState`] se gestiona en Tauri (`.manage`) y se pasa a Axum.
pub fn build_shared_api_state(
    handle: AppHandle,
    origins: Arc<RwLock<AllowedOrigins>>,
    pkcs11: Arc<Pkcs11TokenManager>,
    batch_cancel: Arc<Mutex<HashMap<String, tokio_util::sync::CancellationToken>>>,
    pending_batch_intents: Arc<Mutex<HashMap<String, PendingBatchIntent>>>,
    queue_sqlite_path: PathBuf,
    batch_signed_outputs: Arc<Mutex<HashMap<String, Vec<PathBuf>>>>,
) -> SharedState {
    let intent_request_to_job = Arc::new(Mutex::new(HashMap::new()));
    let (tx, rx) = tokio::sync::mpsc::channel(crate::infrastructure::batch_runtime::BATCH_QUEUE_CAPACITY);
    let batch_job_snapshots = Arc::new(Mutex::new(HashMap::new()));
    crate::adapters::worker::batch::spawn_batch_worker(
        rx,
        pkcs11.clone(),
        Some(handle.clone()),
        batch_cancel.clone(),
        batch_signed_outputs.clone(),
        batch_job_snapshots.clone(),
        Some(Arc::new(queue_sqlite_path.clone())),
    );
    crate::adapters::worker::batch::spawn_batch_job_timeout_watchdog(
        batch_cancel.clone(),
        batch_job_snapshots.clone(),
        batch_signed_outputs.clone(),
        intent_request_to_job.clone(),
        Arc::new(queue_sqlite_path.clone()),
        Some(handle.clone()),
    );

    SharedState::new(
        origins,
        Some(handle),
        Some(tx),
        batch_cancel,
        Some(pkcs11),
        pending_batch_intents,
        batch_signed_outputs,
        batch_job_snapshots,
        intent_request_to_job,
        Some(Arc::new(queue_sqlite_path)),
    )
}

/// Arranca el servidor Axum en segundo plano (`127.0.0.1:14500`) con el estado ya construido.
pub fn spawn_local_api(state: SharedState) {
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
