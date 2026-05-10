//! Cola `mpsc` y worker único para firma batch (PKCS#11 no paralelizable).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::AppHandle;
use tauri::Emitter;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::adapters::pdf::pades::Pkcs11PdfPadesSigner;
use crate::adapters::pkcs11::token::Pkcs11TokenManager;
use crate::application::sign_batch::{process_batch, SignBatchInput};
use crate::infrastructure::batch_runtime::BATCH_WATCHDOG_INTERVAL_SECS;
use crate::ports::{
    BatchJobPhase, BatchJobSnapshot, ProgressEvent, ProgressNotifier, SignatureGridPlacement,
    BATCH_JOB_MAX_WALL_CLOCK_SECS, BATCH_JOB_RAM_GC_AFTER_TERMINAL_SECS,
};

const BATCH_JOB_TIMEOUT_EMIT_ERROR: &str =
    "Tiempo máximo del trabajo (5 min) superado.";

pub struct BatchJob {
    pub job_id: String,
    pub cert_id_hex: String,
    pub inputs: Vec<std::path::PathBuf>,
    pub cancel: CancellationToken,
    pub output_dir: Option<std::path::PathBuf>,
    pub signature_grid: Option<SignatureGridPlacement>,
    /// PIN para repetir `C_Login` en el mismo hilo que `C_Sign` (PKCS#11 suele ser por hilo).
    pub pin: Option<String>,
    /// PNG del sello visible (mismo diseño que Certificados); `None` usa apariencia vectorial.
    pub seal_png: Option<Vec<u8>>,
    /// Directorios o ficheros temporales a borrar tras `process_batch` (p. ej. staging multipart).
    pub cleanup_paths: Vec<PathBuf>,
}

/// Actualiza el mapa compartido (API HTTP) y reenvía el mismo payload al frontend vía Tauri.
struct SharedBatchProgress {
    snapshots: Arc<Mutex<HashMap<String, BatchJobSnapshot>>>,
    app: Option<AppHandle>,
}

impl ProgressNotifier for SharedBatchProgress {
    fn notify(&self, ev: ProgressEvent) {
        if let Ok(mut g) = self.snapshots.lock() {
            let snap = g.entry(ev.job_id.clone()).or_insert_with(|| BatchJobSnapshot {
                job_id: ev.job_id.clone(),
                phase: BatchJobPhase::Running,
                actual: 0,
                total: ev.total.max(1),
                queued_at_unix: None,
                current_file_name: None,
                error: None,
                terminal_at_unix: None,
            });
            snap.phase = BatchJobPhase::Running;
            snap.actual = ev.current;
            snap.total = ev.total.max(1);
            snap.current_file_name = if ev.file_name.is_empty() {
                None
            } else {
                Some(ev.file_name.clone())
            };
            snap.error = ev.error.clone();
        }
        if let Some(ref h) = self.app {
            let payload = serde_json::json!({
                "actual": ev.current,
                "total": ev.total,
                "job_id": ev.job_id,
                "nombre_archivo": ev.file_name,
                "path": ev.path,
                "output_path": ev.output_path,
                "error": ev.error,
            });
            let _ = h.emit("progreso", &payload);
        }
    }
}

fn now_unix_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn finalize_batch_job(
    snapshots: &Arc<Mutex<HashMap<String, BatchJobSnapshot>>>,
    queue_db: Option<&Path>,
    job_id: &str,
    cancelled: bool,
    outputs_len: usize,
    inputs_len: usize,
) {
    fn clear_enqueue(db: Option<&Path>, job_id: &str) {
        if let Some(p) = db {
            let _ = crate::adapters::persistence::queue_store::delete_batch_job_enqueue(p, job_id);
        }
    }

    let ts = now_unix_secs();

    let Ok(mut g) = snapshots.lock() else {
        clear_enqueue(queue_db, job_id);
        return;
    };
    let Some(s) = g.get_mut(job_id) else {
        clear_enqueue(queue_db, job_id);
        return;
    };
    if cancelled {
        s.phase = BatchJobPhase::Cancelled;
        s.terminal_at_unix = Some(ts);
        clear_enqueue(queue_db, job_id);
        return;
    }
    if s.phase == BatchJobPhase::Cancelled {
        clear_enqueue(queue_db, job_id);
        return;
    }
    if outputs_len == 0 && inputs_len > 0 && s.actual == 0 && s.error.is_some() {
        s.phase = BatchJobPhase::Failed;
        s.terminal_at_unix = Some(ts);
        clear_enqueue(queue_db, job_id);
        return;
    }
    s.phase = BatchJobPhase::Completed;
    s.terminal_at_unix = Some(ts);
    clear_enqueue(queue_db, job_id);
}

