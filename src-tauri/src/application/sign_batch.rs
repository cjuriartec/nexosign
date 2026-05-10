//! Procesar un lote de PDFs (PAdES-BES) — orquestación sin Axum/PKCS#11 directo.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::adapters::pdf::pades;
use crate::adapters::pkcs11::token::Pkcs11TokenManager;
use crate::application::errors::SignBatchError;
use crate::ports::{ProgressEvent, ProgressNotifier};

use crate::adapters::pdf::pades::SignatureGridPlacement;

pub struct SignBatchInput {
    pub job_id: String,
    pub cert_id_hex: String,
    pub inputs: Vec<PathBuf>,
    pub cancel: CancellationToken,
    /// Si está definido, los PDF firmados van aquí como `{stem}_firmado.pdf` (p. ej. carpeta hermana `_firmados`).
    pub output_dir: Option<PathBuf>,
    /// Casilla 5×7 en primera página (`None` → valor por defecto del motor PDF).
    pub signature_grid: Option<SignatureGridPlacement>,
}

fn output_path_for(input: &Path, output_dir: Option<&Path>) -> PathBuf {
    let stem = input
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let out_name = format!("{stem}_firmado.pdf");
    if let Ok(dir) = std::env::var("NEXOSIGN_BATCH_OUTPUT_DIR") {
        return PathBuf::from(dir).join(&out_name);
    }
    if let Some(dir) = output_dir {
        return dir.join(&out_name);
    }
    let mut out = input.to_path_buf();
    out.set_file_name(out_name);
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

        let placement = input.signature_grid.unwrap_or_default();

        let res = pades::sign_pdf_pades_bes(
            token.clone(),
            &input.cert_id_hex,
            path,
            &output_path_for(path, input.output_dir.as_deref()),
            placement,
        );

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
    let _ = token.logout();
    Ok(())
}
