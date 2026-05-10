//! Cola `mpsc` y worker único para firma batch (PKCS#11 no paralelizable).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tauri::AppHandle;
use tauri::Emitter;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::adapters::pkcs11::token::Pkcs11TokenManager;
use crate::application::sign_batch::{process_batch, SignBatchInput};
use crate::ports::{
    BatchJobPhase, BatchJobSnapshot, ProgressEvent, ProgressNotifier,
};

pub struct BatchJob {
    pub job_id: String,
    pub cert_id_hex: String,
    pub inputs: Vec<std::path::PathBuf>,
    pub cancel: CancellationToken,
    pub output_dir: Option<std::path::PathBuf>,
    pub signature_grid: Option<crate::adapters::pdf::pades::SignatureGridPlacement>,
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
                current_file_name: None,
                error: None,
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

fn finalize_batch_job(
    snapshots: &Arc<Mutex<HashMap<String, BatchJobSnapshot>>>,
    job_id: &str,
    cancelled: bool,
    outputs_len: usize,
    inputs_len: usize,
) {
    let Ok(mut g) = snapshots.lock() else {
        return;
    };
    let Some(s) = g.get_mut(job_id) else {
        return;
    };
    if cancelled {
        s.phase = BatchJobPhase::Cancelled;
        return;
    }
    if outputs_len == 0 && inputs_len > 0 && s.actual == 0 && s.error.is_some() {
        s.phase = BatchJobPhase::Failed;
        return;
    }
    s.phase = BatchJobPhase::Completed;
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

/// Arranca el consumidor único; debe llamarse una vez al iniciar la API local.
pub fn spawn_batch_worker(
    mut rx: mpsc::Receiver<BatchJob>,
    token: Arc<Pkcs11TokenManager>,
    app: Option<AppHandle>,
    cancel_registry: Arc<Mutex<HashMap<String, CancellationToken>>>,
    signed_outputs: Arc<Mutex<HashMap<String, Vec<PathBuf>>>>,
    job_snapshots: Arc<Mutex<HashMap<String, BatchJobSnapshot>>>,
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
                let outputs = process_batch(input, token_c, notifier);
                let cancelled = cancel_token.is_cancelled();
                finalize_batch_job(&snapshots_c, &jid, cancelled, outputs.len(), inputs_len);
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
