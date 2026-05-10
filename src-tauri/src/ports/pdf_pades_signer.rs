//! Puerto para firmar PDF en perfil PAdES-BES (implementación en adaptador PKCS#11 / PDF).

use std::path::Path;

/// Columnas (ancho del PDF) × filas (alto) en la primera página.
/// `col` 0..=2, `row` 0..=4 (fila 0 arriba al leer la página).
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct SignatureGridPlacement {
    pub col: u8,
    pub row: u8,
}

impl Default for SignatureGridPlacement {
    fn default() -> Self {
        Self { col: 1, row: 4 }
    }
}

impl SignatureGridPlacement {
    pub(crate) fn normalized(self) -> Self {
        Self {
            col: self.col.min(2),
            row: self.row.min(4),
        }
    }
}

/// Abrir sesión en el token y firmar cada PDF del lote.
pub trait PdfPadesSigner: Send + Sync {
    /// Si `pin` tiene texto no vacío, hace login PKCS#11 para el certificado indicado.
    fn ensure_signed_session(&self, pin: Option<&str>, cert_id_hex: &str) -> Result<(), String>;

    fn sign_pdf_pades_bes(
        &self,
        cert_id_hex: &str,
        input_path: &Path,
        output_path: &Path,
        placement: SignatureGridPlacement,
        seal_png: Option<&[u8]>,
    ) -> Result<(), String>;

    fn end_signed_session(&self);
}