fn cleanup_staging_paths(paths: Vec<PathBuf>) {
    for p in paths {
        if p.is_dir() {
            if let Err(e) = std::fs::remove_dir_all(&p) {
                tracing::warn!(path = %p.display(), "staging cleanup: {e}");
            }
        } else if p.is_file() {
            if let Err(e) = std::fs::remove_file(&p) {
                tracing::warn!(path = %p.display(), "staging cleanup: {e}");
            }
        }
    }
}

fn gc_terminal_batch_ram(
    now: i64,
    job_snapshots: &Arc<Mutex<HashMap<String, BatchJobSnapshot>>>,
    signed_outputs: &Arc<Mutex<HashMap<String, Vec<PathBuf>>>>,
    intent_map: &Arc<Mutex<HashMap<String, String>>>,
) {
    let cutoff = now.saturating_sub(BATCH_JOB_RAM_GC_AFTER_TERMINAL_SECS);
    let mut remove_ids: Vec<String> = Vec::new();
    if let Ok(guard) = job_snapshots.lock() {
        for (id, snap) in guard.iter() {
            if matches!(
                snap.phase,
                BatchJobPhase::Completed | BatchJobPhase::Failed | BatchJobPhase::Cancelled
            ) {
                if let Some(t) = snap.terminal_at_unix {
                    if t < cutoff {
                        remove_ids.push(id.clone());
                    }
                }
            }
        }
    }
    for id in remove_ids {
        if let Ok(mut g) = job_snapshots.lock() {
            g.remove(&id);
        }
        if let Ok(mut g) = signed_outputs.lock() {
            g.remove(&id);
        }
        if let Ok(mut g) = intent_map.lock() {
            g.retain(|_rid, jid| jid != &id);
        }
    }
}

/// Vigía trabajos en SQLite `batch_job_enqueue`: si el encolado supera [`BATCH_JOB_MAX_WALL_CLOCK_SECS`],
/// cancela el token y marca la instantánea como `cancelled`.
pub fn spawn_batch_job_timeout_watchdog(
    batch_cancel: Arc<Mutex<HashMap<String, CancellationToken>>>,
    job_snapshots: Arc<Mutex<HashMap<String, BatchJobSnapshot>>>,
    batch_signed_outputs: Arc<Mutex<HashMap<String, Vec<PathBuf>>>>,
    intent_request_to_job: Arc<Mutex<HashMap<String, String>>>,
    queue_sqlite_path: Arc<PathBuf>,
    app: Option<AppHandle>,
) {
    use tokio::time::{interval, Duration, MissedTickBehavior};

    tauri::async_runtime::spawn(async move {
        let mut ticker = interval(Duration::from_secs(BATCH_WATCHDOG_INTERVAL_SECS));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let cutoff = now.saturating_sub(BATCH_JOB_MAX_WALL_CLOCK_SECS);

            let stale_ids = match crate::adapters::persistence::queue_store::list_batch_job_ids_enqueued_before(
                queue_sqlite_path.as_ref(),
                cutoff,
            ) {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!(error = %e, "watchdog listar batch_job_enqueue");
                    Vec::new()
                }
            };

            for id in stale_ids {
                if let Ok(reg) = batch_cancel.lock() {
                    if let Some(t) = reg.get(&id) {
                        t.cancel();
                    }
                }
                if let Ok(mut guard) = job_snapshots.lock() {
                    if let Some(s) = guard.get_mut(&id) {
                        if matches!(s.phase, BatchJobPhase::Queued | BatchJobPhase::Running) {
                            s.phase = BatchJobPhase::Cancelled;
                            s.error = Some(BATCH_JOB_TIMEOUT_EMIT_ERROR.into());
                            s.terminal_at_unix = Some(now);
                        }
                    }
                }
                let _ = crate::adapters::persistence::queue_store::delete_batch_job_enqueue(
                    queue_sqlite_path.as_ref(),
                    &id,
                );
                if let Some(ref h) = app {
                    let _ = h.emit(
                        "progreso",
                        serde_json::json!({
                            "actual": 0,
                            "total": 1,
                            "job_id": id,
                            "nombre_archivo": "",
                            "path": "",
                            "output_path": serde_json::Value::Null,
                            "error": BATCH_JOB_TIMEOUT_EMIT_ERROR,
                        }),
                    );
                }
            }

            gc_terminal_batch_ram(
                now,
                &job_snapshots,
                &batch_signed_outputs,
                &intent_request_to_job,
            );
        }
    });
}

