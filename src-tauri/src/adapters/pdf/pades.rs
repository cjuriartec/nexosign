//! PAdES-BES mínimo: firma CMS detached (`adbe.pkcs7.detached`) con contenido externo (digest PDF).

use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use cms::builder::{SignerInfoBuilder, create_signing_time_attribute};
use cms::cert::{CertificateChoices, IssuerAndSerialNumber};
use cms::content_info::{CmsVersion, ContentInfo};
use cms::signed_data::{
    CertificateSet, DigestAlgorithmIdentifiers, EncapsulatedContentInfo, SignerIdentifier, SignedData,
    SignerInfos,
};
use const_oid::db::rfc5911::{ID_DATA, ID_SIGNED_DATA};
use const_oid::db::rfc5912::ID_SHA_256;
use der::{Any, AnyRef, Decode, Encode};
use image::GenericImageView;
use lopdf::{Dictionary, Document, IncrementalDocument, Object, Stream, StringFormat};
use sha2::{Digest, Sha256};
use spki::{AlgorithmIdentifierOwned, DynSignatureAlgorithmIdentifier};
use x509_cert::Certificate;
use x509_cert::builder::Builder;
use chrono;

use signature::{Keypair, Signer};

use crate::adapters::pdf::cms_signer::Pkcs11RsaCmsSigner;
use crate::adapters::pkcs11::token::Pkcs11TokenManager;
use crate::application::errors::SignBatchError;
use crate::ports::pdf_pades_signer::{PdfPadesSigner, SignatureGridPlacement};

#[cfg(windows)]
use crate::adapters::pdf::win_my_cms_signer::WinMyRsaCmsSigner;
#[cfg(windows)]
use crate::domain::signing_cert::WIN_MY_CERT_ID_PREFIX;

/// Rejilla visible en la primera página (ancho × alto).
const SIG_GRID_COLS: f64 = 3.0;
const SIG_GRID_ROWS: f64 = 5.0;

