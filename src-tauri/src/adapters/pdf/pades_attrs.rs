//! Atributos CMS/PAdES exigidos por validadores estrictos (Firma Perú, Refirma, ETSI PAdES-BES).


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

fn encode_der_length(len: usize) -> Vec<u8> {
    if len < 128 {
        vec![len as u8]
    } else if len < 256 {
        vec![0x81, len as u8]
    } else if len < 65536 {
        vec![0x82, (len >> 8) as u8, (len & 0xFF) as u8]
    } else {
        vec![
            0x83,
            ((len >> 16) & 0xFF) as u8,
            ((len >> 8) & 0xFF) as u8,
            (len & 0xFF) as u8,
        ]
    }
}

fn wrap_sequence(content: &[u8]) -> Vec<u8> {
    let len_bytes = encode_der_length(content.len());
    let mut out = Vec::with_capacity(1 + len_bytes.len() + content.len());
    out.push(0x30);
    out.extend_from_slice(&len_bytes);
    out.extend_from_slice(content);
    out
}

/// ESS `SigningCertificateV2` (RFC 5035) — obligatorio en PAdES-BES con SHA-256.
fn signing_certificate_v2_der(cert_der: &[u8]) -> Result<Vec<u8>, der::Error> {
    let (_, parsed_cert) = x509_parser::parse_x509_certificate(cert_der)
        .map_err(|_| der::Error::from(der::ErrorKind::Failed))?;
        
    let hash = Sha256::digest(cert_der);
    let mut cert_hash_der = Vec::new();
    cert_hash_der.push(0x04); // OCTET STRING
    cert_hash_der.push(0x20); // Length: 32 bytes
    cert_hash_der.extend_from_slice(&hash);
    
    let raw_serial = parsed_cert.tbs_certificate.raw_serial();
    let issuer_raw = parsed_cert.tbs_certificate.issuer.as_raw();
    
    // directoryName [4] Name
    let mut directory_name = Vec::new();
    directory_name.push(0xA4);
    directory_name.extend_from_slice(&encode_der_length(issuer_raw.len()));
    directory_name.extend_from_slice(issuer_raw);
    
    // generalNames SEQUENCE OF GeneralName
    let general_names = wrap_sequence(&directory_name);
    
    // serialNumber INTEGER
    let mut serial_number_der = Vec::new();
    serial_number_der.push(0x02);
    serial_number_der.extend_from_slice(&encode_der_length(raw_serial.len()));
    serial_number_der.extend_from_slice(raw_serial);
    
    // issuerSerial SEQUENCE
    let mut issuer_serial_content = Vec::new();
    issuer_serial_content.extend_from_slice(&general_names);
    issuer_serial_content.extend_from_slice(&serial_number_der);
    let issuer_serial = wrap_sequence(&issuer_serial_content);
    
    // ESSCertIDv2 SEQUENCE
    let mut ess_cert_id_v2_content = Vec::new();
    ess_cert_id_v2_content.extend_from_slice(&cert_hash_der);
    ess_cert_id_v2_content.extend_from_slice(&issuer_serial);
    let ess_cert_id_v2 = wrap_sequence(&ess_cert_id_v2_content);
    
    // certs SEQUENCE OF ESSCertIDv2
    let certs = wrap_sequence(&ess_cert_id_v2);
    Ok(certs)
}

/// Atributo firmado `id-aa-signingCertificateV2`.
pub fn create_signing_certificate_v2_attribute(cert_der: &[u8]) -> Result<Attribute, der::Error> {
    let der = signing_certificate_v2_der(cert_der)?;
    let value = AttributeValue::new(Tag::Sequence, der)?;
    let mut values = SetOfVec::<AttributeValue>::new();
    values.insert(value)?;
    Ok(Attribute {
        oid: ID_AA_SIGNING_CERTIFICATE_V_2,
        values,
    })
}

/// Atributos firmados estándar PAdES-BES además de los que añade `SignerInfoBuilder`.
/// NOTA: NO incluir `signingTime` aquí. Per ETSI EN 319 122-1, cuando se usa
/// `/SubFilter /ETSI.CAdES.detached`, la hora de firma se transmite exclusivamente
/// en la entrada `/M` del diccionario de firma PDF, no en atributos CMS.
pub fn pades_bes_signed_attributes(cert_der: &[u8]) -> Result<Vec<Attribute>, der::Error> {
    let mut attrs = Vec::new();
    attrs.push(create_signing_certificate_v2_attribute(cert_der)?);
    Ok(attrs)
}

#[cfg(windows)]
pub fn certificate_chain_der_windows(leaf_der: &[u8]) -> Vec<Vec<u8>> {
    use windows::Win32::Security::Cryptography::{
        CertFreeCertificateChain, CertFreeCertificateContext, CertGetCertificateChain,
        CERT_CHAIN_CONTEXT, CERT_CHAIN_ELEMENT, CERT_CHAIN_PARA,
        CERT_QUERY_ENCODING_TYPE, PKCS_7_ASN_ENCODING, X509_ASN_ENCODING,
        CertCreateCertificateContext,
    };

    unsafe {
        let enc = CERT_QUERY_ENCODING_TYPE(X509_ASN_ENCODING.0 | PKCS_7_ASN_ENCODING.0);
        let ctx = CertCreateCertificateContext(enc, leaf_der);
        if ctx.is_null() {
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
    fn signing_certificate_v2_fails_on_invalid_cert() {
        let fake_cert = vec![0u8; 64];
        assert!(signing_certificate_v2_der(&fake_cert).is_err());
    }
}
