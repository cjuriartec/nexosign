//! Cola «humana» antes de encolar PKCS#11: la API guarda rutas y la app completa el asistente.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::ports::QUEUE_MAX_WALL_CLOCK_SECS;

#[derive(Clone)]
pub struct PendingBatchIntent {
    pub inputs: Vec<PathBuf>,
    pub output_dir: Option<PathBuf>,
    /// Directorio temporal con PDF subidos por multipart; se borra al expirar o al terminar el lote.
    pub staging_dir: Option<PathBuf>,
    pub created_unix: u64,
}

impl PendingBatchIntent {
    pub fn restore_from_storage(
        inputs: Vec<PathBuf>,
        output_dir: Option<PathBuf>,
        staging_dir: Option<PathBuf>,
        created_unix: u64,
    ) -> Self {
        Self {
            inputs,
            output_dir,
            staging_dir,
            created_unix,
        }
    }

    pub fn new(
        inputs: Vec<PathBuf>,
        output_dir: Option<PathBuf>,
        staging_dir: Option<PathBuf>,
    ) -> Self {
        let created_unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Self {
            inputs,
            output_dir,
            staging_dir,
            created_unix,
        }
    }

    pub fn is_expired(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        now.saturating_sub(self.created_unix) > QUEUE_MAX_WALL_CLOCK_SECS
    }
}
