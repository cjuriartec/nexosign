//! Filtrado “solo firma” a partir del DER X.509 (Key Usage).

use x509_parser::prelude::*;

/// `true` si Key Usage incluye **digitalSignature** y/o **nonRepudiation**.
pub fn der_is_signing_certificate(der: &[u8]) -> bool {
    let Ok((_, cert)) = X509Certificate::from_der(der) else {
        return false;
    };
    let Ok(Some(ext)) = cert.key_usage() else {
        return false;
    };
    let v = ext.value;
    v.digital_signature() || v.non_repudiation()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_digital_signature_accepted() {
        let der = include_bytes!("../../tests/fixtures/signing_digital_signature.der");
        assert!(der_is_signing_certificate(der.as_slice()));
    }

    #[test]
    fn fixture_auth_only_rejected() {
        let der = include_bytes!("../../tests/fixtures/auth_only.der");
        assert!(!der_is_signing_certificate(der.as_slice()));
    }
}
