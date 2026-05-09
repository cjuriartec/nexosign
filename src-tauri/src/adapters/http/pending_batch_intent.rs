//! Cola «humana» antes de encolar PKCS#11: la API guarda rutas y la app completa el asistente.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Tiempo máximo para abrir la app y confirmar (segundos).
pub const PENDING_INTENT_TTL_SECS: u64 = 30 * 60;

#[derive(Clone)]
pub struct PendingBatchIntent {
    pub inputs: Vec<PathBuf>,
    pub output_dir: Option<PathBuf>,
    pub created_unix: u64,
}

impl PendingBatchIntent {
    pub fn new(inputs: Vec<PathBuf>, output_dir: Option<PathBuf>) -> Self {
        let created_unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Self {
            inputs,
            output_dir,
            created_unix,
        }
    }

    pub fn is_expired(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        now.saturating_sub(self.created_unix) > PENDING_INTENT_TTL_SECS
    }
}
