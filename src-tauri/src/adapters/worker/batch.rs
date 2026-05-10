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
use crate::ports::{NoopProgressNotifier, ProgressEvent, ProgressNotifier};

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

struct TauriProgress(AppHandle);

impl ProgressNotifier for TauriProgress {
    fn notify(&self, ev: ProgressEvent) {
        let payload = serde_json::json!({
            "actual": ev.current,
            "total": ev.total,
            "job_id": ev.job_id,
            "nombre_archivo": ev.file_name,
            "path": ev.path,
            "error": ev.error,
        });
        let _ = self.0.emit("progreso", &payload);
    }
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
) {
    // Debe usar el runtime de Tauri (`setup` no tiene Tokio activo en el hilo actual).
    tauri::async_runtime::spawn(async move {
        while let Some(job) = rx.recv().await {
            let jid = job.job_id.clone();
            let token_c = token.clone();
            let app_c = app.clone();
            let reg_c = cancel_registry.clone();
            let cleanup = job.cleanup_paths.clone();
            let run = tokio::task::spawn_blocking(move || {
                let notifier: Box<dyn ProgressNotifier> = match app_c {
                    Some(h) => Box::new(TauriProgress(h)),
                    None => Box::new(NoopProgressNotifier),
                };
                let input = SignBatchInput {
                    job_id: job.job_id,
                    cert_id_hex: job.cert_id_hex,
                    inputs: job.inputs,
                    cancel: job.cancel,
                    output_dir: job.output_dir,
                    signature_grid: job.signature_grid,
                    pin: job.pin,
                    seal_png: job.seal_png,
                };
                let _ = process_batch(input, token_c, notifier);
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
