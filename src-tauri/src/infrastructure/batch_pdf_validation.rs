//! Validación compartida de rutas PDF para lotes (API HTTP local y comandos Tauri).

use std::path::PathBuf;

/// Tamaño máximo por PDF en un lote (50 MiB).
pub const MAX_BATCH_PDF_BYTES: u64 = 50 * 1024 * 1024;

/// Comprueba rutas absolutas, existencia, extensión `.pdf` y tamaño máximo por archivo.
pub fn validate_batch_pdf_inputs(paths: &[PathBuf]) -> Result<(), String> {
    if paths.is_empty() {
        return Err("inputs no puede estar vacío".into());
    }
    for p in paths {
        if !p.is_absolute() {
            return Err(format!(
                "cada ruta debe ser absoluta (recibido: {})",
                p.display()
            ));
        }
        let meta = std::fs::metadata(p).map_err(|e| format!("{}: {e}", p.display()))?;
        if !meta.is_file() {
            return Err(format!("no es un archivo regular: {}", p.display()));
        }
        if meta.len() > MAX_BATCH_PDF_BYTES {
            return Err(format!(
                "archivo demasiado grande (máx. 50 MiB): {}",
                p.display()
            ));
        }
        let ext = p.extension().and_then(|x| x.to_str()).unwrap_or("");
        if !ext.eq_ignore_ascii_case("pdf") {
            return Err(format!("solo se admiten .pdf: {}", p.display()));
        }
    }
    Ok(())
}
