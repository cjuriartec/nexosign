//! Adaptador `rsa::pkcs1v15` + `cms` que delega la firma en PKCS#11 (`CKM_SHA256_RSA_PKCS`).

use std::sync::Arc;

use const_oid::db::rfc5912::SHA_256_WITH_RSA_ENCRYPTION;
use der::{Decode, Encode, asn1::Any};
use rsa::pkcs1v15::VerifyingKey;
use rsa::pkcs8::DecodePublicKey;
use rsa::{RsaPublicKey, pkcs1v15};
use sha2::Sha256;
use signature::{Keypair, Signer};
use spki::{AlgorithmIdentifierOwned, DynSignatureAlgorithmIdentifier};
use x509_cert::Certificate;

use crate::adapters::pkcs11::error::TokenError;
use crate::adapters::pkcs11::token::Pkcs11TokenManager;

pub struct Pkcs11RsaCmsSigner {
    pub token: Arc<Pkcs11TokenManager>,
    pub cert_id_hex: String,
    verifying_key: VerifyingKey<Sha256>,
}

impl Pkcs11RsaCmsSigner {
    pub fn new(token: Arc<Pkcs11TokenManager>, cert_id_hex: String, cert_der: &[u8]) -> Result<Self, TokenError> {
        let cert = Certificate::from_der(cert_der).map_err(|_| TokenError::BadCertId)?;
        let spki_der = cert
            .tbs_certificate
            .subject_public_key_info
            .to_der()
            .map_err(|_| TokenError::UnsupportedKeyType)?;
        let pk = RsaPublicKey::from_public_key_der(&spki_der).map_err(|_| TokenError::UnsupportedKeyType)?;
        let verifying_key = VerifyingKey::<Sha256>::new_unprefixed(pk);
        Ok(Self {
            token,
            cert_id_hex,
            verifying_key,
        })
    }
}

impl Keypair for Pkcs11RsaCmsSigner {
    type VerifyingKey = VerifyingKey<Sha256>;

    fn verifying_key(&self) -> Self::VerifyingKey {
        self.verifying_key.clone()
    }
}

impl DynSignatureAlgorithmIdentifier for Pkcs11RsaCmsSigner {
    fn signature_algorithm_identifier(&self) -> spki::Result<AlgorithmIdentifierOwned> {
        Ok(AlgorithmIdentifierOwned {
            oid: SHA_256_WITH_RSA_ENCRYPTION,
            parameters: Some(Any::null()),
        })
    }
}

impl Signer<pkcs1v15::Signature> for Pkcs11RsaCmsSigner {
    fn try_sign(&self, msg: &[u8]) -> Result<pkcs1v15::Signature, signature::Error> {
        let sig = self
            .token
            .rsa_sha256_pkcs1_sign(&self.cert_id_hex, msg)
            .map_err(signature::Error::from_source)?;
        pkcs1v15::Signature::try_from(sig.as_slice()).map_err(signature::Error::from_source)
    }
}
