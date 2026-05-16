//! Metadatos de certificado de firma expuestos al frontend.

use serde::Serialize;

/// Prefijo de `id_hex` para certificados del almacén Windows **Current User / MY**.
pub const WIN_MY_CERT_ID_PREFIX: &str = "winmy:";

/// `true` si el id pertenece al almacén MY de Windows (no debe pasarse a PKCS#11).
pub fn is_win_my_cert_id(cert_id_hex: &str) -> bool {
    cert_id_hex.trim().starts_with(WIN_MY_CERT_ID_PREFIX)
}

/// Origen del certificado en la lista unificada.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SigningCertSource {
    Pkcs11,
    WinMy,
}

/// Cómo debe mostrarse el PIN en la app (sin «opcional» ambiguo).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SigningPinUi {
    /// Campo PIN obligatorio en NexoSign (PKCS#11 o claves que lo requieran en app).
    RequiredInApp,
    /// No mostrar PIN en NexoSign; criptografía del SO (típico software MY).
    HiddenUseOsCrypto,
    /// Sin PIN en app; aviso fijo de que Windows o el dispositivo pueden intervenir.
    OsMayPrompt,
}

#[derive(Debug, Clone, Serialize)]
pub struct SigningCertSummary {
    /// Hex de `CKA_ID` si existe; si no, prefijo del fingerprint del DER. En MY Windows: `winmy:` + SHA-1 hex.
    pub id_hex: String,
    pub label: String,
    /// Subject del certificado X.509 (texto).
    pub subject_dn: String,
    pub source: SigningCertSource,
    pub pin_ui: SigningPinUi,
}
