//! Estado observable de un trabajo de firma por lotes (expuesto por HTTP y actualizado por el worker).

use serde::{Deserialize, Serialize};

/// Ventana máxima (segundos) para intents pendientes (`created_unix` en BD) y trabajos batch encolados.
pub const QUEUE_MAX_WALL_CLOCK_SECS: u64 = 5 * 60;

/// Misma política en `i64` para timestamps Unix y SQLite (`batch_job_enqueue`).
pub const BATCH_JOB_MAX_WALL_CLOCK_SECS: i64 = QUEUE_MAX_WALL_CLOCK_SECS as i64;

/// Tras terminal (completed/failed/cancelled), cuánto tiempo conservar snapshot + salidas en RAM.
pub const BATCH_JOB_RAM_GC_AFTER_TERMINAL_SECS: i64 = 15 * 60;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BatchJobPhase {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BatchJobSnapshot {
    pub job_id: String,
    pub phase: BatchJobPhase,
    pub actual: u32,
    pub total: u32,
    /// Segundos Unix en que se encoló el trabajo (para expiración por tiempo).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queued_at_unix: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_file_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Momento en que el trabajo pasó a fase terminal (para liberar RAM tras [`BATCH_JOB_RAM_GC_AFTER_TERMINAL_SECS`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_at_unix: Option<i64>,
}
