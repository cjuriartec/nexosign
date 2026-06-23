//! Validación compartida de rutas PDF para lotes (API HTTP local y comandos Tauri).

use std::fs::File;
use std::io::Read;
use std::path::PathBuf;

/// Tamaño máximo por PDF en un lote (50 MiB).
pub const MAX_BATCH_PDF_BYTES: u64 = 50 * 1024 * 1024;

/// Número máximo de PDF por intención multipart (y techo de suma de tamaños).
pub const MAX_PDFS_PER_BATCH_INTENT: usize = 20;

/// Suma máxima de tamaños de PDF en un intent multipart (20 × 50 MiB).
pub const MAX_TOTAL_BATCH_INTENT_BYTES: u64 = MAX_BATCH_PDF_BYTES * MAX_PDFS_PER_BATCH_INTENT as u64;

/// Cabecera mágica de un PDF en offset 0 (PDF 1.7).
const PDF_MAGIC: &[u8] = b"%PDF";

/// Comprueba prefijo `%PDF` y tamaño por archivo.
pub fn validate_pdf_magic_and_size(len: u64, prefix: &[u8]) -> Result<(), String> {
    if len > MAX_BATCH_PDF_BYTES {
        return Err("demasiado grande (máx. 50 MiB por archivo)".into());
    }
    validate_pdf_magic_prefix(prefix)
}

/// Primeros bytes deben ser `%PDF` (sin permitir basura previa en subidas).
pub fn validate_pdf_magic_prefix(prefix: &[u8]) -> Result<(), String> {
    if prefix.len() < PDF_MAGIC.len() {
        return Err("cabecera PDF incompleta (se espera %PDF)".into());
    }
    if &prefix[..PDF_MAGIC.len()] != PDF_MAGIC {
        return Err("no es un PDF válido (cabecera %PDF esperada)".into());
    }
    Ok(())
}

/// Lee los primeros bytes de un fichero para validar magia PDF tras comprobar tamaño en disco.
fn read_pdf_prefix(path: &PathBuf, max_read: usize) -> Result<Vec<u8>, String> {
    let mut f = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut buf = vec![0u8; max_read];
    let n = f
        .read(&mut buf)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    buf.truncate(n);
    Ok(buf)
}

/// Una ruta del lote que no pasó la validación (la UI puede cargar el resto).
#[derive(Debug, Clone, serde::Serialize)]
pub struct RejectedPdfPath {
    pub path: String,
    pub reason: String,
}

/// Comprueba una sola ruta como en `validate_batch_pdf_inputs` (sin exigir lote no vacío).
pub fn validate_single_pdf_input(path: &PathBuf) -> Result<(), String> {
    if !path.is_absolute() {
        return Err(format!(
            "la ruta debe ser absoluta (recibido: {})",
            path.display()
        ));
    }
    let meta = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !meta.is_file() {
        return Err(format!("no es un archivo regular: {}", path.display()));
    }
    if meta.len() > MAX_BATCH_PDF_BYTES {
        return Err(format!(
            "demasiado grande (máx. 50 MiB): {}",
            path.display()
        ));
    }
    let ext = path.extension().and_then(|x| x.to_str()).unwrap_or("");
    if !ext.eq_ignore_ascii_case("pdf") {
        return Err(format!("solo se admiten .pdf: {}", path.display()));
    }
    let prefix = read_pdf_prefix(path, 16)?;
    validate_pdf_magic_prefix(&prefix).map_err(|e| format!("{}: {e}", path.display()))
}

/// Separa rutas válidas de rechazadas (p. ej. tamaño) sin abortar todo el lote.
pub fn partition_pdf_paths(paths: Vec<PathBuf>) -> (Vec<PathBuf>, Vec<RejectedPdfPath>) {
    let mut accepted = Vec::new();
    let mut rejected = Vec::new();
    for p in paths {
        let display = p.display().to_string();
        match validate_single_pdf_input(&p) {
            Ok(()) => accepted.push(p),
            Err(reason) => rejected.push(RejectedPdfPath {
                path: display,
                reason,
            }),
        }
    }
    (accepted, rejected)
}

/// Comprueba rutas absolutas, existencia, extensión `.pdf` y tamaño máximo por archivo.
pub fn validate_batch_pdf_inputs(paths: &[PathBuf]) -> Result<(), String> {
    if paths.is_empty() {
        return Err("inputs no puede estar vacío".into());
    }
    for p in paths {
        validate_single_pdf_input(p)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn magic_prefix_rejects_short_buffer() {
        assert!(validate_pdf_magic_prefix(b"%PD").is_err());
    }

    #[test]
    fn magic_prefix_rejects_wrong_header() {
        assert!(validate_pdf_magic_prefix(b"<!DOCTYPE html>").is_err());
    }

    #[test]
    fn magic_prefix_accepts_pdf_header() {
        assert!(validate_pdf_magic_prefix(b"%PDF-1.7\n").is_ok());
    }

    #[test]
    fn magic_and_size_rejects_over_limit() {
        let big = MAX_BATCH_PDF_BYTES + 1;
        assert!(validate_pdf_magic_and_size(big, b"%PDF").is_err());
    }

    #[test]
    fn batch_inputs_empty_err() {
        assert!(validate_batch_pdf_inputs(&[]).is_err());
    }

    #[test]
    fn validate_single_rejects_relative_path() {
        let rel = std::path::PathBuf::from("relative.pdf");
        let err = validate_single_pdf_input(&rel).unwrap_err();
        assert!(err.contains("absoluta"));
    }

    #[test]
    fn partition_rejected_carries_path_and_reason() {
        let dir = std::env::temp_dir().join(format!(
            "nexosign-pdf-part-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let bad = dir.join("plantilla.pdf");
        std::fs::write(&bad, b"<%xml version=\"1.0\"?>").unwrap();
        let bad_abs = bad.canonicalize().unwrap();

        let (_, rejected) = partition_pdf_paths(vec![bad_abs.clone()]);
        assert_eq!(rejected.len(), 1);
        assert!(rejected[0].path.contains("plantilla.pdf"));
        assert!(
            rejected[0]
                .reason
                .to_lowercase()
                .contains("pdf")
                || rejected[0].reason.contains("%PDF")
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn validate_single_and_partition_with_temp_pdf() {
        let dir = std::env::temp_dir().join(format!(
            "nexosign-pdf-val-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let ok_path = dir.join("good.pdf");
        let mut f = std::fs::File::create(&ok_path).unwrap();
        f.write_all(b"%PDF-1.7 minimal").unwrap();
        drop(f);
        let ok_abs = ok_path.canonicalize().unwrap();
        assert!(validate_single_pdf_input(&ok_abs).is_ok());

        let bad_path = dir.join("bad.pdf");
        std::fs::write(&bad_path, b"not a pdf").unwrap();
        let bad_abs = bad_path.canonicalize().unwrap();
        assert!(validate_single_pdf_input(&bad_abs).is_err());

        let (accepted, rejected) =
            partition_pdf_paths(vec![ok_abs.clone(), bad_abs.clone()]);
        assert_eq!(accepted.len(), 1);
        assert_eq!(rejected.len(), 1);
        assert_eq!(accepted[0], ok_abs);

        assert!(validate_batch_pdf_inputs(&[ok_abs]).is_ok());

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
