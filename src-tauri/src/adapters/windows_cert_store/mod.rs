//! Certificados del almacén **Current User / MY** (solo Windows, RSA vía CNG).

use std::ffi::c_void;

use const_oid::ObjectIdentifier;
use der::Decode;
use sha2::{Digest, Sha256};
use thiserror::Error;
use windows::core::{w, BOOL, PCWSTR};
use windows::Win32::Security::Cryptography::{
    CertCloseStore, CertDuplicateCertificateContext, CertFindCertificateInStore,
    CertFreeCertificateContext, CertGetCertificateContextProperty, CertOpenSystemStoreW,
    CryptAcquireCertificatePrivateKey, CryptCreateHash, CryptDestroyHash, CryptHashData,
    CryptReleaseContext, CryptSignHashW, AT_KEYEXCHANGE, AT_SIGNATURE, BCRYPT_PKCS1_PADDING_INFO,
    BCRYPT_SHA256_ALGORITHM, CALG_SHA_256, CERT_CONTEXT, CERT_FIND_HAS_PRIVATE_KEY,
    CERT_FIND_SHA1_HASH, CERT_KEY_PROV_INFO_PROP_ID, CERT_KEY_SPEC, CERT_NCRYPT_KEY_SPEC,
    CERT_QUERY_ENCODING_TYPE, CERT_SHA1_HASH_PROP_ID, CRYPT_ACQUIRE_ALLOW_NCRYPT_KEY_FLAG,
    CRYPT_ACQUIRE_FLAGS, CRYPT_ACQUIRE_PREFER_NCRYPT_KEY_FLAG, CRYPT_ACQUIRE_SILENT_FLAG,
    CRYPT_INTEGER_BLOB, CRYPT_KEY_PROV_INFO, HCRYPTPROV_OR_NCRYPT_KEY_HANDLE, NCryptFreeObject,
    NCryptSignHash, NCRYPT_HANDLE,
    NCRYPT_KEY_HANDLE, NCRYPT_PAD_PKCS1_FLAG, PKCS_7_ASN_ENCODING, X509_ASN_ENCODING,
};

use crate::domain::cert_filter::der_is_signing_certificate;
use crate::domain::signing_cert::{
    SigningCertSource, SigningCertSummary, SigningPinUi, WIN_MY_CERT_ID_PREFIX,
};
use x509_cert::Certificate;
use x509_parser::prelude::FromDer;

const RSA_OID: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.1.1");

/// HRESULT / NTE_BAD_ALGID — proveedor no admite el algoritmo pedido.
const NTE_BAD_ALGID: u32 = 0x8009_0008;

fn acquire_flags(silent: bool) -> CRYPT_ACQUIRE_FLAGS {
    let mut f = CRYPT_ACQUIRE_ALLOW_NCRYPT_KEY_FLAG | CRYPT_ACQUIRE_PREFER_NCRYPT_KEY_FLAG;
    if silent {
        f |= CRYPT_ACQUIRE_SILENT_FLAG;
    }
    f
}

fn is_nte_bad_algid(err: &windows::core::Error) -> bool {
    err.code().0 as u32 == NTE_BAD_ALGID
}

/// DigestInfo DER para RSA PKCS#1 v1.5 + SHA-256 (cuando NCrypt no acepta `BCRYPT_SHA256_ALGORITHM`).
fn sha256_digest_info_der(hash: &[u8; 32]) -> Vec<u8> {
    let mut der = vec![
        0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01,
        0x05, 0x00, 0x04, 0x20,
    ];
    der.extend_from_slice(hash);
    der
}

#[derive(Debug, Error)]
pub enum WinCertError {
    #[error("API Windows: {0}")]
    Api(String),
    #[error("clave privada RSA no compatible con CNG ni CryptoAPI")]
    UnsupportedKey,
}

fn map_win(e: windows::core::Error) -> WinCertError {
    WinCertError::Api(e.to_string())
}

