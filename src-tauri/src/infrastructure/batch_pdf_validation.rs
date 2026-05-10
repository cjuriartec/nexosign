//! Validación compartida de rutas PDF para lotes (API HTTP local y comandos Tauri).

use std::path::PathBuf;

/// Tamaño máximo por PDF en un lote (50 MiB).
pub const MAX_BATCH_PDF_BYTES: u64 = 50 * 1024 * 1024;

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
    Ok(())
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