fn find_sub(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// `lopdf` puede escribir `/SubFilter/adbe.pkcs7.detached` (sin espacio) o con espacio; ambos son PDF válidos.
fn find_subfilter_adobe_pkcs7_detached(buf: &[u8]) -> Option<usize> {
    const KEY: &[u8] = b"/SubFilter";
    let mut search_from = 0usize;
    while search_from < buf.len() {
        let rel = find_sub(&buf[search_from..], KEY)?;
        let i = search_from + rel;
        let mut j = i + KEY.len();
        while j < buf.len() && buf[j].is_ascii_whitespace() {
            j += 1;
        }
        if buf[j..].starts_with(b"/adbe.pkcs7.detached") {
            return Some(i);
        }
        search_from = i.saturating_add(1);
    }
    None
}

/// Posición del `<` que abre el hex string de `/Contents` del diccionario de firma (busca desde `from`).
fn find_contents_hex_angle_open(buf: &[u8], from: usize) -> Option<usize> {
    const KEY: &[u8] = b"/Contents";
    let mut search_from = from;
    while search_from < buf.len() {
        let rel = find_sub(&buf[search_from..], KEY)?;
        let key_start = search_from + rel;
        let mut j = key_start + KEY.len();
        while j < buf.len() && buf[j].is_ascii_whitespace() {
            j += 1;
        }
        if buf.get(j) == Some(&b'<') {
            return Some(j);
        }
        search_from = key_start.saturating_add(1);
    }
    None
}

/// Calcula digest SHA-256 del PDF excluyendo el contenido hexadecimal de `/Contents < ... >`.
fn digest_pdf_pkcs7_detached(buf: &[u8]) -> Result<[u8; 32], SignBatchError> {
    let base = find_subfilter_adobe_pkcs7_detached(buf).ok_or_else(|| {
        SignBatchError::Pades(
            "no se encontró el diccionario de firma (/SubFilter + adbe.pkcs7.detached) tras guardar".into(),
        )
    })?;
    let lt = find_contents_hex_angle_open(buf, base).ok_or_else(|| {
        SignBatchError::Pades("no se encontró /Contents <hex> para la firma".into())
    })?;
    debug_assert_eq!(buf.get(lt), Some(&b'<'));
    let tail = &buf[lt + 1..];
    let rel_gt = find_sub(tail, b">").ok_or_else(|| SignBatchError::Pades("sin '>' de cierre de Contents".into()))?;
    let gt = lt + 1 + rel_gt;
    let mut hasher = Sha256::new();
    hasher.update(&buf[..lt]);
    hasher.update(&buf[gt + 1..]);
    Ok(hasher.finalize().into())
}

fn patch_pdf_contents_der(buf: &mut Vec<u8>, cms_der: &[u8]) -> Result<(), SignBatchError> {
    let base = find_subfilter_adobe_pkcs7_detached(buf.as_slice())
        .ok_or_else(|| SignBatchError::Pades("marker cms".into()))?;
    let lt = find_contents_hex_angle_open(buf.as_slice(), base)
        .ok_or_else(|| SignBatchError::Pades("/Contents".into()))?;
    if buf.get(lt) != Some(&b'<') {
        return Err(SignBatchError::Pades("formato /Contents inesperado".into()));
    }
    let inner_start = lt + 1;
    let tail = &buf[inner_start..];
    let rel_gt = find_sub(tail, b">").ok_or_else(|| SignBatchError::Pades("cierre contents".into()))?;
    let inner_end = inner_start + rel_gt;
    let expected_hex_len = inner_end - inner_start;
    let hex_str = hex::encode_upper(cms_der);
    if hex_str.len() > expected_hex_len {
        return Err(SignBatchError::Pades(format!(
            "tamaño CMS ({}) excede el hueco hex ({})",
            hex_str.len(),
            expected_hex_len
        )));
    }
    
    let hex_bytes = hex_str.as_bytes();
    buf[inner_start..inner_start + hex_bytes.len()].copy_from_slice(hex_bytes);
    
    let pad_len = expected_hex_len - hex_str.len();
    if pad_len > 0 {
        buf[inner_start + hex_bytes.len()..inner_end].fill(b'0');
    }
    
    Ok(())
}

/// Expone el motivo real del fallo (la crate `cms` lo ocultaba en `add_signer_info`).
/// El texto conserva el error original para soporte; el encabezado orienta al usuario.
fn map_signer_info_build_error(e: x509_cert::builder::Error) -> String {
    match e {
        x509_cert::builder::Error::Signature(inner) => format!(
            "No se ha podido firmar con el DNIe o la tarjeta: {inner} · Revisa el PIN, vuelve a conectar el lector y comprueba que el certificado sea de tipo RSA con SHA-256."
        ),
        x509_cert::builder::Error::Asn1(inner) => format!(
            "Error DER al construir el CMS (SignerInfo): {inner} · Suele ser atributos firmados o codificación de la firma; conserva este texto para soporte."
        ),
        x509_cert::builder::Error::PublicKey(inner) => format!(
            "Error de clave pública del certificado (SPKI): {inner}"
        ),
        other => other.to_string(),
    }
}

/// Equivalente a `SignedDataBuilder::calculate_version` para un único `SignerInfo` con `issuerAndSerialNumber` y certificado X.509.
fn signed_data_version_nexosign_bes() -> CmsVersion {
    CmsVersion::V1
}

use rsa::pkcs1v15::VerifyingKey;

fn build_cms_signed_data<S>(
    signer: &S,
    cert_der: &[u8],
    pdf_digest: &[u8; 32],
) -> Result<Vec<u8>, SignBatchError>
where
    S: Signer<rsa::pkcs1v15::Signature>
        + DynSignatureAlgorithmIdentifier
        + Keypair<VerifyingKey = VerifyingKey<Sha256>>,
{
    let cert = Certificate::from_der(cert_der).map_err(|e| SignBatchError::Pades(format!("cert DER: {e}")))?;
    let sid = SignerIdentifier::IssuerAndSerialNumber(IssuerAndSerialNumber {
        issuer: cert.tbs_certificate.issuer.clone(),
        serial_number: cert.tbs_certificate.serial_number.clone(),
    });

    let digest_algorithm = AlgorithmIdentifierOwned {
        oid: ID_SHA_256,
        parameters: None,
    };

    let encap = EncapsulatedContentInfo {
        econtent_type: ID_DATA,
        econtent: None,
    };

    let mut signer_info_builder =
        SignerInfoBuilder::new(signer, sid, digest_algorithm.clone(), &encap, Some(pdf_digest.as_slice()))
            .map_err(|e| SignBatchError::Pades(format!("SignerInfoBuilder: {e}")))?;
    signer_info_builder
        .add_signed_attribute(create_signing_time_attribute().map_err(|e| SignBatchError::Pades(format!("signingTime: {e}")))?)
        .map_err(|e| SignBatchError::Pades(format!("signed attr: {e}")))?;

    let signer_info = signer_info_builder
        .build::<rsa::pkcs1v15::Signature>()
        .map_err(|e| SignBatchError::Pades(map_signer_info_build_error(e)))?;

    let digest_algorithms = DigestAlgorithmIdentifiers::try_from(vec![digest_algorithm.clone()])
        .map_err(|e| SignBatchError::Pades(format!("digestAlgorithms CMS: {e}")))?;
    let certificate_set =
        CertificateSet::try_from(vec![CertificateChoices::Certificate(cert.clone())])
            .map_err(|e| SignBatchError::Pades(format!("CertificateSet CMS: {e}")))?;
    let signer_infos =
        SignerInfos::try_from(vec![signer_info]).map_err(|e| SignBatchError::Pades(format!("SignerInfos CMS: {e}")))?;

    let signed_data = SignedData {
        version: signed_data_version_nexosign_bes(),
        digest_algorithms,
        encap_content_info: encap,
        certificates: Some(certificate_set),
        crls: None,
        signer_infos,
    };

    let signed_data_der = signed_data
        .to_der()
        .map_err(|e| SignBatchError::Pades(format!("SignedData DER: {e}")))?;
    let content = AnyRef::try_from(signed_data_der.as_slice())
        .map_err(|e| SignBatchError::Pades(format!("CMS ContentInfo (AnyRef): {e}")))?;

    let ci = ContentInfo {
        content_type: ID_SIGNED_DATA,
        content: Any::from(content),
    };
    ci.to_der()
        .map_err(|e| SignBatchError::Pades(format!("ContentInfo DER: {e}")))
}

fn object_to_f64(obj: &Object) -> Result<f64, SignBatchError> {
    match obj {
        Object::Integer(i) => Ok(*i as f64),
        Object::Real(r) => Ok(f64::from(*r)),
        _ => Err(SignBatchError::Pades("coordenada PDF: número esperado".into())),
    }
}

fn array4_from_object(doc: &Document, obj: &Object) -> Result<[f64; 4], SignBatchError> {
    let arr = match obj {
        Object::Array(a) => a,
        Object::Reference(r) => {
            let o = doc
                .get_object(*r)
                .map_err(|e| SignBatchError::Pades(format!("MediaBox ref: {e}")))?;
            match o {
                Object::Array(a) => a,
                _ => {
                    return Err(SignBatchError::Pades(
                        "MediaBox: indirecto no es array".into(),
                    ));
                }
            }
        }
        _ => {
            return Err(SignBatchError::Pades("MediaBox: se esperaba array".into()));
        }
    };
    if arr.len() < 4 {
        return Err(SignBatchError::Pades("MediaBox: faltan valores".into()));
    }
    Ok([
        object_to_f64(&arr[0])?,
        object_to_f64(&arr[1])?,
        object_to_f64(&arr[2])?,
        object_to_f64(&arr[3])?,
    ])
}

/// MediaBox o CropBox de la primera página (la de menor número).
fn read_first_page_box(pdf_bytes: &[u8]) -> Result<[f64; 4], SignBatchError> {
    use std::io::Cursor;
    let doc = Document::load_from(Cursor::new(pdf_bytes))
        .map_err(|e| SignBatchError::Pades(format!("leer PDF (MediaBox): {e}")))?;
    let page_id = doc
        .get_pages()
        .into_iter()
        .next()
        .map(|(_, id)| id)
        .ok_or_else(|| SignBatchError::Pades("PDF sin páginas".into()))?;
    let page = doc
        .get_object(page_id)
        .map_err(|e| SignBatchError::Pades(format!("página: {e}")))?
        .as_dict()
        .map_err(|e| SignBatchError::Pades(format!("página no diccionario: {e}")))?;
    let mb = page
        .get(b"MediaBox")
        .or_else(|_| page.get(b"CropBox"))
        .map_err(|_| SignBatchError::Pades("página sin MediaBox ni CropBox".into()))?;
    array4_from_object(&doc, mb)
}

/// `Rect` pequeño en user space: fila 0 = cabecera del PDF, col 0 = izquierda.
fn rect_from_grid(page_box: [f64; 4], g: SignatureGridPlacement) -> [i64; 4] {
    let g = g.normalized();
    let page_llx = page_box[0];
    let page_lly = page_box[1];
    let page_urx = page_box[2];
    let page_ury = page_box[3];
    let w = (page_urx - page_llx).max(1.0);
    let h = (page_ury - page_lly).max(1.0);
    let margin = 0.0;
    let inner_w = w - 2.0 * margin;
    let inner_h = h - 2.0 * margin;
    let cell_w = inner_w / SIG_GRID_COLS;
    let cell_h = inner_h / SIG_GRID_ROWS;
    let col = f64::from(g.col);
    let row = f64::from(g.row);
    let x0 = page_llx + margin + col * cell_w;
    let y_cell_bottom = page_ury - margin - (row + 1.0) * cell_h;
    let widget_w = (cell_w * 0.58).clamp(70.0, 120.0);
    let widget_h = (cell_h * 0.40).clamp(24.0, 44.0);
    let llx = x0 + (cell_w - widget_w) * 0.5;
    let lly = y_cell_bottom + (cell_h - widget_h) * 0.5;
    let urx = llx + widget_w;
    let ury = lly + widget_h;
    let llx = llx.max(page_llx);
    let lly = lly.max(page_lly);
    let mut urx = urx.min(page_urx);
    let mut ury = ury.min(page_ury);
    if urx <= llx {
        urx = llx + 1.0;
    }
    if ury <= lly {
        ury = lly + 1.0;
    }
    [
        llx.round() as i64,
        lly.round() as i64,
        urx.round() as i64,
        ury.round() as i64,
    ]
}

fn pdf_escape_pdf_literal(s: &str) -> String {
    s.replace('\\', "\\\\").replace('(', "\\(").replace(')', "\\)")
}

/// `Rect` del widget según proporción del PNG del sello (caben imagen + texto como en Certificados).
fn rect_from_grid_with_aspect(page_box: [f64; 4], g: SignatureGridPlacement, aspect: f64) -> [i64; 4] {
    let g = g.normalized();
    let page_llx = page_box[0];
    let page_lly = page_box[1];
    let page_urx = page_box[2];
    let page_ury = page_box[3];
    let w = (page_urx - page_llx).max(1.0);
    let h = (page_ury - page_lly).max(1.0);
    let margin = 0.0;
    let inner_w = w - 2.0 * margin;
    let inner_h = h - 2.0 * margin;
    let cell_w = inner_w / SIG_GRID_COLS;
    let cell_h = inner_h / SIG_GRID_ROWS;
    let col = f64::from(g.col);
    let row = f64::from(g.row);
    let x0 = page_llx + margin + col * cell_w;
    let y_cell_bottom = page_ury - margin - (row + 1.0) * cell_h;

    let ar = aspect.clamp(0.25, 4.0);
    let max_w = cell_w * 0.40;
    let max_h = cell_h * 0.40;
    let mut widget_w = max_w;
    let mut widget_h = widget_w / ar;
    if widget_h > max_h {
        widget_h = max_h;
        widget_w = widget_h * ar;
    }

    let llx = if g.col == 0 {
        x0
    } else if f64::from(g.col) >= SIG_GRID_COLS - 1.0 {
        x0 + cell_w - widget_w
    } else {
        x0 + (cell_w - widget_w) * 0.5
    };

    let lly = if g.row == 0 {
        y_cell_bottom + cell_h - widget_h
    } else if f64::from(g.row) >= SIG_GRID_ROWS - 1.0 {
        y_cell_bottom
    } else {
        y_cell_bottom + (cell_h - widget_h) * 0.5
    };

    let urx = llx + widget_w;
    let ury = lly + widget_h;
    let llx = llx.max(page_llx);
    let lly = lly.max(page_lly);
    let mut urx = urx.min(page_urx);
    let mut ury = ury.min(page_ury);
    if urx <= llx {
        urx = llx + 1.0;
    }
    if ury <= lly {
        ury = lly + 1.0;
    }
    [
        llx.round() as i64,
        lly.round() as i64,
        urx.round() as i64,
        ury.round() as i64,
    ]
}

fn seal_png_dimensions(bytes: &[u8]) -> Result<(u32, u32), SignBatchError> {
    let img = image::load_from_memory(bytes)
        .map_err(|e| SignBatchError::Pades(format!("imagen sello: {e}")))?;
    Ok(img.dimensions())
}

fn create_image_and_smask(
    doc: &mut IncrementalDocument,
    png_bytes: &[u8],
) -> Result<(lopdf::ObjectId, u32, u32), SignBatchError> {
    use std::io::Write;
    use flate2::write::ZlibEncoder;
    use flate2::Compression;

    let dyn_img = image::load_from_memory(png_bytes)
        .map_err(|e| SignBatchError::Pades(format!("imagen sello: {e}")))?;
    let rgba = dyn_img.to_rgba8();
    let (iw, ih) = rgba.dimensions();
    if iw == 0 || ih == 0 {
        return Err(SignBatchError::Pades("imagen sello vacía".into()));
    }

    let mut rgb = Vec::with_capacity((iw * ih * 3) as usize);
    let mut alpha = Vec::with_capacity((iw * ih) as usize);
    let mut has_alpha = false;

    for p in rgba.pixels() {
        if p[3] < 255 {
            has_alpha = true;
        }
        if p[3] == 0 {
            rgb.extend_from_slice(&[255, 255, 255]);
        } else {
            rgb.extend_from_slice(&[p[0], p[1], p[2]]);
        }
        alpha.push(p[3]);
    }

    let mut rgb_z = ZlibEncoder::new(Vec::new(), Compression::default());
    rgb_z.write_all(&rgb).map_err(|e| SignBatchError::Pades(format!("zlib rgb: {e}")))?;
    let rgb_compressed = rgb_z.finish().map_err(|e| SignBatchError::Pades(format!("zlib rgb: {e}")))?;

    let mut smask_ref = None;
    if has_alpha {
        let mut a_z = ZlibEncoder::new(Vec::new(), Compression::default());
        a_z.write_all(&alpha).map_err(|e| SignBatchError::Pades(format!("zlib alpha: {e}")))?;
        let alpha_compressed = a_z.finish().map_err(|e| SignBatchError::Pades(format!("zlib alpha: {e}")))?;

        let mut smask_dict = Dictionary::new();
        smask_dict.set("Type", Object::Name(b"XObject".to_vec()));
        smask_dict.set("Subtype", Object::Name(b"Image".to_vec()));
        smask_dict.set("Width", Object::Integer(iw.into()));
        smask_dict.set("Height", Object::Integer(ih.into()));
        smask_dict.set("ColorSpace", Object::Name(b"DeviceGray".to_vec()));
        smask_dict.set("BitsPerComponent", Object::Integer(8));
        smask_dict.set("Filter", Object::Name(b"FlateDecode".to_vec()));
        let smask_stream = Stream::new(smask_dict, alpha_compressed);
        smask_ref = Some(doc.new_document.add_object(Object::Stream(smask_stream)));
    }

    let mut img_dict = Dictionary::new();
    img_dict.set("Type", Object::Name(b"XObject".to_vec()));
    img_dict.set("Subtype", Object::Name(b"Image".to_vec()));
    img_dict.set("Width", Object::Integer(iw.into()));
    img_dict.set("Height", Object::Integer(ih.into()));
    img_dict.set("ColorSpace", Object::Name(b"DeviceRGB".to_vec()));
    img_dict.set("BitsPerComponent", Object::Integer(8));
    img_dict.set("Filter", Object::Name(b"FlateDecode".to_vec()));
    if let Some(sm_ref) = smask_ref {
        img_dict.set("SMask", Object::Reference(sm_ref));
    }
    let img_stream = Stream::new(img_dict, rgb_compressed);
    let img_ref = doc.new_document.add_object(Object::Stream(img_stream));

    Ok((img_ref, iw, ih))
}

fn create_appearance_from_seal_png(
    doc: &mut IncrementalDocument,
    rect: [i64; 4],
    png_bytes: &[u8],
) -> Result<Object, SignBatchError> {
    let (img_ref, _iw, _ih) = create_image_and_smask(doc, png_bytes)?;

    let fw = (rect[2] - rect[0]).abs() as f64;
    let fh = (rect[3] - rect[1]).abs() as f64;

    let content = format!("q {} 0 0 {} 0 0 cm /Img0 Do Q", fw, fh);

    let mut xobj = Dictionary::new();
    xobj.set("Img0", Object::Reference(img_ref));

    let mut form_res = Dictionary::new();
    form_res.set("XObject", Object::Dictionary(xobj));

    let mut form_dict = Dictionary::new();
    form_dict.set("Type", Object::Name(b"XObject".to_vec()));
    form_dict.set("Subtype", Object::Name(b"Form".to_vec()));
    form_dict.set("FormType", Object::Integer(1));
    form_dict.set(
        "Matrix",
        Object::Array(vec![
            Object::Integer(1),
            Object::Integer(0),
            Object::Integer(0),
            Object::Integer(1),
            Object::Integer(0),
            Object::Integer(0),
        ]),
    );
    form_dict.set(
        "BBox",
        Object::Array(vec![
            Object::Real(0.0),
            Object::Real(0.0),
            Object::Real(fw as f32),
            Object::Real(fh as f32),
        ]),
    );
    form_dict.set("Resources", Object::Dictionary(form_res));
    let form_stream = Stream::new(form_dict, content.into_bytes());
    Ok(Object::Reference(
        doc.new_document
            .add_object(Object::Stream(form_stream)),
    ))
}

fn create_appearance_stream(
    doc: &mut IncrementalDocument,
    rect: [i64; 4],
    signer_name: &str,
) -> Result<Object, SignBatchError> {
    let w = (rect[2] - rect[0]).abs() as f64;
    let h = (rect[3] - rect[1]).abs() as f64;

    let mut resources = Dictionary::new();
    let mut font_dict = Dictionary::new();
    font_dict.set("Type", Object::Name(b"Font".to_vec()));
    font_dict.set("Subtype", Object::Name(b"Type1".to_vec()));
    font_dict.set("BaseFont", Object::Name(b"Helvetica".to_vec()));
    let font_id = doc.new_document.add_object(Object::Dictionary(font_dict));

    let mut fonts = Dictionary::new();
    fonts.set("F1", Object::Reference(font_id));
    resources.set("Font", Object::Dictionary(fonts));

    let mut xobject = Dictionary::new();
    xobject.set("Type", Object::Name(b"XObject".to_vec()));
    xobject.set("Subtype", Object::Name(b"Form".to_vec()));
    xobject.set("FormType", Object::Integer(1));
    xobject.set(
        "Matrix",
        Object::Array(vec![
            Object::Integer(1),
            Object::Integer(0),
            Object::Integer(0),
            Object::Integer(1),
            Object::Integer(0),
            Object::Integer(0),
        ]),
    );
    xobject.set(
        "BBox",
        Object::Array(vec![
            Object::Real(0.0),
            Object::Real(0.0),
            Object::Real(w as f32),
            Object::Real(h as f32),
        ]),
    );
    xobject.set("Resources", Object::Dictionary(resources));

    let now = chrono::Local::now().format("%d/%m/%Y %H:%M").to_string();
    let name_esc = pdf_escape_pdf_literal(signer_name);
    let now_esc = pdf_escape_pdf_literal(&now);

    let content = format!(
        "q 0.97 0.98 1 rg 0 0 {w} {h} re f Q \
         BT /F1 7 Tf 0.22 0.24 0.3 rg 8 {y1} Td (Firma digital PAdES) Tj \
         /F1 8 Tf 0.07 0.09 0.14 rg 0 -12 Td ({name}) Tj \
         /F1 6.5 Tf 0.4 0.42 0.46 rg 0 -10 Td ({now}) Tj ET",
        w = w,
        h = h,
        y1 = h - 13.0,
        name = name_esc,
        now = now_esc,
    );

    let stream = Stream::new(xobject, content.into_bytes());
    Ok(Object::Reference(doc.new_document.add_object(Object::Stream(stream))))
}

fn get_signer_name_from_der(der: &[u8]) -> String {
    let Ok((_, cert)) = x509_parser::parse_x509_certificate(der) else {
        return "Firmante Desconocido".into();
    };
    let name = cert.subject()
        .iter_common_name()
        .next()
        .and_then(|cn| cn.as_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| cert.subject().to_string());
    name
}

fn append_signature_objects(
    doc: &mut IncrementalDocument,
    der_placeholder_len: usize,
    rect: [i64; 4],
    signer_name: &str,
    seal_png: Option<&[u8]>,
) -> Result<(), SignBatchError> {
    let prev = doc.get_prev_documents();
    let page_id = prev
        .page_iter()
        .next()
        .ok_or_else(|| SignBatchError::Pades("PDF sin páginas".into()))?;
    let root_ref = prev
        .trailer
        .get(b"Root")
        .map_err(|e| SignBatchError::Pades(format!("trailer Root: {e}")))?
        .as_reference()
        .map_err(|e| SignBatchError::Pades(format!("Root ref: {e}")))?;

    doc.opt_clone_object_to_new_document(root_ref)
        .map_err(|e| SignBatchError::Pades(format!("catalogo: {e}")))?;
    doc.opt_clone_object_to_new_document(page_id)
        .map_err(|e| SignBatchError::Pades(format!("pagina: {e}")))?;

    let mut sig_dict = Dictionary::new();
    sig_dict.set("Type", Object::Name(b"Sig".to_vec()));
    sig_dict.set("Filter", Object::Name(b"Adobe.PPKLite".to_vec()));
    sig_dict.set("SubFilter", Object::Name(b"adbe.pkcs7.detached".to_vec()));
    sig_dict.set(
        "Contents",
        Object::String(vec![0u8; der_placeholder_len], StringFormat::Hexadecimal),
    );
    sig_dict.set(
        "ByteRange",
        Object::Array(vec![
            Object::Integer(0),
            Object::Integer(999_999_999_999_999),
            Object::Integer(999_999_999_999_999),
            Object::Integer(999_999_999_999_999),
        ]),
    );

    let now_pdf = chrono::Local::now().format("D:%Y%m%d%H%M%S%:z").to_string().replace(':', "'") + "'";
    sig_dict.set(
        "M",
        Object::String(
            now_pdf.into_bytes(),
            StringFormat::Literal,
        ),
    );

    let sig_id = doc.new_document.add_object(Object::Dictionary(sig_dict));

    let ap_ref = match seal_png {
        Some(bytes) => create_appearance_from_seal_png(doc, rect, bytes)?,
        None => create_appearance_stream(doc, rect, signer_name)?,
    };
    let mut ap_dict = Dictionary::new();
    ap_dict.set("N", ap_ref);

    let mut annot = Dictionary::new();
    annot.set("Type", Object::Name(b"Annot".to_vec()));
    annot.set("Subtype", Object::Name(b"Widget".to_vec()));
    annot.set("FT", Object::Name(b"Sig".to_vec()));
    annot.set("T", Object::String(b"Signature1".to_vec(), StringFormat::Literal));
    annot.set("V", Object::Reference(sig_id));
    annot.set("P", Object::Reference(page_id));
    annot.set(
        "Rect",
        Object::Array(vec![
            Object::Integer(rect[0]),
            Object::Integer(rect[1]),
            Object::Integer(rect[2]),
            Object::Integer(rect[3]),
        ]),
    );
    annot.set("F", Object::Integer(132));
    annot.set(
        "Border",
        Object::Array(vec![
            Object::Integer(0),
            Object::Integer(0),
            Object::Integer(0),
        ]),
    );
    annot.set("AP", Object::Dictionary(ap_dict));

    let annot_id = doc.new_document.add_object(Object::Dictionary(annot));

    let mut acro = Dictionary::new();
    acro.set(
        "Fields",
        Object::Array(vec![Object::Reference(annot_id)]),
    );
    acro.set("SigFlags", Object::Integer(3));

    let acro_id = doc.new_document.add_object(Object::Dictionary(acro));

    let catalog = doc
        .new_document
        .catalog_mut()
        .map_err(|e| SignBatchError::Pades(format!("catalog_mut: {e}")))?;
    catalog.set("AcroForm", Object::Reference(acro_id));

    let page = doc
        .new_document
        .get_object_mut(page_id)
        .and_then(Object::as_dict_mut)
        .map_err(|e| SignBatchError::Pades(format!("page {e}")))?;

    match page.get_mut(b"Annots") {
        Ok(Object::Array(arr)) => arr.push(Object::Reference(annot_id)),
        _ => {
            page.set("Annots", Object::Array(vec![Object::Reference(annot_id)]));
        }
    }

    Ok(())
}

fn patch_byte_range(buf: &mut [u8]) -> Result<(), SignBatchError> {
    let base = find_subfilter_adobe_pkcs7_detached(buf).ok_or_else(|| {
        SignBatchError::Pades(
            "no se encontró la firma provisional (/SubFilter + adbe.pkcs7.detached) en el PDF incremental"
                .into(),
        )
    })?;
    let lt = find_contents_hex_angle_open(buf, base).ok_or_else(|| SignBatchError::Pades("contents".into()))?;
    let tail = &buf[lt + 1..];
    let rel_gt = find_sub(tail, b">").ok_or_else(|| SignBatchError::Pades(">".into()))?;
    let gt = lt + 1 + rel_gt;

    let start2 = gt + 1;
    let len2 = buf.len().saturating_sub(start2);
    let br = format!("{} {} {} {}", 0, lt, start2, len2);
    let needle = b"/ByteRange";
    let br_rel = find_sub(&buf[base..], needle).ok_or_else(|| SignBatchError::Pades("/ByteRange".into()))?;
    let br_pos = base + br_rel;
    let open = br_pos + find_sub(&buf[br_pos..], b"[").ok_or_else(|| SignBatchError::Pades("[ br".into()))?;
    let close_rel = find_sub(&buf[open..], b"]").ok_or_else(|| SignBatchError::Pades("] br".into()))?;
    let close = open + close_rel;
    let inner = &buf[open + 1..close];
    if inner.len() < br.len() {
        return Err(SignBatchError::Pades("hueco /ByteRange demasiado pequeño".into()));
    }
    let pad = inner.len() - br.len();
    let mut replacement = br.into_bytes();
    replacement.extend(std::iter::repeat(b' ').take(pad));
    buf[open + 1..close].copy_from_slice(&replacement);
    Ok(())
}

/// Firma un PDF hacia `output_path` usando el token (PIN previo).
pub fn sign_pdf_pades_bes(
    token: Arc<Pkcs11TokenManager>,
    cert_id_hex: &str,
    input_path: &Path,
    output_path: &Path,
    placement: SignatureGridPlacement,
    seal_png: Option<&[u8]>,
) -> Result<(), SignBatchError> {
    let cert_der = token
        .certificate_der_by_id_hex(cert_id_hex)
        .map_err(|e| SignBatchError::Signer(e.to_string()))?;
    let pdf_bytes = std::fs::read(input_path).map_err(|e| SignBatchError::Io {
        path: input_path.to_path_buf(),
        source: e,
    })?;

    let page_box = read_first_page_box(&pdf_bytes)?;
    let rect = if let Some(png) = seal_png {
        match seal_png_dimensions(png) {
            Ok((iw, ih)) if iw > 0 && ih > 0 => rect_from_grid_with_aspect(
                page_box,
                placement,
                f64::from(iw) / f64::from(ih),
            ),
            _ => rect_from_grid(page_box, placement),
        }
    } else {
        rect_from_grid(page_box, placement)
    };

    let signer_name = get_signer_name_from_der(&cert_der);
    let mut der_cap = 8192usize;

    for _ in 0..25 {
        let mut doc: IncrementalDocument = pdf_bytes
            .as_slice()
            .try_into()
            .map_err(|e| SignBatchError::Pades(format!("PDF lectura: {e}")))?;

        append_signature_objects(&mut doc, der_cap, rect, &signer_name, seal_png)?;

        let mut buf = Vec::new();
        doc.save_to(&mut buf)
            .map_err(|e| SignBatchError::Pades(format!("save incremental: {e}")))?;

        patch_byte_range(&mut buf)?;

        let digest = digest_pdf_pkcs7_detached(&buf)?;

        let cms_signer = Pkcs11RsaCmsSigner::new(token.clone(), cert_id_hex.to_string(), &cert_der)
            .map_err(|e| SignBatchError::Signer(e.to_string()))?;
        let cms_der = build_cms_signed_data(&cms_signer, &cert_der, &digest)?;

        if cms_der.len() <= der_cap {
            patch_pdf_contents_der(&mut buf, &cms_der)?;
            if let Some(parent) = output_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| SignBatchError::Io {
                    path: parent.to_path_buf(),
                    source: e,
                })?;
            }
            let mut f = File::create(output_path).map_err(|e| SignBatchError::Io {
                path: output_path.to_path_buf(),
                source: e,
            })?;
            f.write_all(&buf).map_err(|e| SignBatchError::Io {
                path: output_path.to_path_buf(),
                source: e,
            })?;
            return Ok(());
        }

        der_cap = cms_der.len().max(4096);
        if der_cap != cms_der.len() {
            der_cap = cms_der.len();
        }
    }

    Err(SignBatchError::Pades(
        "no convergió el tamaño reservado para CMS".into(),
    ))
}