unsafe fn release_acquired_key(
    hkey: HCRYPTPROV_OR_NCRYPT_KEY_HANDLE,
    caller_free: BOOL,
    keyspec: CERT_KEY_SPEC,
) {
    if !caller_free.as_bool() {
        return;
    }
    if keyspec == CERT_NCRYPT_KEY_SPEC {
        let _ = NCryptFreeObject(NCRYPT_HANDLE(hkey.0));
    } else {
        let _ = CryptReleaseContext(hkey.0, 0);
    }
}

fn keyspec_is_legacy_rsa(keyspec: CERT_KEY_SPEC) -> bool {
    keyspec == AT_SIGNATURE || keyspec == AT_KEYEXCHANGE
}

unsafe fn crypt_acquire_key(
    ctx: *const CERT_CONTEXT,
    silent: bool,
) -> windows::core::Result<(HCRYPTPROV_OR_NCRYPT_KEY_HANDLE, CERT_KEY_SPEC, BOOL)> {
    let mut hkey = HCRYPTPROV_OR_NCRYPT_KEY_HANDLE::default();
    let mut keyspec = CERT_KEY_SPEC::default();
    let mut caller_free = BOOL::default();
    CryptAcquireCertificatePrivateKey(
        ctx,
        acquire_flags(silent),
        None,
        &mut hkey,
        Some(&mut keyspec),
        Some(&mut caller_free),
    )?;
    Ok((hkey, keyspec, caller_free))
}

/// Comprueba si podemos abrir la clave privada (CNG) sin UI; libera el handle.
unsafe fn silent_acquire_succeeds(ctx: *const CERT_CONTEXT) -> bool {
    match crypt_acquire_key(ctx, true) {
        Ok((h, ks, cf)) => {
            let ok = ks == CERT_NCRYPT_KEY_SPEC || keyspec_is_legacy_rsa(ks);
            release_acquired_key(h, cf, ks);
            ok
        }
        Err(_) => false,
    }
}

unsafe fn wstr_to_lower(p: windows::core::PWSTR) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let ptr = p.as_ptr() as *const u16;
    let mut out = Vec::new();
    for i in 0..4096 {
        let c = *ptr.add(i);
        if c == 0 {
            break;
        }
        out.push(c);
    }
    Some(String::from_utf16_lossy(&out).to_lowercase())
}

unsafe fn provider_name_lower(ctx: *const CERT_CONTEXT) -> Option<String> {
    let mut cb = 0u32;
    let _ = CertGetCertificateContextProperty(ctx, CERT_KEY_PROV_INFO_PROP_ID, None, &mut cb);
    if cb as usize <= std::mem::size_of::<CRYPT_KEY_PROV_INFO>() {
        return None;
    }
    let mut buf = vec![0u8; cb as usize];
    CertGetCertificateContextProperty(
        ctx,
        CERT_KEY_PROV_INFO_PROP_ID,
        Some(buf.as_mut_ptr().cast::<c_void>()),
        &mut cb,
    )
    .ok()?;
    let kpi = &*(buf.as_ptr() as *const CRYPT_KEY_PROV_INFO);
    wstr_to_lower(kpi.pwszProvName)
}

pub(crate) unsafe fn classify_pin_ui(ctx: *const CERT_CONTEXT) -> SigningPinUi {
    if let Some(name) = provider_name_lower(ctx) {
        if name.contains("smart card")
            || name.contains("scard")
            || name.contains("virtual smart card")
            || name.contains("minidriver")
        {
            return SigningPinUi::RequiredInApp;
        }
        if name.contains("microsoft software key storage") || name.contains("software key storage") {
            return if silent_acquire_succeeds(ctx) {
                SigningPinUi::HiddenUseOsCrypto
            } else {
                SigningPinUi::OsMayPrompt
            };
        }
    }
    if silent_acquire_succeeds(ctx) {
        SigningPinUi::HiddenUseOsCrypto
    } else {
        SigningPinUi::OsMayPrompt
    }
}

