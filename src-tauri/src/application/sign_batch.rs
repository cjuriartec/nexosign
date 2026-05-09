//! Procesar un lote de PDFs (PAdES-BES) — orquestación sin Axum/PKCS#11 directo.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::adapters::pdf::pades;
use crate::adapters::pkcs11::token::Pkcs11TokenManager;
use crate::application::errors::SignBatchError;
use crate::ports::{ProgressEvent, ProgressNotifier};

pub struct SignBatchInput {
    pub job_id: String,
    pub cert_id_hex: String,
    pub inputs: Vec<PathBuf>,
    pub cancel: CancellationToken,
}

fn output_path_for(input: &Path) -> PathBuf {
    if let Ok(dir) = std::env::var("NEXOSIGN_BATCH_OUTPUT_DIR") {
        let base = input.file_name().unwrap_or_default();
        let stem = Path::new(base).file_stem().unwrap_or_default();
        return PathBuf::from(dir).join(format!("{}_signed.pdf", stem.to_string_lossy()));
    }
    let mut out = input.to_path_buf();
    let stem = out.file_stem().unwrap_or_default().to_string_lossy();
    out.set_file_name(format!("{}_signed.pdf", stem));
    out
}

/// Ejecuta el lote en el hilo actual (el worker lo invoca dentro de `spawn_blocking`).
pub fn process_batch<P: ProgressNotifier>(
    input: SignBatchInput,
    token: Arc<Pkcs11TokenManager>,
    progress: P,
) -> Result<(), SignBatchError> {
    let total = input.inputs.len().try_into().unwrap_or(u32::MAX);
    for (idx, path) in input.inputs.iter().enumerate() {
        if input.cancel.is_cancelled() {
            progress.notify(ProgressEvent {
                job_id: input.job_id.clone(),
                current: idx.try_into().unwrap_or(0),
                total,
                file_name: String::new(),
                path: String::new(),
                error: Some("lote cancelado".into()),
            });
            break;
        }

        let current = (idx + 1).try_into().unwrap_or(u32::MAX);
        let file_name = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let path_str = path.display().to_string();

        let res =
            pades::sign_pdf_pades_bes(token.clone(), &input.cert_id_hex, path, &output_path_for(path));

        match res {
            Ok(()) => progress.notify(ProgressEvent {
                job_id: input.job_id.clone(),
                current,
                total,
                file_name,
                path: path_str,
                error: None,
            }),
            Err(e) => progress.notify(ProgressEvent {
                job_id: input.job_id.clone(),
                current,
                total,
                file_name,
                path: path_str,
                error: Some(e.to_string()),
            }),
        }
    }
    Ok(())
}
