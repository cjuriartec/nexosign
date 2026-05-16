//! Procesar un lote de PDFs (PAdES-BES) — orquestación sin Axum ni PKCS#11 directo.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::ports::pdf_pades_signer::{PdfPadesSigner, SignatureGridPlacement};
use crate::ports::{ProgressEvent, ProgressNotifier};

pub struct SignBatchInput {
    pub job_id: String,
    pub cert_id_hex: String,
    pub inputs: Vec<PathBuf>,
    pub cancel: CancellationToken,
    /// Si está definido, los PDF firmados van aquí como `{stem}_firmado.pdf` (p. ej. carpeta hermana `_firmados`).
    pub output_dir: Option<PathBuf>,
    /// Casilla 3×5 en primera página (`None` → valor por defecto del motor PDF).
    pub signature_grid: Option<SignatureGridPlacement>,
    /// Mismo PIN que `POST /batch/sign`; el worker repite login en su hilo para PKCS#11.
    pub pin: Option<String>,
    /// PNG del sello (render del diseño en Certificados).
    pub seal_png: Option<Vec<u8>>,
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
/// Devuelve las rutas de los PDF firmados correctamente (en orden).
pub fn process_batch<P: ProgressNotifier>(
    input: SignBatchInput,
    signer: Arc<dyn PdfPadesSigner>,
    progress: P,
) -> Vec<PathBuf> {
    let mut signed_outputs = Vec::new();
    let total = input.inputs.len().try_into().unwrap_or(u32::MAX);

    if let Err(e) = signer.ensure_signed_session(input.pin.as_deref(), &input.cert_id_hex) {
        progress.notify(ProgressEvent {
            job_id: input.job_id.clone(),
            current: 0,
            total: total.max(1),
            file_name: String::new(),
            path: String::new(),
            output_path: None,
            error: Some(format!("Sesión de firma en el proceso de firma: {e}")),
        });
        signer.end_signed_session();
        return signed_outputs;
    }

    for (idx, path) in input.inputs.iter().enumerate() {
        if input.cancel.is_cancelled() {
            progress.notify(ProgressEvent {
                job_id: input.job_id.clone(),
                current: idx.try_into().unwrap_or(0),
                total,
                file_name: String::new(),
                path: String::new(),
                output_path: None,
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
        let out_path = output_path_for(path, input.output_dir.as_deref());

        let res = signer.sign_pdf_pades_bes(
            &input.cert_id_hex,
            path,
            &out_path,
            placement,
            input.seal_png.as_deref(),
        );

        match res {
            Ok(()) => {
                signed_outputs.push(out_path.clone());
                progress.notify(ProgressEvent {
                    job_id: input.job_id.clone(),
                    current,
                    total,
                    file_name,
                    path: path_str,
                    output_path: Some(out_path.display().to_string()),
                    error: None,
                });
            }
            Err(e) => progress.notify(ProgressEvent {
                job_id: input.job_id.clone(),
                current,
                total,
                file_name,
                path: path_str,
                output_path: None,
                error: Some(e),
            }),
        }
    }
    signer.end_signed_session();
    signed_outputs
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use crate::ports::{NoopProgressNotifier, ProgressEvent};

    struct MockSigner {
        session_ok: bool,
        sign_results: Vec<Result<(), String>>,
        sign_calls: AtomicUsize,
        session_calls: AtomicUsize,
        end_calls: AtomicUsize,
    }

    impl MockSigner {
        fn new(session_ok: bool, sign_results: Vec<Result<(), String>>) -> Self {
            Self {
                session_ok,
                sign_results,
                sign_calls: AtomicUsize::new(0),
                session_calls: AtomicUsize::new(0),
                end_calls: AtomicUsize::new(0),
            }
        }
    }

    impl PdfPadesSigner for MockSigner {
        fn ensure_signed_session(&self, _pin: Option<&str>, _cert_id_hex: &str) -> Result<(), String> {
            self.session_calls.fetch_add(1, Ordering::SeqCst);
            if self.session_ok {
                Ok(())
            } else {
                Err("pin inválido".into())
            }
        }

        fn sign_pdf_pades_bes(
            &self,
            _cert_id_hex: &str,
            _input_path: &Path,
            _output_path: &Path,
            _placement: SignatureGridPlacement,
            _seal_png: Option<&[u8]>,
        ) -> Result<(), String> {
            let i = self.sign_calls.fetch_add(1, Ordering::SeqCst);
            self.sign_results
                .get(i)
                .cloned()
                .unwrap_or(Ok(()))
        }

        fn end_signed_session(&self) {
            self.end_calls.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[derive(Clone, Default)]
    struct CollectNotifier {
        events: Arc<std::sync::Mutex<Vec<ProgressEvent>>>,
    }

    impl CollectNotifier {
        fn new() -> Self {
            Self {
                events: Arc::new(std::sync::Mutex::new(Vec::new())),
            }
        }
    }

    impl ProgressNotifier for CollectNotifier {
        fn notify(&self, ev: ProgressEvent) {
            let mut g = self.events.lock().unwrap();
            g.push(ev);
        }
    }

    fn input_paths(n: usize) -> Vec<PathBuf> {
        (0..n)
            .map(|i| PathBuf::from(format!("/tmp/mock-batch-{i}.pdf")))
            .collect()
    }

    #[test]
    fn process_batch_session_error_emits_no_signed_outputs() {
        let signer = Arc::new(MockSigner::new(false, vec![]));
        let progress = CollectNotifier::new();
        let cancel = CancellationToken::new();
        let out = process_batch(
            SignBatchInput {
                job_id: "j1".into(),
                cert_id_hex: "ab".into(),
                inputs: input_paths(2),
                cancel,
                output_dir: None,
                signature_grid: None,
                pin: Some("1234".into()),
                seal_png: None,
            },
            signer.clone(),
            progress.clone(),
        );
        assert!(out.is_empty());
        assert_eq!(signer.end_calls.load(Ordering::SeqCst), 1);
        let evs = progress.events.lock().unwrap();
        assert!(evs.iter().any(|e| e.error.as_deref() == Some("Sesión de firma en el proceso de firma: pin inválido")));
    }

    #[test]
    fn process_batch_cancel_before_first_sign() {
        let signer = Arc::new(MockSigner::new(true, vec![Ok(())]));
        let progress = CollectNotifier::new();
        let cancel = CancellationToken::new();
        cancel.cancel();
        let out = process_batch(
            SignBatchInput {
                job_id: "j2".into(),
                cert_id_hex: "ab".into(),
                inputs: input_paths(1),
                cancel,
                output_dir: None,
                signature_grid: None,
                pin: None,
                seal_png: None,
            },
            signer.clone(),
            progress.clone(),
        );
        assert!(out.is_empty());
        assert_eq!(signer.sign_calls.load(Ordering::SeqCst), 0);
        let evs = progress.events.lock().unwrap();
        assert!(evs.iter().any(|e| e.error.as_deref() == Some("lote cancelado")));
    }

    #[test]
    fn process_batch_sign_error_still_ends_session() {
        let signer = Arc::new(MockSigner::new(
            true,
            vec![Err("fallo firma".into()), Ok(())],
        ));
        let progress = CollectNotifier::new();
        let cancel = CancellationToken::new();
        let out = process_batch(
            SignBatchInput {
                job_id: "j3".into(),
                cert_id_hex: "ab".into(),
                inputs: input_paths(2),
                cancel,
                output_dir: None,
                signature_grid: None,
                pin: None,
                seal_png: None,
            },
            signer.clone(),
            progress.clone(),
        );
        assert_eq!(out.len(), 1);
        assert_eq!(signer.sign_calls.load(Ordering::SeqCst), 2);
        assert_eq!(signer.end_calls.load(Ordering::SeqCst), 1);
        let evs = progress.events.lock().unwrap();
        assert!(evs.iter().any(|e| e.error.as_deref() == Some("fallo firma")));
    }

    #[test]
    fn process_batch_success_invokes_noop_progress() {
        let signer = Arc::new(MockSigner::new(true, vec![Ok(()), Ok(())]));
        let cancel = CancellationToken::new();
        let out = process_batch(
            SignBatchInput {
                job_id: "j4".into(),
                cert_id_hex: "cd".into(),
                inputs: input_paths(2),
                cancel,
                output_dir: None,
                signature_grid: None,
                pin: None,
                seal_png: None,
            },
            signer.clone(),
            NoopProgressNotifier,
        );
        assert_eq!(out.len(), 2);
        assert_eq!(signer.end_calls.load(Ordering::SeqCst), 1);
    }
}
