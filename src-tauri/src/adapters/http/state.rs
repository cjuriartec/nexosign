use std::sync::{Arc, RwLock};

use serde::Serialize;
use tauri::AppHandle;
use tokio::sync::mpsc;

use crate::adapters::worker::batch::BatchJob;
use crate::domain::allowed_origins::AllowedOrigins;

/// Estado compartido entre el servidor Axum y Tauri (`manage`).
#[derive(Clone)]
pub struct SharedState {
    pub origins: Arc<RwLock<AllowedOrigins>>,
    pub app_handle: Option<AppHandle>,
    pub batch_tx: Option<mpsc::Sender<BatchJob>>,
}

impl SharedState {
    pub fn new(
        origins: Arc<RwLock<AllowedOrigins>>,
        app_handle: Option<AppHandle>,
        batch_tx: Option<mpsc::Sender<BatchJob>>,
    ) -> Self {
        Self {
            origins,
            app_handle,
            batch_tx,
        }
    }

    /// Estado para tests sin ventana Tauri.
    pub fn test_default() -> Self {
        Self {
            origins: Arc::new(RwLock::new(AllowedOrigins::development_defaults())),
            app_handle: None,
            batch_tx: None,
        }
    }

    /// Tests HTTP: cola batch simulada.
    pub fn test_with_batch(sender: mpsc::Sender<BatchJob>) -> Self {
        Self {
            origins: Arc::new(RwLock::new(AllowedOrigins::development_defaults())),
            app_handle: None,
            batch_tx: Some(sender),
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
