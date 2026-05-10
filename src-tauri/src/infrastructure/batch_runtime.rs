//! Constantes operativas compartidas entre servidor local y worker batch.

/// Capacidad del canal `mpsc` de trabajos batch (una firma a la vez en PKCS#11).
pub const BATCH_QUEUE_CAPACITY: usize = 16;

/// Intervalo del vigía de timeouts de encolado (`batch_job_enqueue`).
pub const BATCH_WATCHDOG_INTERVAL_SECS: u64 = 30;
