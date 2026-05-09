use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use serde::Serialize;
use tauri::AppHandle;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

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
}

impl SharedState {
    pub fn new(
        origins: Arc<RwLock<AllowedOrigins>>,
        app_handle: Option<AppHandle>,
        batch_tx: Option<mpsc::Sender<BatchJob>>,
        batch_cancel: Arc<Mutex<HashMap<String, CancellationToken>>>,
    ) -> Self {
        Self {
            origins,
            app_handle,
            batch_tx,
            batch_cancel,
        }
    }

    /// Estado para tests sin ventana Tauri.
    pub fn test_default() -> Self {
        Self {
            origins: Arc::new(RwLock::new(AllowedOrigins::development_defaults())),
            app_handle: None,
            batch_tx: None,
            batch_cancel: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Tests HTTP: cola batch simulada.
    pub fn test_with_batch(sender: mpsc::Sender<BatchJob>) -> Self {
        Self {
            origins: Arc::new(RwLock::new(AllowedOrigins::development_defaults())),
            app_handle: None,
            batch_tx: Some(sender),
            batch_cancel: Arc::new(Mutex::new(HashMap::new())),
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