/// Arranca el consumidor único; debe llamarse una vez al iniciar la API local.
pub fn spawn_batch_worker(
    mut rx: mpsc::Receiver<BatchJob>,
    token: Arc<Pkcs11TokenManager>,
    app: Option<AppHandle>,
    cancel_registry: Arc<Mutex<HashMap<String, CancellationToken>>>,
    signed_outputs: Arc<Mutex<HashMap<String, Vec<PathBuf>>>>,
    job_snapshots: Arc<Mutex<HashMap<String, BatchJobSnapshot>>>,
    queue_sqlite_path: Option<Arc<PathBuf>>,
) {
    // Debe usar el runtime de Tauri (`setup` no tiene Tokio activo en el hilo actual).
    tauri::async_runtime::spawn(async move {
        while let Some(job) = rx.recv().await {
            let jid = job.job_id.clone();
            let token_c = token.clone();
            let app_c = app.clone();
            let reg_c = cancel_registry.clone();
            let cleanup = job.cleanup_paths.clone();
            let signed_outputs_c = signed_outputs.clone();
            let snapshots_c = job_snapshots.clone();
            let qdb = queue_sqlite_path.clone();
            let run = tokio::task::spawn_blocking(move || {
                let inputs_len = job.inputs.len();
                let cancel_token = job.cancel.clone();
                let notifier = SharedBatchProgress {
                    snapshots: snapshots_c.clone(),
                    app: app_c.clone(),
                };
                let input = SignBatchInput {
                    job_id: job.job_id.clone(),
                    cert_id_hex: job.cert_id_hex,
                    inputs: job.inputs,
                    cancel: job.cancel,
                    output_dir: job.output_dir,
                    signature_grid: job.signature_grid,
                    pin: job.pin,
                    seal_png: job.seal_png,
                };
                let signer = Arc::new(Pkcs11PdfPadesSigner { token: token_c });
                let outputs = process_batch(input, signer, notifier);
                let cancelled = cancel_token.is_cancelled();
                let qpath = qdb.as_ref().map(|a| a.as_path());
                finalize_batch_job(
                    &snapshots_c,
                    qpath,
                    &jid,
                    cancelled,
                    outputs.len(),
                    inputs_len,
                );
                if let Ok(mut m) = signed_outputs_c.lock() {
                    m.insert(jid.clone(), outputs);
                }
                cleanup_staging_paths(cleanup);
                if let Ok(mut g) = reg_c.lock() {
                    g.remove(&jid);
                }
            });
            let _ = run.await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleanup_staging_paths_removes_existing_file_and_dir() {
        let root = std::env::temp_dir().join(format!("nexosign-worker-cleanup-{}", std::process::id()));
        let dir = root.join("dir");
        let file = root.join("a.pdf");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&file, b"%PDF-1.4\n").unwrap();

        cleanup_staging_paths(vec![file.clone(), dir.clone(), root.join("missing.tmp")]);

        assert!(!file.exists());
        assert!(!dir.exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
