//! Estado observable de un trabajo de firma por lotes (expuesto por HTTP y actualizado por el worker).

use std::sync::RwLock;

use serde::{Deserialize, Serialize};

/// Default si no hay guardado en BD y no aplica override por entorno.
pub const DEFAULT_QUEUE_MAX_WALL_CLOCK_SECS: u64 = 5 * 60;

/// Mínimo y máximo permitidos (segundos) para el ajuste guardado y la variable de entorno.
pub const MIN_QUEUE_MAX_SECS: u64 = 60;
pub const MAX_QUEUE_MAX_SECS: u64 = 604_800;

/// Variable de entorno: si está definida y válida, **tiene prioridad** sobre el valor guardado en ajustes.
pub const ENV_BATCH_JOB_MAX_SECS: &str = "NEXOSIGN_BATCH_JOB_MAX_SECS";

static STORED_QUEUE_MAX_SECS: RwLock<u64> = RwLock::new(DEFAULT_QUEUE_MAX_WALL_CLOCK_SECS);

/// Valor de `NEXOSIGN_BATCH_JOB_MAX_SECS` si está fijada y en rango; si no, `None`.
pub fn env_batch_job_max_secs_override() -> Option<u64> {
    std::env::var(ENV_BATCH_JOB_MAX_SECS)
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|&n| (MIN_QUEUE_MAX_SECS..=MAX_QUEUE_MAX_SECS).contains(&n))
}

/// Valor guardado en SQLite / memoria (sin aplicar el override de entorno).
pub fn stored_queue_max_wall_clock_secs() -> u64 {
    STORED_QUEUE_MAX_SECS
        .read()
        .map(|g| *g)
        .unwrap_or(DEFAULT_QUEUE_MAX_WALL_CLOCK_SECS)
}

/// Inicializa el valor almacenado en memoria desde la BD (o default). Llamar una vez al arranque.
pub fn init_stored_queue_max_secs_from_db(stored: Option<u64>) {
    let v = stored
        .filter(|&n| (MIN_QUEUE_MAX_SECS..=MAX_QUEUE_MAX_SECS).contains(&n))
        .unwrap_or(DEFAULT_QUEUE_MAX_WALL_CLOCK_SECS);
    if let Ok(mut g) = STORED_QUEUE_MAX_SECS.write() {
        *g = v;
    }
}

/// Ventana máxima (segundos) para intents pendientes y trabajos batch encolados.
///
/// Precedencia: variable de entorno [`ENV_BATCH_JOB_MAX_SECS`] (si válida) > valor en ajustes (BD) > default.
pub fn queue_max_wall_clock_secs() -> u64 {
    env_batch_job_max_secs_override().unwrap_or_else(stored_queue_max_wall_clock_secs)
}

/// Misma política en `i64` para timestamps Unix y SQLite.
pub fn batch_job_max_wall_clock_secs_i64() -> i64 {
    queue_max_wall_clock_secs() as i64
}

/// Mensaje de error para timeouts del vigía de cola (texto mostrado al usuario).
pub fn batch_job_timeout_user_message() -> String {
    let s = queue_max_wall_clock_secs();
    format!("Tiempo máximo del trabajo ({s} s) superado.")
}

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