fn cert_is_rsa(der: &[u8]) -> bool {
    let Ok(cert) = Certificate::from_der(der) else {
        return false;
    };
    cert.tbs_certificate
        .subject_public_key_info
        .algorithm
        .oid
        == RSA_OID
}

unsafe fn sha1_thumbprint(ctx: *const CERT_CONTEXT) -> Result<[u8; 20], WinCertError> {
    let mut cb = 0u32;
    CertGetCertificateContextProperty(ctx, CERT_SHA1_HASH_PROP_ID, None, &mut cb).map_err(map_win)?;
    if cb != 20 {
        return Err(WinCertError::Api(format!(
            "huella SHA-1 inesperada: {} bytes",
            cb
        )));
    }
    let mut thumb = [0u8; 20];
    CertGetCertificateContextProperty(
        ctx,
        CERT_SHA1_HASH_PROP_ID,
        Some(thumb.as_mut_ptr().cast::<c_void>()),
        &mut cb,
    )
    .map_err(map_win)?;
    Ok(thumb)
}

fn subject_dn_from_der(der: &[u8]) -> String {
    let Ok((_, cert)) = x509_parser::prelude::X509Certificate::from_der(der) else {
        return String::new();
    };
    cert.subject().to_string()
}

/// Lista certificados de firma (KeyUsage nonRepudiation) con clave privada en MY (RSA, CNG o CSP legacy).
pub fn list_my_store_signing_rsa_certs() -> Result<Vec<SigningCertSummary>, WinCertError> {
    unsafe {
        let store = CertOpenSystemStoreW(None, w!("MY")).map_err(map_win)?;
        if store.is_invalid() {
            return Err(WinCertError::Api("CertOpenSystemStoreW devolvió handle inválido".into()));
        }
        let enc = CERT_QUERY_ENCODING_TYPE(X509_ASN_ENCODING.0 | PKCS_7_ASN_ENCODING.0);
        let mut prev: Option<*const CERT_CONTEXT> = None;
        let mut out = Vec::new();

        loop {
            let ctx = CertFindCertificateInStore(
                store,
                enc,
                0,
                CERT_FIND_HAS_PRIVATE_KEY,
                None,
                prev,
            );
            if ctx.is_null() {
                break;
            }
            let der = std::slice::from_raw_parts((*ctx).pbCertEncoded, (*ctx).cbCertEncoded as usize);
            if der_is_signing_certificate(der) && cert_is_rsa(der) {
                let thumb = sha1_thumbprint(ctx)?;
                let id_hex = format!("{}{}", WIN_MY_CERT_ID_PREFIX, hex::encode(thumb));
                let subject_dn = subject_dn_from_der(der);
                let pin_ui = classify_pin_ui(ctx as *const CERT_CONTEXT);
                let label = if subject_dn.is_empty() {
                    id_hex.clone()
                } else {
                    subject_dn.clone()
                };
                out.push(SigningCertSummary {
                    id_hex,
                    label,
                    subject_dn,
                    source: SigningCertSource::WinMy,
                    pin_ui,
                });
            }
            prev = Some(ctx.cast_const());
        }

        let _ = CertCloseStore(Some(store), 0);
        Ok(out)
    }
}

/// Interpreta `winmy:` + 40 hex chars (SHA-1) como bytes.
pub fn parse_win_my_thumbprint(cert_id_hex: &str) -> Option<[u8; 20]> {
    let rest = cert_id_hex.strip_prefix(WIN_MY_CERT_ID_PREFIX)?;
    if rest.len() != 40 {
        return None;
    }
    let bytes = hex::decode(rest).ok()?;
    if bytes.len() != 20 {
        return None;
    }
    let mut a = [0u8; 20];
    a.copy_from_slice(&bytes);
    Some(a)
}

