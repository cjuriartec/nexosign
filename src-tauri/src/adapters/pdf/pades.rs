//! PAdES-BES mínimo: firma CMS detached (`adbe.pkcs7.detached`) con contenido externo (digest PDF).

use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use cms::builder::{
    SignedDataBuilder, SignerInfoBuilder, create_signing_time_attribute,
};
use cms::cert::{CertificateChoices, IssuerAndSerialNumber};
use cms::signed_data::{EncapsulatedContentInfo, SignerIdentifier};
use const_oid::db::rfc5911::ID_DATA;
use const_oid::db::rfc5912::ID_SHA_256;
use der::{Decode, Encode};
use lopdf::{Dictionary, Document, IncrementalDocument, Object, StringFormat};
use sha2::{Digest, Sha256};
use spki::AlgorithmIdentifierOwned;
use x509_cert::Certificate;

use crate::adapters::pdf::cms_signer::Pkcs11RsaCmsSigner;
use crate::adapters::pkcs11::token::Pkcs11TokenManager;
use crate::application::errors::SignBatchError;

/// Casilla 5×7 en la primera página: `col` 0..4 (izq→der), `row` 0..6 (arriba→abajo, como al leer el PDF).
#[derive(Clone, Copy, Debug)]
pub struct SignatureGridPlacement {
    pub col: u8,
    pub row: u8,
}

impl Default for SignatureGridPlacement {
    fn default() -> Self {
        Self { col: 2, row: 6 }
    }
}

impl SignatureGridPlacement {
    fn normalized(self) -> Self {
        Self {
            col: self.col.min(4),
            row: self.row.min(6),
        }
    }
}

