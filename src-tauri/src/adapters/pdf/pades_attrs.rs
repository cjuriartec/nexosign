//! Atributos CMS/PAdES exigidos por validadores estrictos (Firma Perú, Refirma, ETSI PAdES-BES).

use cms::builder::create_signing_time_attribute;
use der::asn1::{Any, SetOfVec};
use der::Tag;
use sha2::{Digest, Sha256};
use spki::AlgorithmIdentifierOwned;
use x509_cert::attr::{Attribute, AttributeValue};
use const_oid::db::rfc5911::ID_AA_SIGNING_CERTIFICATE_V_2;
use const_oid::db::rfc5912::ID_SHA_256;

/// `AlgorithmIdentifier` SHA-256 con parámetro NULL explícito (algunos validadores lo exigen).
pub fn sha256_digest_algorithm() -> AlgorithmIdentifierOwned {
    AlgorithmIdentifierOwned {
        oid: ID_SHA_256,
        parameters: Some(Any::null()),
    }
}

fn wrap_sequence(content: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(content.len() + 4);
    out.push(0x30);
    if content.len() < 128 {
        out.push(content.len() as u8);
    } else {
        out.push(0x81);
        out.push(content.len() as u8);
    }
    out.extend_from_slice(content);
    out
}

/// ESS `SigningCertificateV2` (RFC 5035) — obligatorio en PAdES-BES con SHA-256.
fn signing_certificate_v2_der(cert_der: &[u8]) -> Vec<u8> {
    let hash = Sha256::digest(cert_der);
    // AlgorithmIdentifier SHA-256
    let alg_id: [u8; 15] = [
        0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01, 0x05, 0x00,
    ];
    let mut ess_cert_id_v2 = Vec::with_capacity(alg_id.len() + 34);
    ess_cert_id_v2.extend_from_slice(&alg_id);
    ess_cert_id_v2.push(0x04);
    ess_cert_id_v2.push(0x20);
    ess_cert_id_v2.extend_from_slice(&hash);
    let ess_wrapped = wrap_sequence(&ess_cert_id_v2);
    let certs_of = wrap_sequence(&ess_wrapped);
    wrap_sequence(&certs_of)
}

/// Atributo firmado `id-aa-signingCertificateV2`.
pub fn create_signing_certificate_v2_attribute(cert_der: &[u8]) -> Result<Attribute, der::Error> {
    let der = signing_certificate_v2_der(cert_der);
    let value = AttributeValue::new(Tag::Sequence, der)?;
    let mut values = SetOfVec::<AttributeValue>::new();
    values.insert(value)?;
    Ok(Attribute {
        oid: ID_AA_SIGNING_CERTIFICATE_V_2,
        values,
    })
}

/// Atributos firmados estándar PAdES-BES además de los que añade `SignerInfoBuilder`.
pub fn pades_bes_signed_attributes(cert_der: &[u8]) -> Result<Vec<Attribute>, der::Error> {
  let mut attrs = Vec::new();
    attrs.push(create_signing_certificate_v2_attribute(cert_der)?);
    attrs.push(
        create_signing_time_attribute().map_err(|_| der::Error::from(der::ErrorKind::Failed))?,
    );
    Ok(attrs)
}

#[cfg(windows)]
pub fn certificate_chain_der_windows(leaf_der: &[u8]) -> Vec<Vec<u8>> {
    use windows::Win32::Security::Cryptography::{
        CertCloseStore, CertFindCertificateInStore, CertFreeCertificateChain,
        CertFreeCertificateContext, CertGetCertificateChain, CertOpenSystemStoreW,
        CERT_CHAIN_CONTEXT, CERT_CHAIN_ELEMENT, CERT_CHAIN_PARA, CERT_FIND_EXISTING,
        CERT_QUERY_ENCODING_TYPE, PKCS_7_ASN_ENCODING, X509_ASN_ENCODING,
    };
    use windows::core::w;

    unsafe {
        let store = match CertOpenSystemStoreW(None, w!("MY")) {
            Ok(s) => s,
            Err(_) => return vec![leaf_der.to_vec()],
        };
        let enc = CERT_QUERY_ENCODING_TYPE(X509_ASN_ENCODING.0 | PKCS_7_ASN_ENCODING.0);
        let mut find_blob = windows::Win32::Security::Cryptography::CRYPT_INTEGER_BLOB {
            cbData: leaf_der.len() as u32,
            pbData: leaf_der.as_ptr() as *mut u8,
        };
        let ctx = CertFindCertificateInStore(
            store,
            enc,
            0,
            CERT_FIND_EXISTING,
            Some(std::ptr::addr_of_mut!(find_blob).cast()),
            None,
        );
        if ctx.is_null() {
            let _ = CertCloseStore(Some(store), 0);
            return vec![leaf_der.to_vec()];
        }

        let mut chain_para = CERT_CHAIN_PARA::default();
        chain_para.cbSize = std::mem::size_of::<CERT_CHAIN_PARA>() as u32;
        let mut chain: *mut CERT_CHAIN_CONTEXT = std::ptr::null_mut();
        let chain_ok = CertGetCertificateChain(
            None,
            ctx,
            None,
            None,
            &chain_para,
            0,
            None,
            &mut chain,
        )
        .is_ok();

        let mut out = Vec::new();
        if chain_ok && !chain.is_null() {
            let chain_ref = &*chain;
            if !chain_ref.rgpChain.is_null() && chain_ref.cChain > 0 {
                let simple = &**chain_ref.rgpChain;
                if !simple.rgpElement.is_null() && simple.cElement > 0 {
                    for i in 0..simple.cElement {
                        let el_ptr = *simple.rgpElement.add(i as usize);
                        if el_ptr.is_null() {
                            continue;
                        }
                        let el: &CERT_CHAIN_ELEMENT = &*el_ptr;
                        if el.pCertContext.is_null() {
                            continue;
                        }
                        let cert_ctx = &*el.pCertContext;
                        let der = std::slice::from_raw_parts(
                            cert_ctx.pbCertEncoded,
                            cert_ctx.cbCertEncoded as usize,
                        );
                        out.push(der.to_vec());
                    }
                }
            }
            CertFreeCertificateChain(chain);
        }

        let _ = CertFreeCertificateContext(Some(ctx));
        let _ = CertCloseStore(Some(store), 0);

        if out.is_empty() {
            vec![leaf_der.to_vec()]
        } else {
            out
        }
    }
}

#[cfg(not(windows))]
pub fn certificate_chain_der_windows(leaf_der: &[u8]) -> Vec<Vec<u8>> {
    vec![leaf_der.to_vec()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signing_certificate_v2_has_expected_size() {
        let fake_cert = vec![0u8; 64];
        let der = signing_certificate_v2_der(&fake_cert);
        assert!(der.starts_with(&[0x30]));
        assert!(der.len() > 40 && der.len() < 120);
    }
}