/// Firma con certificado del almacén **MY** (Windows, RSA CNG).
#[cfg(windows)]
pub fn sign_pdf_pades_bes_win_my(
    cert_id_hex: &str,
    input_path: &Path,
    output_path: &Path,
    placement: SignatureGridPlacement,
    seal_png: Option<&[u8]>,
) -> Result<(), SignBatchError> {
    let win = Arc::new(unsafe {
        WinMyRsaCmsSigner::from_cert_id_hex(cert_id_hex)
            .map_err(|e| SignBatchError::Signer(e.to_string()))?
    });
    let cert_der = win.cert_der.clone();
    let pdf_bytes = std::fs::read(input_path).map_err(|e| SignBatchError::Io {
        path: input_path.to_path_buf(),
        source: e,
    })?;

    let page_box = read_first_page_box(&pdf_bytes)?;
    let rect = if let Some(png) = seal_png {
        match seal_png_dimensions(png) {
            Ok((iw, ih)) if iw > 0 && ih > 0 => rect_from_grid_with_aspect(
                page_box,
                placement,
                f64::from(iw) / f64::from(ih),
            ),
            _ => rect_from_grid(page_box, placement),
        }
    } else {
        rect_from_grid(page_box, placement)
    };

    let signer_name = get_signer_name_from_der(&cert_der);
    let mut der_cap = 8192usize;

    for _ in 0..25 {
        let mut doc: IncrementalDocument = pdf_bytes
            .as_slice()
            .try_into()
            .map_err(|e| SignBatchError::Pades(format!("PDF lectura: {e}")))?;

        append_signature_objects(&mut doc, der_cap, rect, &signer_name, seal_png)?;

        let mut buf = Vec::new();
        doc.save_to(&mut buf)
            .map_err(|e| SignBatchError::Pades(format!("save incremental: {e}")))?;

        patch_byte_range(&mut buf)?;

        let digest = digest_pdf_pkcs7_detached(&buf)?;

        let cms_der = build_cms_signed_data(win.as_ref(), &cert_der, &digest)?;

        if cms_der.len() <= der_cap {
            patch_pdf_contents_der(&mut buf, &cms_der)?;
            if let Some(parent) = output_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| SignBatchError::Io {
                    path: parent.to_path_buf(),
                    source: e,
                })?;
            }
            let mut f = File::create(output_path).map_err(|e| SignBatchError::Io {
                path: output_path.to_path_buf(),
                source: e,
            })?;
            f.write_all(&buf).map_err(|e| SignBatchError::Io {
                path: output_path.to_path_buf(),
                source: e,
            })?;
            return Ok(());
        }

        der_cap = cms_der.len().max(4096);
        if der_cap != cms_der.len() {
            der_cap = cms_der.len();
        }
    }

    Err(SignBatchError::Pades(
        "no convergió el tamaño reservado para CMS".into(),
    ))
}