fn find_sub(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Calcula digest SHA-256 del PDF excluyendo el contenido hexadecimal de `/Contents < ... >`.
fn digest_pdf_pkcs7_detached(buf: &[u8]) -> Result<[u8; 32], SignBatchError> {
    let marker = b"/SubFilter /adbe.pkcs7.detached";
    let base = find_sub(buf, marker).ok_or_else(|| {
        SignBatchError::Pades("no se encontró /SubFilter /adbe.pkcs7.detached tras guardar".into())
    })?;
    let slice = &buf[base..];
    let rel = find_sub(slice, b"/Contents <").ok_or_else(|| {
        SignBatchError::Pades("no se encontró /Contents < para la firma".into())
    })?;
    let lt = base + rel + b"/Contents ".len(); // apunta a `<`
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
    let marker = b"/SubFilter /adbe.pkcs7.detached";
    let base = find_sub(buf.as_slice(), marker).ok_or_else(|| SignBatchError::Pades("marker cms".into()))?;
    let slice = &buf[base..];
    let rel = find_sub(slice, b"/Contents <").ok_or_else(|| SignBatchError::Pades("/Contents".into()))?;
    let lt = base + rel + b"/Contents ".len();
    if buf.get(lt) != Some(&b'<') {
        return Err(SignBatchError::Pades("formato /Contents inesperado".into()));
    }
    let inner_start = lt + 1;
    let tail = &buf[inner_start..];
    let rel_gt = find_sub(tail, b">").ok_or_else(|| SignBatchError::Pades("cierre contents".into()))?;
    let inner_end = inner_start + rel_gt;
    let expected_hex_len = inner_end - inner_start;
    let hex_str = hex::encode_upper(cms_der);
    if hex_str.len() != expected_hex_len {
        return Err(SignBatchError::Pades(format!(
            "tamaño CMS ({}) no coincide con hueco hex ({})",
            hex_str.len(),
            expected_hex_len
        )));
    }
    buf[inner_start..inner_end].copy_from_slice(hex_str.as_bytes());
    Ok(())
}

fn build_cms_signed_data(
    signer: &Pkcs11RsaCmsSigner,
    cert_der: &[u8],
    pdf_digest: &[u8; 32],
) -> Result<Vec<u8>, SignBatchError> {
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

    let mut sd = SignedDataBuilder::new(&encap);
    sd.add_digest_algorithm(digest_algorithm.clone())
        .map_err(|e| SignBatchError::Pades(format!("digest alg: {e}")))?;
    sd.add_certificate(CertificateChoices::Certificate(cert.clone()))
        .map_err(|e| SignBatchError::Pades(format!("cert: {e}")))?;
    sd.add_signer_info::<Pkcs11RsaCmsSigner, rsa::pkcs1v15::Signature>(signer_info_builder)
        .map_err(|e| SignBatchError::Pades(format!("signer info: {e}")))?;

    let ci = sd.build().map_err(|e| SignBatchError::Pades(format!("SignedData: {e}")))?;
    ci.to_der()
        .map_err(|e| SignBatchError::Pades(format!("CMS DER: {e}")))
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
    let margin = (w.min(h) * 0.028).clamp(16.0, 44.0);
    let inner_w = w - 2.0 * margin;
    let inner_h = h - 2.0 * margin;
    let cell_w = inner_w / 5.0;
    let cell_h = inner_h / 7.0;
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

fn append_signature_objects(
    doc: &mut IncrementalDocument,
    der_placeholder_len: usize,
    rect: [i64; 4],
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
    sig_dict.set(
        "M",
        Object::String(
            b"D:20260101120000Z".to_vec(),
            StringFormat::Literal,
        ),
    );

    let sig_id = doc.new_document.add_object(Object::Dictionary(sig_dict));

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
    let marker = b"/SubFilter /adbe.pkcs7.detached";
    let base = find_sub(buf, marker).ok_or_else(|| {
        SignBatchError::Pades(
            "no se encontró la firma provisional (/SubFilter /adbe.pkcs7.detached) en el PDF incremental"
                .into(),
        )
    })?;
    let slice = &buf[base..];
    let rel = find_sub(slice, b"/Contents <").ok_or_else(|| SignBatchError::Pades("contents".into()))?;
    let lt = base + rel + b"/Contents ".len();
    let tail = &buf[lt + 1..];
    let rel_gt = find_sub(tail, b">").ok_or_else(|| SignBatchError::Pades(">".into()))?;
    let gt = lt + 1 + rel_gt;

    let start2 = gt + 1;
    let len2 = buf.len().saturating_sub(start2);
    let br = format!("{} {} {} {}", 0, lt, start2, len2);
    let needle = b"/ByteRange";
    let br_pos = find_sub(buf, needle).ok_or_else(|| SignBatchError::Pades("/ByteRange".into()))?;
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
) -> Result<(), SignBatchError> {
    let cert_der = token.certificate_der_by_id_hex(cert_id_hex)?;
    let pdf_bytes = std::fs::read(input_path).map_err(|e| SignBatchError::Io {
        path: input_path.to_path_buf(),
        source: e,
    })?;

    let page_box = read_first_page_box(&pdf_bytes)?;
    let rect = rect_from_grid(page_box, placement);

    let mut der_cap = 8192usize;

    for _ in 0..25 {
        let mut doc: IncrementalDocument = pdf_bytes
            .as_slice()
            .try_into()
            .map_err(|e| SignBatchError::Pades(format!("PDF lectura: {e}")))?;

        append_signature_objects(&mut doc, der_cap, rect)?;

        let mut buf = Vec::new();
        doc.save_to(&mut buf)
            .map_err(|e| SignBatchError::Pades(format!("save incremental: {e}")))?;

        patch_byte_range(&mut buf)?;

        let digest = digest_pdf_pkcs7_detached(&buf)?;

        let cms_signer = Pkcs11RsaCmsSigner::new(token.clone(), cert_id_hex.to_string(), &cert_der)
            .map_err(|e| SignBatchError::Token(e))?;
        let cms_der = build_cms_signed_data(&cms_signer, &cert_der, &digest)?;

        if cms_der.len() == der_cap {
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