/// Firma RSA SHA-256 PKCS#1 v1.5 con CSP legacy (`CryptHashData` sobre el mensaje CMS).
unsafe fn cryptapi_sign_sha256_pkcs1(
    hkey: HCRYPTPROV_OR_NCRYPT_KEY_HANDLE,
    keyspec: CERT_KEY_SPEC,
    message: &[u8],
) -> Result<Vec<u8>, WinCertError> {
    let hprov = hkey.0;
    let mut hhash = 0usize;
    CryptCreateHash(hprov, CALG_SHA_256, 0, 0, &mut hhash).map_err(|e| {
        WinCertError::Api(format!(
            "el proveedor de la tarjeta no admite SHA-256 en CryptoAPI ({e}); prueba el certificado del lector (chip)"
        ))
    })?;
    CryptHashData(hhash, message, 0).map_err(|e| {
        let _ = CryptDestroyHash(hhash);
        map_win(e)
    })?;
    let mut sig_len = 0u32;
    let spec = keyspec.0;
    CryptSignHashW(hhash, spec, None, 0, None, &mut sig_len).map_err(|e| {
        let _ = CryptDestroyHash(hhash);
        map_win(e)
    })?;
    let mut sig = vec![0u8; sig_len as usize];
    CryptSignHashW(hhash, spec, None, 0, Some(sig.as_mut_ptr()), &mut sig_len).map_err(map_win)?;
    let _ = CryptDestroyHash(hhash);
    sig.truncate(sig_len as usize);
    tracing::debug!(keyspec = spec, sig_len = sig.len(), "MY: firma RSA vía CryptoAPI");
    Ok(sig)
}

unsafe fn ncrypt_sign_with_padding(
    hkey: HCRYPTPROV_OR_NCRYPT_KEY_HANDLE,
    to_sign: &[u8],
    padding: &BCRYPT_PKCS1_PADDING_INFO,
) -> windows::core::Result<Vec<u8>> {
    let pad_ptr: *const c_void = (std::ptr::addr_of!(*padding) as *const BCRYPT_PKCS1_PADDING_INFO).cast();
    let flags = NCRYPT_PAD_PKCS1_FLAG;
    let mut cb = 0u32;
    NCryptSignHash(
        NCRYPT_KEY_HANDLE(hkey.0),
        Some(pad_ptr),
        to_sign,
        None,
        &mut cb,
        flags,
    )?;
    let mut sig = vec![0u8; cb as usize];
    NCryptSignHash(
        NCRYPT_KEY_HANDLE(hkey.0),
        Some(pad_ptr),
        to_sign,
        Some(&mut sig),
        &mut cb,
        flags,
    )?;
    sig.truncate(cb as usize);
    Ok(sig)
}

/// CNG: hash SHA-256 de 32 bytes; si el KSP no admite `SHA256`, firma DigestInfo con padding PKCS#1 directo.
unsafe fn ncrypt_sign_sha256_digest(
    hkey: HCRYPTPROV_OR_NCRYPT_KEY_HANDLE,
    hash: &[u8; 32],
) -> Result<Vec<u8>, WinCertError> {
    let padding_sha256 = BCRYPT_PKCS1_PADDING_INFO {
        pszAlgId: BCRYPT_SHA256_ALGORITHM,
    };
    match ncrypt_sign_with_padding(hkey, hash.as_slice(), &padding_sha256) {
        Ok(sig) => {
            tracing::debug!(sig_len = sig.len(), "MY: firma RSA vía CNG (SHA256)");
            Ok(sig)
        }
        Err(e) if is_nte_bad_algid(&e) => {
            let digest_info = sha256_digest_info_der(hash);
            let padding_pkcs1 = BCRYPT_PKCS1_PADDING_INFO {
                pszAlgId: PCWSTR::null(),
            };
            let sig = ncrypt_sign_with_padding(hkey, &digest_info, &padding_pkcs1).map_err(map_win)?;
            tracing::debug!(sig_len = sig.len(), "MY: firma RSA vía CNG (DigestInfo PKCS#1)");
            Ok(sig)
        }
        Err(e) => Err(map_win(e)),
    }
}

