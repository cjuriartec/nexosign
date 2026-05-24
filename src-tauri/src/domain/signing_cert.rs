//! Metadatos de certificado de firma expuestos al frontend.

use std::collections::HashSet;

use serde::Serialize;
use sha1::{Digest, Sha1};

/// Prefijo de `id_hex` para certificados del almacén Windows **Current User / MY**.
pub const WIN_MY_CERT_ID_PREFIX: &str = "winmy:";

/// `true` si el id pertenece al almacén MY de Windows (no debe pasarse a PKCS#11).
pub fn is_win_my_cert_id(cert_id_hex: &str) -> bool {
    cert_id_hex.trim().starts_with(WIN_MY_CERT_ID_PREFIX)
}

/// Huella SHA-1 del DER X.509 (hex minúsculas), mismo criterio que Windows MY.
pub fn sha1_thumbprint_hex(der: &[u8]) -> String {
    hex::encode(Sha1::digest(der))
}

/// Huella SHA-1 extraída de `winmy:` + 40 hex, si aplica.
pub fn thumbprint_from_win_my_id(cert_id_hex: &str) -> Option<String> {
    let rest = cert_id_hex.strip_prefix(WIN_MY_CERT_ID_PREFIX)?;
    if rest.len() != 40 || !rest.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(rest.to_ascii_lowercase())
}

/// Clave de subject normalizada para deduplicar cuando falta huella.
fn subject_dedupe_key(subject_dn: &str) -> String {
    subject_dn
        .split(',')
        .map(|p| p.trim().to_ascii_lowercase())
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(",")
}

/// Si el mismo certificado está en chip (PKCS#11) y en MY, conserva solo la entrada del lector.
pub fn dedupe_signing_certs_prefer_pkcs11(certs: Vec<SigningCertSummary>) -> Vec<SigningCertSummary> {
    let mut pkcs11_thumbs: HashSet<String> = HashSet::new();
    let mut pkcs11_subjects: HashSet<String> = HashSet::new();

    for c in &certs {
        if c.source != SigningCertSource::Pkcs11 {
            continue;
        }
        if !c.cert_thumbprint_sha1_hex.is_empty() {
            pkcs11_thumbs.insert(c.cert_thumbprint_sha1_hex.clone());
        }
        let subj = subject_dedupe_key(&c.subject_dn);
        if !subj.is_empty() {
            pkcs11_subjects.insert(subj);
        }
    }

    certs
        .into_iter()
        .filter(|c| {
            if c.source != SigningCertSource::WinMy {
                return true;
            }
            if !c.cert_thumbprint_sha1_hex.is_empty()
                && pkcs11_thumbs.contains(&c.cert_thumbprint_sha1_hex)
            {
                return false;
            }
            let subj = subject_dedupe_key(&c.subject_dn);
            !(subj.is_empty() || !pkcs11_subjects.contains(&subj))
        })
        .collect()
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
    /// Huella SHA-1 del DER (hex minúsculas); permite ocultar duplicados MY cuando ya hay chip.
    pub cert_thumbprint_sha1_hex: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cert(
        id: &str,
        source: SigningCertSource,
        thumb: &str,
        subject: &str,
    ) -> SigningCertSummary {
        SigningCertSummary {
            id_hex: id.to_string(),
            label: id.to_string(),
            subject_dn: subject.to_string(),
            source,
            pin_ui: SigningPinUi::RequiredInApp,
            cert_thumbprint_sha1_hex: thumb.to_string(),
        }
    }

    #[test]
    fn dedupe_hides_win_my_when_same_thumbprint_on_chip() {
        let chip = cert(
            "aa",
            SigningCertSource::Pkcs11,
            "deadbeef",
            "CN=Test",
        );
        let my = cert(
            "winmy:deadbeef",
            SigningCertSource::WinMy,
            "deadbeef",
            "CN=Test",
        );
        let out = dedupe_signing_certs_prefer_pkcs11(vec![chip, my]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].source, SigningCertSource::Pkcs11);
    }

    #[test]
    fn dedupe_keeps_win_my_when_no_chip_match() {
        let my = cert(
            "winmy:abc",
            SigningCertSource::WinMy,
            "abc",
            "CN=Solo Windows",
        );
        let out = dedupe_signing_certs_prefer_pkcs11(vec![my]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].source, SigningCertSource::WinMy);
    }

    #[test]
    fn dedupe_falls_back_to_subject_when_thumbprint_missing() {
        let chip = cert("aa", SigningCertSource::Pkcs11, "", "CN=Same, SN=1");
        let my = cert("winmy:x", SigningCertSource::WinMy, "", "cn=same, sn=1");
        let out = dedupe_signing_certs_prefer_pkcs11(vec![chip, my]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].source, SigningCertSource::Pkcs11);
    }
}
