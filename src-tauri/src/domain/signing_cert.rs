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
            subj.is_empty() || !pkcs11_subjects.contains(&subj)
        })
        .collect()
}

/// Dónde reside la clave privada asociada a un certificado en el almacén MY (solo Windows).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WinMyKeyBinding {
    /// Clave en tarjeta inteligente / lector (referencia en MY).
    SmartCard,
    /// Clave software en este equipo.
    Software,
    /// No clasificado; se trata como tarjeta para la política de visibilidad.
    Unknown,
}

/// `true` si la lista incluye al menos un certificado de firma leído del chip (PKCS#11).
pub fn has_pkcs11_signing_cert(certs: &[SigningCertSummary]) -> bool {
    certs.iter().any(|c| c.source == SigningCertSource::Pkcs11)
}

/// Oculta entradas MY ligadas a tarjeta cuando el chip no está visible en PKCS#11.
pub fn apply_signing_cert_visibility_policy(certs: Vec<SigningCertSummary>) -> Vec<SigningCertSummary> {
    if has_pkcs11_signing_cert(&certs) {
        return certs;
    }
    certs
        .into_iter()
        .filter(|c| {
            if c.source != SigningCertSource::WinMy {
                return true;
            }
            match c.win_my_key_binding {
                Some(WinMyKeyBinding::Software) => true,
                Some(WinMyKeyBinding::SmartCard) | Some(WinMyKeyBinding::Unknown) | None => false,
            }
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
    /// Solo `win_my`: dónde está la clave privada (política de visibilidad).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub win_my_key_binding: Option<WinMyKeyBinding>,
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
        cert_with_binding(id, source, thumb, subject, None)
    }

    fn cert_my(
        id: &str,
        thumb: &str,
        subject: &str,
        binding: WinMyKeyBinding,
    ) -> SigningCertSummary {
        cert_with_binding(id, SigningCertSource::WinMy, thumb, subject, Some(binding))
    }

    fn cert_with_binding(
        id: &str,
        source: SigningCertSource,
        thumb: &str,
        subject: &str,
        win_my_key_binding: Option<WinMyKeyBinding>,
    ) -> SigningCertSummary {
        SigningCertSummary {
            id_hex: id.to_string(),
            label: id.to_string(),
            subject_dn: subject.to_string(),
            source,
            pin_ui: SigningPinUi::RequiredInApp,
            cert_thumbprint_sha1_hex: thumb.to_string(),
            win_my_key_binding,
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

    #[test]
    fn dedupe_keeps_win_my_when_mixed_with_pkcs11() {
        let chip = cert("aa", SigningCertSource::Pkcs11, "deadbeef", "CN=Test");
        let my1 = cert("winmy:deadbeef", SigningCertSource::WinMy, "deadbeef", "CN=Test");
        let my2 = cert("winmy:abc", SigningCertSource::WinMy, "abc", "CN=Solo Windows");
        let out = dedupe_signing_certs_prefer_pkcs11(vec![chip, my1, my2]);
        assert_eq!(out.len(), 2);
        assert!(out.iter().any(|c| c.source == SigningCertSource::Pkcs11 && c.cert_thumbprint_sha1_hex == "deadbeef"));
        assert!(out.iter().any(|c| c.source == SigningCertSource::WinMy && c.cert_thumbprint_sha1_hex == "abc"));
    }

    #[test]
    fn visibility_hides_my_smart_card_without_chip() {
        let my = cert_my("winmy:aa", "aa", "CN=DNI", WinMyKeyBinding::SmartCard);
        let out = apply_signing_cert_visibility_policy(vec![my]);
        assert!(out.is_empty());
    }

    #[test]
    fn visibility_keeps_my_software_without_chip() {
        let my = cert_my("winmy:bb", "bb", "CN=Soft", WinMyKeyBinding::Software);
        let out = apply_signing_cert_visibility_policy(vec![my]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].source, SigningCertSource::WinMy);
    }

    #[test]
    fn visibility_hides_my_unknown_without_chip() {
        let my = cert_my("winmy:cc", "cc", "CN=X", WinMyKeyBinding::Unknown);
        let out = apply_signing_cert_visibility_policy(vec![my]);
        assert!(out.is_empty());
    }

    #[test]
    fn visibility_keeps_chip_and_hides_orphan_my_after_dedupe() {
        let chip = cert("aa", SigningCertSource::Pkcs11, "deadbeef", "CN=Test");
        let my = cert_my(
            "winmy:deadbeef",
            "deadbeef",
            "CN=Test",
            WinMyKeyBinding::SmartCard,
        );
        let out = apply_signing_cert_visibility_policy(dedupe_signing_certs_prefer_pkcs11(vec![
            chip, my,
        ]));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].source, SigningCertSource::Pkcs11);
    }

    #[test]
    fn visibility_does_not_hide_my_smart_card_when_chip_present() {
        let chip = cert("aa", SigningCertSource::Pkcs11, "t1", "CN=A");
        let my_other = cert_my("winmy:t2", "t2", "CN=B", WinMyKeyBinding::SmartCard);
        let out = apply_signing_cert_visibility_policy(vec![chip, my_other]);
        assert_eq!(out.len(), 2);
    }
}
