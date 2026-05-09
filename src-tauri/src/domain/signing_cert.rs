//! Metadatos de certificado de firma expuestos al frontend.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct SigningCertSummary {
    /// Hex de `CKA_ID` si existe; si no, prefijo del fingerprint del DER.
    pub id_hex: String,
    pub label: String,
    /// Subject del certificado X.509 (texto).
    pub subject_dn: String,
}