/// Adaptador PKCS#11 para el puerto [`PdfPadesSigner`].
pub struct Pkcs11PdfPadesSigner {
    pub token: Arc<Pkcs11TokenManager>,
}

impl PdfPadesSigner for Pkcs11PdfPadesSigner {
    fn ensure_signed_session(&self, pin: Option<&str>, cert_id_hex: &str) -> Result<(), String> {
        let Some(p) = pin else {
            return Ok(());
        };
        let pt = p.trim();
        if pt.is_empty() {
            return Ok(());
        }
        let _ = self.token.reset_pkcs11_driver_state();
        self.token
            .login_for_certificate(pt.to_string(), cert_id_hex)
            .map_err(|e| e.to_string())
    }

    fn sign_pdf_pades_bes(
        &self,
        cert_id_hex: &str,
        input_path: &Path,
        output_path: &Path,
        placement: SignatureGridPlacement,
        seal_png: Option<&[u8]>,
    ) -> Result<(), String> {
        sign_pdf_pades_bes(
            self.token.clone(),
            cert_id_hex,
            input_path,
            output_path,
            placement,
            seal_png,
        )
        .map_err(|e| e.to_string())
    }

    fn end_signed_session(&self) {
        let _ = self.token.logout();
    }
}

/// Enruta firma entre PKCS#11 y almacén MY de Windows.
#[cfg(windows)]
pub struct CompositePdfPadesSigner {
    pub pkcs11: Pkcs11PdfPadesSigner,
}

