//! Contrato para firma RSA PKCS#1 v1.5 + SHA-256 sobre datos arbitrarios (p. ej. atributos CMS).

use std::sync::Arc;

use crate::adapters::pkcs11::error::TokenError;
use crate::adapters::pkcs11::token::Pkcs11TokenManager;

#[derive(Debug, Clone)]
pub struct DigestSignerError(pub String);

impl std::fmt::Display for DigestSignerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for DigestSignerError {}

impl From<TokenError> for DigestSignerError {
    fn from(e: TokenError) -> Self {
        DigestSignerError(e.to_string())
    }
}

/// Firma con token PKCS#11 y acceso al certificado X.509 en DER.
pub trait DigestSigner: Send + Sync {
    fn certificate_der(&self, cert_id_hex: &str) -> Result<Vec<u8>, DigestSignerError>;

    /// Firma con `CKM_SHA256_RSA_PKCS` (hash SHA-256 interno + RSA PKCS#1 v1.5).
    fn rsa_sha256_pkcs1_sign(&self, cert_id_hex: &str, data: &[u8])
        -> Result<Vec<u8>, DigestSignerError>;
}

impl DigestSigner for Arc<Pkcs11TokenManager> {
    fn certificate_der(&self, cert_id_hex: &str) -> Result<Vec<u8>, DigestSignerError> {
        Pkcs11TokenManager::certificate_der_by_id_hex(self.as_ref(), cert_id_hex)
            .map_err(DigestSignerError::from)
    }

    fn rsa_sha256_pkcs1_sign(
        &self,
        cert_id_hex: &str,
        data: &[u8],
    ) -> Result<Vec<u8>, DigestSignerError> {
        Pkcs11TokenManager::rsa_sha256_pkcs1_sign(self.as_ref(), cert_id_hex, data)
            .map_err(DigestSignerError::from)
    }
}
