//! Firmador CMS RSA (SHA-256 PKCS#1 v1.5) vía **CNG** para certificados del almacén MY.

use const_oid::db::rfc5912::SHA_256_WITH_RSA_ENCRYPTION;
use der::{Any, Decode, Encode};
use rsa::pkcs1v15::VerifyingKey;
use rsa::pkcs8::DecodePublicKey;
use rsa::traits::PublicKeyParts;
use rsa::{RsaPublicKey, pkcs1v15};
use sha2::Sha256;
use signature::{Keypair, Signer};
use spki::{AlgorithmIdentifierOwned, DynSignatureAlgorithmIdentifier};
use x509_cert::Certificate;

use crate::adapters::pdf::cms_signer::pad_rsa_signature_to_modulus;
use crate::adapters::windows_cert_store::{
    WinCertError, classify_pin_ui, find_my_cert_by_thumbprint, ncrypt_sign_sha256_pkcs1,
    parse_win_my_thumbprint,
};
use crate::domain::signing_cert::SigningPinUi;

/// RSA CMS con clave en almacén MY (reabre el certificado en cada firma).
pub struct WinMyRsaCmsSigner {
    pub cert_der: Vec<u8>,
    thumbprint: [u8; 20],
    pin_ui: SigningPinUi,
    verifying_key: VerifyingKey<Sha256>,
    rsa_signature_octets: usize,
}

impl WinMyRsaCmsSigner {
    /// Carga el certificado por huella, clasifica `pin_ui` y prepara la clave pública RSA.
    pub unsafe fn from_cert_id_hex(cert_id_hex: &str) -> Result<Self, WinCertError> {
        let thumb = parse_win_my_thumbprint(cert_id_hex).ok_or_else(|| {
            WinCertError::Api("id de certificado Windows inválido (se esperaba winmy: + SHA-1 hex)".into())
        })?;
        let ctx = find_my_cert_by_thumbprint(&thumb)?;
        let pin_ui = classify_pin_ui(ctx as *const _);
        let cert_der =
            std::slice::from_raw_parts((*ctx).pbCertEncoded, (*ctx).cbCertEncoded as usize).to_vec();
        let _ = windows::Win32::Security::Cryptography::CertFreeCertificateContext(Some(
            ctx.cast_const(),
        ));
        let cert = Certificate::from_der(&cert_der).map_err(|_| WinCertError::Api("DER X.509 inválido".into()))?;
        let spki_der = cert
            .tbs_certificate
            .subject_public_key_info
            .to_der()
            .map_err(|e| WinCertError::Api(format!("SPKI: {e}")))?;
        let pk = RsaPublicKey::from_public_key_der(&spki_der)
            .map_err(|_| WinCertError::Api("solo RSA en almacén MY (CNG)".into()))?;
        let rsa_signature_octets = pk.size();
        let verifying_key = VerifyingKey::<Sha256>::new_unprefixed(pk);
        Ok(Self {
            cert_der,
            thumbprint: thumb,
            pin_ui,
            verifying_key,
            rsa_signature_octets,
        })
    }
}

impl Keypair for WinMyRsaCmsSigner {
    type VerifyingKey = VerifyingKey<Sha256>;

    fn verifying_key(&self) -> Self::VerifyingKey {
        self.verifying_key.clone()
    }
}

impl DynSignatureAlgorithmIdentifier for WinMyRsaCmsSigner {
    fn signature_algorithm_identifier(&self) -> spki::Result<AlgorithmIdentifierOwned> {
        Ok(AlgorithmIdentifierOwned {
            oid: SHA_256_WITH_RSA_ENCRYPTION,
            parameters: Some(Any::null()),
        })
    }
}

impl Signer<pkcs1v15::Signature> for WinMyRsaCmsSigner {
    fn try_sign(&self, msg: &[u8]) -> Result<pkcs1v15::Signature, signature::Error> {
        let silent_first = matches!(self.pin_ui, SigningPinUi::HiddenUseOsCrypto);
        let sig = (|| -> Result<Vec<u8>, WinCertError> {
            unsafe {
                let ctx = find_my_cert_by_thumbprint(&self.thumbprint)?;
                let r = ncrypt_sign_sha256_pkcs1(ctx.cast_const(), silent_first, msg);
                let _ = windows::Win32::Security::Cryptography::CertFreeCertificateContext(Some(
                    ctx.cast_const(),
                ));
                r
            }
        })()
        .map_err(signature::Error::from_source)?;
        let sig = pad_rsa_signature_to_modulus(sig, self.rsa_signature_octets)
            .map_err(signature::Error::from_source)?;
        pkcs1v15::Signature::try_from(sig.as_slice()).map_err(signature::Error::from_source)
    }
}