#[cfg(windows)]
impl PdfPadesSigner for CompositePdfPadesSigner {
    fn ensure_signed_session(&self, pin: Option<&str>, cert_id_hex: &str) -> Result<(), String> {
        if cert_id_hex.starts_with(WIN_MY_CERT_ID_PREFIX) {
            return Ok(());
        }
        self.pkcs11.ensure_signed_session(pin, cert_id_hex)
    }

    fn sign_pdf_pades_bes(
        &self,
        cert_id_hex: &str,
        input_path: &Path,
        output_path: &Path,
        placement: SignatureGridPlacement,
        seal_png: Option<&[u8]>,
    ) -> Result<(), String> {
        if cert_id_hex.starts_with(WIN_MY_CERT_ID_PREFIX) {
            return sign_pdf_pades_bes_win_my(
                cert_id_hex,
                input_path,
                output_path,
                placement,
                seal_png,
            )
            .map_err(|e| e.to_string());
        }
        self.pkcs11
            .sign_pdf_pades_bes(cert_id_hex, input_path, output_path, placement, seal_png)
    }

    fn end_signed_session(&self) {
        self.pkcs11.end_signed_session();
    }
}

#[cfg(test)]
mod tests {
    use super::{find_contents_hex_angle_open, find_subfilter_adobe_pkcs7_detached};

    #[test]
    fn subfilter_adobe_with_or_without_space() {
        let with_space = b"<</Type/Sig/SubFilter /adbe.pkcs7.detached/Contents<";
        let no_space = b"<</Type/Sig/SubFilter/adbe.pkcs7.detached/Contents<";
        assert!(find_subfilter_adobe_pkcs7_detached(with_space).is_some());
        assert!(find_subfilter_adobe_pkcs7_detached(no_space).is_some());
    }

    #[test]
    fn contents_hex_angle_after_optional_whitespace() {
        let buf = b"<</Type/Sig/SubFilter/adbe.pkcs7.detached/Contents   <001122>";
        let base = find_subfilter_adobe_pkcs7_detached(buf).unwrap();
        let lt = find_contents_hex_angle_open(buf, base).unwrap();
        assert_eq!(buf[lt], b'<');
    }
}