/// Firma RSA SHA-256 PKCS#1 v1.5 usando la clave privada del certificado MY (CNG o CryptoAPI).
///
/// `message` suele ser el DER de signed attributes CMS (~100+ bytes); se hashea con SHA-256
/// antes de firmar (equivalente a `CKM_SHA256_RSA_PKCS` en PKCS#11).
pub(crate) unsafe fn ncrypt_sign_sha256_pkcs1(
    ctx: *const CERT_CONTEXT,
    silent_first: bool,
    message: &[u8],
) -> Result<Vec<u8>, WinCertError> {
    let hash: [u8; 32] = Sha256::digest(message).into();

    let acquire = |silent: bool| {
        let mut hkey = HCRYPTPROV_OR_NCRYPT_KEY_HANDLE::default();
        let mut keyspec = CERT_KEY_SPEC::default();
        let mut caller_free = BOOL::default();
        CryptAcquireCertificatePrivateKey(
            ctx,
            acquire_flags(silent),
            None,
            &mut hkey,
            Some(&mut keyspec),
            Some(&mut caller_free),
        )
        .map(|_| (hkey, keyspec, caller_free))
    };

    let (hkey, keyspec, caller_free) = match acquire(silent_first) {
        Ok(t) => t,
        Err(_) if silent_first => acquire(false).map_err(map_win)?,
        Err(e) => return Err(map_win(e)),
    };

    if keyspec_is_legacy_rsa(keyspec) {
        let sig = cryptapi_sign_sha256_pkcs1(hkey, keyspec, message)?;
        release_acquired_key(hkey, caller_free, keyspec);
        return Ok(sig);
    }
    if keyspec != CERT_NCRYPT_KEY_SPEC {
        release_acquired_key(hkey, caller_free, keyspec);
        return Err(WinCertError::UnsupportedKey);
    }

    let sig = ncrypt_sign_sha256_digest(hkey, &hash)?;
    release_acquired_key(hkey, caller_free, keyspec);
    Ok(sig)
}

/// Duplica el contexto del certificado en MY por huella SHA-1 (caller: `CertFreeCertificateContext`).
pub unsafe fn find_my_cert_by_thumbprint(thumb: &[u8; 20]) -> Result<*mut CERT_CONTEXT, WinCertError> {
    let store = CertOpenSystemStoreW(None, w!("MY")).map_err(map_win)?;
    if store.is_invalid() {
        return Err(WinCertError::Api("CertOpenSystemStoreW".into()));
    }
    let enc = CERT_QUERY_ENCODING_TYPE(X509_ASN_ENCODING.0 | PKCS_7_ASN_ENCODING.0);
    let blob = CRYPT_INTEGER_BLOB {
        cbData: thumb.len() as u32,
        pbData: thumb.as_ptr() as *mut u8,
    };
    let ctx = CertFindCertificateInStore(
        store,
        enc,
        0,
        CERT_FIND_SHA1_HASH,
        Some(std::ptr::addr_of!(blob).cast::<c_void>()),
        None,
    );
    let dup = if ctx.is_null() {
        let _ = CertCloseStore(Some(store), 0);
        return Err(WinCertError::Api(
            "certificado no encontrado en MY (¿revocado o desinstalado?)".into(),
        ));
    } else {
        CertDuplicateCertificateContext(Some(ctx.cast_const()))
    };
    let _ = CertFreeCertificateContext(Some(ctx.cast_const()));
    let _ = CertCloseStore(Some(store), 0);
    if dup.is_null() {
        return Err(WinCertError::Api("CertDuplicateCertificateContext".into()));
    }
    Ok(dup)
}
