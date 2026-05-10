use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use serde::Serialize;
use tauri::AppHandle;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::adapters::http::pending_batch_intent::PendingBatchIntent;
use crate::ports::BatchJobSnapshot;
use crate::adapters::pkcs11::token::Pkcs11TokenManager;
use crate::adapters::worker::batch::BatchJob;
use crate::domain::allowed_origins::AllowedOrigins;

/// Estado compartido entre el servidor Axum y Tauri (`manage`).
#[derive(Clone)]
pub struct SharedState {
    pub origins: Arc<RwLock<AllowedOrigins>>,
    pub app_handle: Option<AppHandle>,
    pub batch_tx: Option<mpsc::Sender<BatchJob>>,
    /// Tokens de cancelación por `job_id` (HTTP registra; worker elimina al terminar).
    pub batch_cancel: Arc<Mutex<HashMap<String, CancellationToken>>>,
    /// Para `POST /batch/sign` con PIN opcional (solo proceso real Tauri).
    pub pkcs11: Option<std::sync::Arc<Pkcs11TokenManager>>,
    /// Tras terminar un `job_id`, rutas de los `*_firmado.pdf` listos para `GET …/files/{i}`.
    pub batch_signed_outputs: Arc<Mutex<HashMap<String, Vec<PathBuf>>>>,
    /// Tras encolar con `intent_request_id`, correlación para sondeo del portal (`GET …/intent/{request_id}/status`).
    pub intent_request_to_job: Arc<Mutex<HashMap<String, String>>>,
    /// Firma diferida: POST `/batch/sign/intent` registra aquí; la UI consume al confirmar.
    pub pending_batch_intents: Arc<Mutex<HashMap<String, PendingBatchIntent>>>,
    /// Estado del trabajo (`GET /api/v1/batch/jobs/{job_id}/status`), actualizado por el worker.
    pub batch_job_snapshots: Arc<Mutex<HashMap<String, BatchJobSnapshot>>>,
}

/// Referencia compartida para comandos Tauri (misma que [`SharedState::pending_batch_intents`]).
#[derive(Clone)]
pub struct PendingBatchIntents(pub Arc<Mutex<HashMap<String, PendingBatchIntent>>>);

impl SharedState {
    pub fn new(
        origins: Arc<RwLock<AllowedOrigins>>,
        app_handle: Option<AppHandle>,
        batch_tx: Option<mpsc::Sender<BatchJob>>,
        batch_cancel: Arc<Mutex<HashMap<String, CancellationToken>>>,
        pkcs11: Option<std::sync::Arc<Pkcs11TokenManager>>,
        pending_batch_intents: Arc<Mutex<HashMap<String, PendingBatchIntent>>>,
        batch_signed_outputs: Arc<Mutex<HashMap<String, Vec<PathBuf>>>>,
        batch_job_snapshots: Arc<Mutex<HashMap<String, BatchJobSnapshot>>>,
    ) -> Self {
        Self {
            origins,
            app_handle,
            batch_tx,
            batch_cancel,
            pkcs11,
            batch_signed_outputs,
            intent_request_to_job: Arc::new(Mutex::new(HashMap::new())),
            pending_batch_intents,
            batch_job_snapshots,
        }
    }

    /// Estado para tests sin ventana Tauri.
    pub fn test_default() -> Self {
        Self {
            origins: Arc::new(RwLock::new(AllowedOrigins::development_defaults())),
            app_handle: None,
            batch_tx: None,
            batch_cancel: Arc::new(Mutex::new(HashMap::new())),
            pkcs11: None,
            batch_signed_outputs: Arc::new(Mutex::new(HashMap::new())),
            intent_request_to_job: Arc::new(Mutex::new(HashMap::new())),
            pending_batch_intents: Arc::new(Mutex::new(HashMap::new())),
            batch_job_snapshots: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Tests HTTP: cola batch simulada.
    pub fn test_with_batch(sender: mpsc::Sender<BatchJob>) -> Self {
        Self {
            origins: Arc::new(RwLock::new(AllowedOrigins::development_defaults())),
            app_handle: None,
            batch_tx: Some(sender),
            batch_cancel: Arc::new(Mutex::new(HashMap::new())),
            pkcs11: None,
            batch_signed_outputs: Arc::new(Mutex::new(HashMap::new())),
            intent_request_to_job: Arc::new(Mutex::new(HashMap::new())),
            pending_batch_intents: Arc::new(Mutex::new(HashMap::new())),
            batch_job_snapshots: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Tests HTTP: cola batch + mismo mapa de intenciones que puede inspeccionar el test.
    pub fn test_with_batch_intents(
        sender: mpsc::Sender<BatchJob>,
        pending_batch_intents: Arc<Mutex<HashMap<String, PendingBatchIntent>>>,
    ) -> Self {
        Self {
            origins: Arc::new(RwLock::new(AllowedOrigins::development_defaults())),
            app_handle: None,
            batch_tx: Some(sender),
            batch_cancel: Arc::new(Mutex::new(HashMap::new())),
            pkcs11: None,
            batch_signed_outputs: Arc::new(Mutex::new(HashMap::new())),
            intent_request_to_job: Arc::new(Mutex::new(HashMap::new())),
            pending_batch_intents,
            batch_job_snapshots: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub service: &'static str,
    pub version: &'static str,
}

#[derive(Serialize)]
pub struct PingResponse {
    pub ok: bool,
}
