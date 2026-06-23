//! PKCS#11 token manager (initialize, list signing certs, PIN solo durante operaciones de firma).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use cryptoki::context::{CInitializeArgs, CInitializeFlags, Pkcs11};
use cryptoki::mechanism::Mechanism;
use cryptoki::object::{Attribute, AttributeType, CertificateType, ObjectClass};
use cryptoki::session::UserType;
use cryptoki::slot::Slot;
use cryptoki::error::{Error as CryptokiError, RvError};
use cryptoki::types::AuthPin;
use x509_parser::prelude::*;

use crate::adapters::persistence::Pkcs11PathsDb;
use crate::adapters::pcsc_wake;
use crate::adapters::pkcs11::driver::find_all_pkcs11_modules;
use crate::adapters::pkcs11::error::TokenError;
use crate::domain::cert_filter::der_is_signing_certificate;
use crate::domain::signing_cert::{
    is_win_my_cert_id, sha1_thumbprint_hex, SigningCertSource, SigningCertSummary, SigningPinUi,
};

/// Una sola exploración PKCS#11 (varias DLL) a la vez; evita conflictos PC/SC entre controladores.
fn pkcs11_scan_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Lista de slots donde PKCS#11 considera que hay token insertado.
///
/// Algunos controladores devuelven una lista vacía en `get_slots_with_token()` pero sí marcan
/// `CKF_TOKEN_PRESENT` en `C_GetSlotInfo`; repetimos la misma lógica que en sesión RW.
fn slots_with_token_effective(pkcs11: &Pkcs11) -> Result<Vec<Slot>, TokenError> {
    let primary = pkcs11.get_slots_with_token()?;
    if !primary.is_empty() {
        return Ok(primary);
    }
    let all = pkcs11.get_all_slots()?;
    let mut out = Vec::new();
    for slot in all {
        if pkcs11
            .get_slot_info(slot)
            .map(|i| i.token_present())
            .unwrap_or(false)
        {
            out.push(slot);
        }
    }
    Ok(out)
}

fn pick_slot_from_slice(slots: &[Slot]) -> Result<Slot, TokenError> {
    if slots.is_empty() {
        return Err(TokenError::NoSlot);
    }
    let idx = std::env::var("NEXOSIGN_PKCS11_SLOT")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0);
    slots.get(idx).copied().ok_or(TokenError::SlotIndex)
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn hex_to_bytes(hex: &str) -> Result<Vec<u8>, TokenError> {
    hex::decode(hex.trim()).map_err(|_| TokenError::BadCertId)
}

fn cert_der_and_id_for_hex(
    session: &cryptoki::session::Session,
    cert_id_hex: &str,
) -> Result<(Vec<u8>, Vec<u8>), TokenError> {
    let desired_id = hex_to_bytes(cert_id_hex)?;
    let search_cert = vec![
        Attribute::Class(ObjectClass::CERTIFICATE),
        Attribute::CertificateType(CertificateType::X_509),
        Attribute::Id(desired_id.clone()),
    ];
    let handles = session.find_objects(&search_cert)?;
    if let Some(h) = handles.first() {
        let attrs = session.get_attributes(*h, &[AttributeType::Value, AttributeType::Id])?;
        let mut der: Option<Vec<u8>> = None;
        let mut id_bytes = desired_id.clone();
        for a in attrs {
            match a {
                Attribute::Value(v) => der = Some(v),
                Attribute::Id(id) => id_bytes = id,
                _ => {}
            }
        }
        if let Some(der) = der {
            return Ok((der, id_bytes));
        }
    }

    // Fallback: mismo criterio que `list_signing_certificates` (id derivado del DER si CKA_ID falta).
    let search = vec![
        Attribute::Class(ObjectClass::CERTIFICATE),
        Attribute::CertificateType(CertificateType::X_509),
    ];
    let handles = session.find_objects(&search)?;
    for h in handles {
        let attrs = session.get_attributes(
            h,
            &[
                AttributeType::Value,
                AttributeType::Label,
                AttributeType::Id,
            ],
        )?;
        let mut der: Option<Vec<u8>> = None;
        let mut id_bytes: Option<Vec<u8>> = None;
        for a in attrs {
            match a {
                Attribute::Value(v) => der = Some(v),
                Attribute::Id(id) => id_bytes = Some(id),
                _ => {}
            }
        }
        let Some(der) = der else {
            continue;
        };
        if !der_is_signing_certificate(&der) {
            continue;
        }
        let id_hex = match id_bytes {
            Some(ref id) if !id.is_empty() => bytes_to_hex(id),
            _ => bytes_to_hex(&der[..der.len().min(32)]),
        };
        if id_hex.eq_ignore_ascii_case(cert_id_hex.trim()) {
            let raw_id = id_bytes.unwrap_or_else(|| der[..der.len().min(32)].to_vec());
            return Ok((der, raw_id));
        }
    }

    Err(TokenError::BadCertId)
}

fn subject_dn_from_der(der: &[u8]) -> String {
    let Ok((_, cert)) = X509Certificate::from_der(der) else {
        return String::new();
    };
    cert.subject().to_string()
}

/// Cuenta certificados X.509 en sesión: (total DER, con KeyUsage de firma).
fn count_x509_in_session(session: &cryptoki::session::Session) -> Result<(usize, usize), TokenError> {
    let search = vec![
        Attribute::Class(ObjectClass::CERTIFICATE),
        Attribute::CertificateType(CertificateType::X_509),
    ];
    let handles = session.find_objects(&search)?;
    let raw = handles.len();
    let mut signing = 0usize;
    for h in handles {
        let attrs = session.get_attributes(h, &[AttributeType::Value])?;
        let mut der: Option<Vec<u8>> = None;
        for a in attrs {
            if let Attribute::Value(v) = a {
                der = Some(v);
            }
        }
        let Some(der) = der else {
            continue;
        };
        if der_is_signing_certificate(&der) {
            signing += 1;
        }
    }
    Ok((raw, signing))
}

/// Enumera certificados de firma en una sesión RW ya abierta.
fn collect_signing_certs_from_session(
    session: &mut cryptoki::session::Session,
) -> Result<Vec<SigningCertSummary>, TokenError> {
    let search = vec![
        Attribute::Class(ObjectClass::CERTIFICATE),
        Attribute::CertificateType(CertificateType::X_509),
    ];
    let handles = session.find_objects(&search)?;

    let mut out = Vec::new();
    for h in handles {
        let attrs = session.get_attributes(
            h,
            &[
                AttributeType::Value,
                AttributeType::Label,
                AttributeType::Id,
            ],
        )?;
        let mut der: Option<Vec<u8>> = None;
        let mut label = String::new();
        let mut id_bytes: Option<Vec<u8>> = None;
        for a in attrs {
            match a {
                Attribute::Value(v) => der = Some(v),
                Attribute::Label(l) => label = String::from_utf8_lossy(&l).into_owned(),
                Attribute::Id(id) => id_bytes = Some(id),
                _ => {}
            }
        }
        let Some(der) = der else {
            continue;
        };
        if !der_is_signing_certificate(&der) {
            continue;
        }
        let id_hex = match id_bytes {
            Some(ref id) if !id.is_empty() => bytes_to_hex(id),
            _ => bytes_to_hex(&der[..der.len().min(32)]),
        };
        out.push(SigningCertSummary {
            id_hex,
            label,
            subject_dn: subject_dn_from_der(&der),
            source: SigningCertSource::Pkcs11,
            pin_ui: SigningPinUi::RequiredInApp,
            cert_thumbprint_sha1_hex: sha1_thumbprint_hex(&der),
            win_my_key_binding: None,
        });
    }

    Ok(out)
}

/// Fusiona certificados deduplicando por `id_hex` (misma lógica entre slots y entre DLL).
fn merge_signing_cert_summaries(
    merged: &mut Vec<SigningCertSummary>,
    batch: Vec<SigningCertSummary>,
    seen: &mut HashSet<String>,
) {
    for c in batch {
        if seen.insert(c.id_hex.clone()) {
            merged.push(c);
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Pkcs11ProbeSlotListing {
    pub slot_id: u64,
    pub token_label: Option<String>,
    pub raw_x509_count: usize,
    pub signing_after_filter_count: usize,
    pub session_error: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Pkcs11ProbeModuleListing {
    pub path: String,
    pub slots_with_token: usize,
    pub slots: Vec<Pkcs11ProbeSlotListing>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Pkcs11ProbeCertificateListing {
    pub modules: Vec<Pkcs11ProbeModuleListing>,
}

struct PathScanOutcome {
    certs: Vec<SigningCertSummary>,
    slot_probes: Vec<Pkcs11ProbeSlotListing>,
    slots_with_token: usize,
    error: Option<String>,
}

const PKCS11_SCAN_TIMEOUT: Duration = Duration::from_secs(12);
/// En listado rutinario no recorrer todos los `.dll` del SO (evita bloqueos y minutos de espera).
const MAX_MODULES_ROUTINE_LIST: usize = 2;

/// Explora un módulo PKCS#11 (todos los slots); opcionalmente hace `C_Login` antes de listar.
fn scan_pkcs11_path(path: &Path, pin: Option<&str>) -> PathScanOutcome {
    let mut outcome = PathScanOutcome {
        certs: Vec::new(),
        slot_probes: Vec::new(),
        slots_with_token: 0,
        error: None,
    };

    let pkcs11 = match Pkcs11::new(path) {
        Ok(p) => p,
        Err(e) => {
            outcome.error = Some(format!("Cargar módulo: {e}"));
            return outcome;
        }
    };
    if let Err(e) = pkcs11.initialize(CInitializeArgs::new(CInitializeFlags::OS_LOCKING_OK)) {
        outcome.error = Some(format!("C_Initialize: {e}"));
        return outcome;
    }

    let slots = match slots_with_token_effective(&pkcs11) {
        Ok(s) => s,
        Err(e) => {
            outcome.error = Some(format!("Slots: {e}"));
            let _ = pkcs11.finalize();
            return outcome;
        }
    };
    outcome.slots_with_token = slots.len();

    let mut seen = HashSet::new();
    let needs_rw = pin.is_some();
    let auth_pin = pin.map(|p| AuthPin::new(Box::from(p)));

    for slot in slots {
        let slot_id = slot.id();
        let token_label = pkcs11.get_token_info(slot).ok().map(|t| t.label().to_string());

        let mut session = match open_session_for_slot_with_retry(&pkcs11, slot, needs_rw) {
            Ok(s) => s,
            Err(err) => {
                outcome.slot_probes.push(Pkcs11ProbeSlotListing {
                    slot_id,
                    token_label,
                    raw_x509_count: 0,
                    signing_after_filter_count: 0,
                    session_error: Some(err),
                });
                continue;
            }
        };

        let mut session_error = None;
        if let Some(ref auth) = auth_pin {
            match session.login(UserType::User, Some(auth)) {
                Ok(()) => {}
                Err(CryptokiError::Pkcs11(RvError::UserAlreadyLoggedIn, _)) => {}
                Err(e) => {
                    session_error = Some(format!("C_Login: {e}"));
                }
            }
        }

        let (raw, signing) = count_x509_in_session(&session).unwrap_or((0, 0));
        if session_error.is_none() {
            if let Ok(certs) = collect_signing_certs_from_session(&mut session) {
                merge_signing_cert_summaries(&mut outcome.certs, certs, &mut seen);
            }
        }
        if auth_pin.is_some() {
            let _ = session.logout();
        }

        outcome.slot_probes.push(Pkcs11ProbeSlotListing {
            slot_id,
            token_label,
            raw_x509_count: raw,
            signing_after_filter_count: signing,
            session_error,
        });
    }

    let _ = pkcs11.finalize();
    outcome
}

fn scan_pkcs11_path_timed(path: &Path, pin: Option<&str>, timeout: Duration) -> PathScanOutcome {
    let path = path.to_path_buf();
    let path_label = path.display().to_string();
    let pin_owned = pin.map(|s| s.to_string());
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let pin_ref = pin_owned.as_deref();
        let _ = tx.send(scan_pkcs11_path(&path, pin_ref));
    });
    match rx.recv_timeout(timeout) {
        Ok(outcome) => outcome,
        Err(_) => PathScanOutcome {
            certs: Vec::new(),
            slot_probes: Vec::new(),
            slots_with_token: 0,
            error: Some(format!(
                "Tiempo de espera agotado ({:.0}s) al usar {path_label}",
                timeout.as_secs_f64(),
            )),
        },
    }
}


pub struct Pkcs11TokenManager {
    inner: Mutex<Inner>,
}

struct Inner {
    pkcs11: Option<Pkcs11>,
    active_module_path: Option<PathBuf>,
    session: Option<cryptoki::session::Session>,
    logged_in: bool,
    /// PIN almacenado para re-autenticación `CKU_CONTEXT_SPECIFIC` (`CKA_ALWAYS_AUTHENTICATE`).
    pin: Option<String>,
    /// Misma base SQLite que orígenes permitidos (`OriginDbPath`).
    app_database_path: Arc<Mutex<Option<PathBuf>>>,
}

/// Cierra sesión y hace `C_Finalize` del módulo PKCS#11 en memoria (libera el driver para otro escaneo).
fn reset_pkcs11_inner_state(inner: &mut Inner) {
    if let Some(sess) = inner.session.take() {
        let _ = sess.logout();
    }
    if let Some(pkcs11) = inner.pkcs11.take() {
        let _ = pkcs11.finalize();
    }
    inner.active_module_path = None;
    inner.logged_in = false;
    inner.pin = None;
}

fn session_error_token_not_recognized(err: &str) -> bool {
    let e = err.to_lowercase();
    e.contains("not recognize") || e.contains("token not recognized")
}

/// Abre sesión en el slot: RO para listar sin PIN; RW si hace falta login o RO falla.
fn open_session_for_slot(
    pkcs11: &Pkcs11,
    slot: Slot,
    needs_rw: bool,
) -> Result<cryptoki::session::Session, String> {
    if needs_rw {
        return pkcs11
            .open_rw_session(slot)
            .map_err(|e| format!("Sesión RW: {e}"));
    }
    match pkcs11.open_ro_session(slot) {
        Ok(s) => Ok(s),
        Err(e_ro) => pkcs11
            .open_rw_session(slot)
            .map_err(|e_rw| format!("Sesión RO: {e_ro}; RW: {e_rw}")),
    }
}

/// Reintenta apertura de sesión si el driver aún no reconoce el token (sin PC/SC: evita bloqueo con PKCS#11 activo).
fn open_session_for_slot_with_retry(
    pkcs11: &Pkcs11,
    slot: Slot,
    needs_rw: bool,
) -> Result<cryptoki::session::Session, String> {
    let mut last_err = String::new();
    for attempt in 0..2u32 {
        if attempt > 0 {
            std::thread::sleep(Duration::from_millis(350));
        }
        match open_session_for_slot(pkcs11, slot, needs_rw) {
            Ok(s) => return Ok(s),
            Err(e) => {
                let retry = session_error_token_not_recognized(&e);
                last_err = e;
                if !retry {
                    break;
                }
                tracing::debug!(attempt, "PKCS#11: reintento apertura de sesión (token no reconocido aún)");
            }
        }
    }
    Err(last_err)
}

fn merge_paths_scan(
    paths: &[PathBuf],
    pin: Option<&str>,
) -> (Vec<SigningCertSummary>, bool) {
    let mut merged = Vec::new();
    let mut seen = HashSet::new();
    let mut saw_token_not_recognized = false;

    for (i, path) in paths.iter().enumerate() {
        if i >= MAX_MODULES_ROUTINE_LIST {
            break;
        }
        let scan = scan_pkcs11_path_timed(path, pin, PKCS11_SCAN_TIMEOUT);
        if let Some(err) = &scan.error {
            tracing::info!(path = %path.display(), error = %err, "PKCS#11 listado sin certificados");
        }
        for slot in &scan.slot_probes {
            if let Some(e) = &slot.session_error {
                if session_error_token_not_recognized(e) {
                    saw_token_not_recognized = true;
                }
            }
        }
        merge_signing_cert_summaries(&mut merged, scan.certs, &mut seen);

        let found_chip = merged.iter().any(|c| c.source == SigningCertSource::Pkcs11);
        if found_chip {
            break;
        }
    }
    (merged, saw_token_not_recognized)
}

impl Pkcs11TokenManager {
    /// `app_database_path`: ruta al `.sqlite` de la app; `None` hasta que Tauri ejecuta `setup` (tests: `None`).
    pub fn new(app_database_path: Arc<Mutex<Option<PathBuf>>>) -> Self {
        Self {
            inner: Mutex::new(Inner {
                pkcs11: None,
                active_module_path: None,
                session: None,
                logged_in: false,
                pin: None,
                app_database_path,
            }),
        }
    }

    /// Cierra sesión PKCS#11 en memoria para volver a cargar el módulo tras cambiar rutas en BD.
    pub fn reset_pkcs11_driver_state(&self) -> Result<(), TokenError> {
        let mut inner = self.lock_inner()?;
        reset_pkcs11_inner_state(&mut inner);
        drop(inner);
        pcsc_wake::wake_smart_card_readers();
        Ok(())
    }

    /// Cierra solo la sesión PKCS#11 (no descarga el módulo); el siguiente login la reabre.
    ///
    /// Se usa entre hilos: algunos drivers asocian el estado de `C_Login` al hilo OS que lo invocó,
    /// así que tras validar PIN en el hilo HTTP soltamos la sesión para que el worker abra una nueva
    /// en su propio hilo y haga `C_Login` + `C_Sign` allí.
    pub fn release_session(&self) -> Result<(), TokenError> {
        let mut inner = self.lock_inner()?;
        if let Some(sess) = inner.session.take() {
            if inner.logged_in {
                let _ = sess.logout();
            }
        }
        inner.logged_in = false;
        Ok(())
    }

    fn lock_inner(&self) -> Result<std::sync::MutexGuard<'_, Inner>, TokenError> {
        self.inner.lock().map_err(|_| TokenError::MutexPoisoned)
    }

    pub fn probe_module_path(&self) -> Result<String, TokenError> {
        let mut inner = self.lock_inner()?;
        ensure_pkcs11(&mut inner)?;
        let p = inner.active_module_path.as_ref().unwrap();
        Ok(p.display().to_string())
    }

    pub fn slot_count_with_token(&self) -> Result<usize, TokenError> {
        let _scan = pkcs11_scan_lock();
        let mut inner = self.lock_inner()?;
        ensure_pkcs11(&mut inner)?;
        let pkcs11 = inner.pkcs11.as_ref().expect("initialized");
        let n = slots_with_token_effective(pkcs11)?.len();
        reset_pkcs11_inner_state(&mut inner);
        Ok(n)
    }

    /// Lectores y tokens según el mismo PKCS#11 que usa la app (para depuración en UI).
    pub fn diagnose_slots(&self) -> Result<Pkcs11Diagnostics, TokenError> {
        let _scan = pkcs11_scan_lock();
        let mut inner = self.lock_inner()?;
        ensure_pkcs11(&mut inner)?;
        let pkcs11 = inner.pkcs11.as_ref().expect("initialized");
        let module_path = inner.active_module_path.as_ref().unwrap().display().to_string();

        let count_slot_list_true = pkcs11.get_slots_with_token()?.len();
        let effective = slots_with_token_effective(pkcs11)?;
        let count_effective = effective.len();

        let all = pkcs11.get_all_slots()?;
        let mut slots = Vec::with_capacity(all.len());
        for slot in all {
            let slot_id = slot.id();
            let Ok(si) = pkcs11.get_slot_info(slot) else {
                continue;
            };
            let token_label = pkcs11.get_token_info(slot).ok().map(|t| t.label().to_string());
            slots.push(Pkcs11SlotDetail {
                slot_id,
                slot_description: si.slot_description().to_string(),
                manufacturer_id: si.manufacturer_id().to_string(),
                token_present_in_slot_info: si.token_present(),
                token_label,
            });
        }

        let diag = Pkcs11Diagnostics {
            module_path,
            count_pkcs11_get_slot_list_true: count_slot_list_true,
            count_effective_for_nexosign: count_effective,
            slots,
        };
        reset_pkcs11_inner_state(&mut inner);
        Ok(diag)
    }

    pub fn list_signing_certificates(&self) -> Result<Vec<SigningCertSummary>, TokenError> {
        let _scan = pkcs11_scan_lock();
        pcsc_wake::wake_smart_card_readers();
        let paths = {
            let mut inner = self.lock_inner()?;
            if inner.logged_in {
                if let Some(ref mut sess) = inner.session {
                    return collect_signing_certs_from_session(sess);
                }
            }
            reset_pkcs11_inner_state(&mut inner);
            merged_pkcs11_module_paths(&inner)?
        };

        let (mut merged, saw_token_not_recognized) = merge_paths_scan(&paths, None);
        if merged.is_empty() && saw_token_not_recognized {
            if let Some(first) = paths.first() {
                tracing::info!("PKCS#11: segundo intento en controlador preferido tras PC/SC wake");
                pcsc_wake::wake_smart_card_readers();
                std::thread::sleep(Duration::from_millis(400));
                let scan = scan_pkcs11_path_timed(first, None, PKCS11_SCAN_TIMEOUT);
                let mut seen = HashSet::new();
                merge_signing_cert_summaries(&mut merged, scan.certs, &mut seen);
            }
        }
        Ok(merged)
    }

    /// Lista certificados PKCS#11 en todos los slots (con PIN), sin dejar sesión abierta en el manager.
    pub fn list_pkcs11_signing_with_pin(&self, pin: String) -> Result<Vec<SigningCertSummary>, TokenError> {
        if pin.is_empty() {
            return Err(TokenError::EmptyPin);
        }
        let _scan = pkcs11_scan_lock();
        pcsc_wake::wake_smart_card_readers();
        let paths = {
            let mut inner = self.lock_inner()?;
            reset_pkcs11_inner_state(&mut inner);
            merged_pkcs11_module_paths(&inner)?
        };

        let (merged, _) = merge_paths_scan(&paths, Some(&pin));
        Ok(merged)
    }

    /// Diagnóstico por DLL/slot: objetos X.509 en chip vs filtro KeyUsage de firma.
    pub fn probe_certificate_listing(&self) -> Result<Pkcs11ProbeCertificateListing, TokenError> {
        let _scan = pkcs11_scan_lock();
        pcsc_wake::wake_smart_card_readers();
        let paths = {
            let mut inner = self.lock_inner()?;
            reset_pkcs11_inner_state(&mut inner);
            merged_pkcs11_module_paths(&inner)?
        };

        let mut modules = Vec::with_capacity(paths.len());
        for path in paths.iter().take(4) {
            let scan = scan_pkcs11_path_timed(path, None, PKCS11_SCAN_TIMEOUT);
            for slot in &scan.slot_probes {
                tracing::info!(
                    path = %path.display(),
                    slot_id = slot.slot_id,
                    raw_x509 = slot.raw_x509_count,
                    signing = slot.signing_after_filter_count,
                    session_error = ?slot.session_error,
                    "PKCS#11 probe slot"
                );
            }
            if let Some(err) = &scan.error {
                tracing::info!(path = %path.display(), error = %err, "PKCS#11 probe módulo");
            }
            modules.push(Pkcs11ProbeModuleListing {
                path: path.display().to_string(),
                slots_with_token: scan.slots_with_token,
                slots: scan.slot_probes,
                error: scan.error,
            });
        }
        Ok(Pkcs11ProbeCertificateListing { modules })
    }

    pub fn certificate_der_by_id_hex(&self, cert_id_hex: &str) -> Result<Vec<u8>, TokenError> {
        let mut inner = self.lock_inner()?;
        ensure_pkcs11_and_session_for_cert(&mut inner, cert_id_hex)?;
        let session = inner.session.as_mut().expect("session");
        let (der, _) = cert_der_and_id_for_hex(session, cert_id_hex)?;
        Ok(der)
    }

    /// Firma RSA SHA-256 PKCS#1 v1.5 (`CKM_SHA256_RSA_PKCS`). Requiere PIN (`login` / `login_for_certificate`).
    ///
    /// Realiza `C_Login(CKU_CONTEXT_SPECIFIC)` antes de cada `C_Sign` para soportar
    /// claves con `CKA_ALWAYS_AUTHENTICATE = true` (típico en DNIe/eID).
    pub fn rsa_sha256_pkcs1_sign(
        &self,
        cert_id_hex: &str,
        data: &[u8],
    ) -> Result<Vec<u8>, TokenError> {
        let mut inner = self.lock_inner()?;
        ensure_pkcs11_and_session_for_cert(&mut inner, cert_id_hex)?;
        if !inner.logged_in {
            return Err(TokenError::NotLoggedIn);
        }

        // Extraemos el PIN antes de pedir la sesión mutably para evitar errores del borrow checker (E0502).
        let pin_for_auth = inner.pin.clone();

        let session = inner.session.as_mut().expect("session");
        let (_, id_bytes) = cert_der_and_id_for_hex(session, cert_id_hex)?;
        let search_key = vec![
            Attribute::Class(ObjectClass::PRIVATE_KEY),
            Attribute::Id(id_bytes),
        ];
        let handles = session.find_objects(&search_key)?;
        let key = handles.into_iter().next().ok_or(TokenError::NoPrivateKey)?;

        // CKA_ALWAYS_AUTHENTICATE: re-autenticación por operación antes de firmar.
        // El estándar PKCS#11 exige: C_SignInit -> C_Login(ContextSpecific) -> C_Sign.
        
        // 1. Inicializar la operación de firma
        session.sign_init(&Mechanism::Sha256RsaPkcs, key)?;

        // 2. Realizar re-autenticación si tenemos el PIN
        if let Some(pin_str) = pin_for_auth {
            let auth = AuthPin::new(pin_str.into());
            match session.login(UserType::ContextSpecific, Some(&auth)) {
                Ok(()) => {
                    eprintln!("[NexoSign DIAG] ContextSpecific login OK ✓");
                }
                // Algunos drivers no soportan ContextSpecific o ya están logueados; ignorar y probar firma.
                Err(CryptokiError::Pkcs11(RvError::UserAlreadyLoggedIn, _)) => {}
                Err(CryptokiError::Pkcs11(RvError::OperationNotInitialized, _)) => {
                    eprintln!("[NexoSign DIAG] ContextSpecific login: OperationNotInitialized (posiblemente el driver no sigue el estándar)");
                }
                Err(e) => {
                    eprintln!("[NexoSign DIAG] ContextSpecific login error: {e}");
                }
            }
        }

        // 3. Ejecutar la firma (usamos update + final para evitar el re-init interno de cryptoki)
        session.sign_update(data)?;
        Ok(session.sign_final()?)
    }

    pub fn login(&self, pin: String) -> Result<(), TokenError> {
        if pin.is_empty() {
            return Err(TokenError::EmptyPin);
        }
        let mut inner = self.lock_inner()?;
        ensure_session_rw(&mut inner)?;
        let pin_saved = pin.clone();
        let auth = AuthPin::new(pin.into());
        let login_result = {
            let session = inner.session.as_mut().expect("session");
            session.login(UserType::User, Some(&auth))
        };
        match login_result {
            Ok(()) => {
                inner.logged_in = true;
                inner.pin = Some(pin_saved);
                Ok(())
            }
            Err(CryptokiError::Pkcs11(RvError::UserAlreadyLoggedIn, _)) => {
                inner.logged_in = true;
                inner.pin = Some(pin_saved);
                Ok(())
            }
            Err(e) => {
                reset_pkcs11_inner_state(&mut inner);
                Err(e.into())
            }
        }
    }

    /// Igual que [`login`], pero abre sesión en el **módulo PKCS#11 que contiene** `cert_id_hex`
    /// (necesario si hay varios middlewares y el certificado no está en el primero de la lista).
    pub fn login_for_certificate(&self, pin: String, cert_id_hex: &str) -> Result<(), TokenError> {
        if pin.is_empty() {
            return Err(TokenError::EmptyPin);
        }
        let mut inner = self.lock_inner()?;
        ensure_pkcs11_and_session_for_cert(&mut inner, cert_id_hex)?;
        let pin_saved = pin.clone();
        let auth = AuthPin::new(pin.into());
        let login_result = {
            let session = inner.session.as_mut().expect("session");
            session.login(UserType::User, Some(&auth))
        };
        match login_result {
            Ok(()) => {
                inner.logged_in = true;
                inner.pin = Some(pin_saved);
                Ok(())
            }
            Err(CryptokiError::Pkcs11(RvError::UserAlreadyLoggedIn, _)) => {
                inner.logged_in = true;
                inner.pin = Some(pin_saved);
                Ok(())
            }
            Err(e) => {
                reset_pkcs11_inner_state(&mut inner);
                Err(e.into())
            }
        }
    }

    pub fn logout(&self) -> Result<(), TokenError> {
        let mut inner = self.lock_inner()?;
        if let Some(ref s) = inner.session {
            if inner.logged_in {
                let _ = s.logout();
            }
        }
        inner.logged_in = false;
        inner.pin = None;
        Ok(())
    }

    pub fn session_status(&self) -> SessionStatusDto {
        let logged_in = self
            .lock_inner()
            .map(|g| g.logged_in)
            .unwrap_or(false);
        SessionStatusDto {
            logged_in,
            idle_timeout_secs: 0,
            seconds_until_auto_logout: None,
        }
    }

    /// Verifica que el PIN sea correcto sin dejar sesión abierta.
    ///
    /// Abre sesión → login → logout → reset, de forma que el worker de firma
    /// pueda hacer `C_Initialize` + `C_Login` en su propio hilo sin conflictos.
    pub fn verify_pin(&self, pin: String, cert_id_hex: &str) -> Result<(), TokenError> {
        if pin.is_empty() {
            return Err(TokenError::EmptyPin);
        }
        let mut inner = self.lock_inner()?;
        ensure_pkcs11_and_session_for_cert(&mut inner, cert_id_hex)?;
        let auth = AuthPin::new(pin.into());
        let login_result = {
            let session = inner.session.as_mut().expect("session");
            session.login(UserType::User, Some(&auth))
        };
        match login_result {
            Ok(()) | Err(CryptokiError::Pkcs11(RvError::UserAlreadyLoggedIn, _)) => {
                // PIN correcto; limpiar sesión para que el worker arranque limpio.
                reset_pkcs11_inner_state(&mut inner);
                Ok(())
            }
            Err(e) => {
                reset_pkcs11_inner_state(&mut inner);
                match &e {
                    CryptokiError::Pkcs11(RvError::PinIncorrect, _) => Err(TokenError::PinIncorrect),
                    CryptokiError::Pkcs11(RvError::PinLocked, _) => Err(TokenError::PinLocked),
                    _ => Err(e.into()),
                }
            }
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionStatusDto {
    pub logged_in: bool,
    /// Siempre `0` (ya no hay cierre por inactividad).
    pub idle_timeout_secs: u64,
    pub seconds_until_auto_logout: Option<u64>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Pkcs11SlotDetail {
    pub slot_id: u64,
    pub slot_description: String,
    pub manufacturer_id: String,
    pub token_present_in_slot_info: bool,
    pub token_label: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Pkcs11Diagnostics {
    pub module_path: String,
    /// Resultado estricto de `C_GetSlotList(tokenPresent=CK_TRUE)`.
    pub count_pkcs11_get_slot_list_true: usize,
    /// Slots que NexoSign usará (incluye fallback por `CK_SLOT_INFO`).
    pub count_effective_for_nexosign: usize,
    pub slots: Vec<Pkcs11SlotDetail>,
}

fn pkcs11_paths_from_db(inner: &Inner) -> Option<Vec<PathBuf>> {
    let guard = inner.app_database_path.lock().ok()?;
    let db_file = guard.as_ref()?.clone();
    let db = Pkcs11PathsDb::open(&db_file).ok()?;
    let paths = db.list_paths_ordered().ok()?;
    Some(paths.into_iter().map(PathBuf::from).collect())
}

fn preferred_module_path_from_db(inner: &Inner) -> Option<PathBuf> {
    let guard = inner.app_database_path.lock().ok()?;
    let db_file = guard.as_ref()?.clone();
    let db = Pkcs11PathsDb::open(&db_file).ok()?;
    let s = db.get_preferred_module_path().ok()??;
    let p = PathBuf::from(s);
    if p.is_file() {
        Some(p)
    } else {
        None
    }
}

/// Orden efectivo de candidatos: si el usuario eligió un middleware preferido en BD, se prueba primero.
fn merged_pkcs11_module_paths(inner: &Inner) -> Result<Vec<PathBuf>, TokenError> {
    let db_paths = pkcs11_paths_from_db(inner);
    let mut paths = find_all_pkcs11_modules(db_paths.as_deref())?;
    if let Some(pref) = preferred_module_path_from_db(inner) {
        let key = pref.to_string_lossy().to_lowercase();
        paths.retain(|p| p.to_string_lossy().to_lowercase() != key);
        paths.insert(0, pref);
    }
    Ok(paths)
}

/// Selecciona el módulo PKCS#11 y slot donde existe `cert_id_hex` (recorre rutas BD + incorporadas).
fn ensure_pkcs11_and_session_for_cert(
    inner: &mut Inner,
    cert_id_hex: &str,
) -> Result<(), TokenError> {
    if is_win_my_cert_id(cert_id_hex) {
        return Err(TokenError::WinMyNotPkcs11);
    }
    let cert_id_hex = cert_id_hex.trim();
    if let Some(ref mut sess) = inner.session {
        if cert_der_and_id_for_hex(sess, cert_id_hex).is_ok() {
            return Ok(());
        }
        inner.session = None;
        inner.logged_in = false;
    }

    if let Some(ref pkcs11) = inner.pkcs11 {
        let slots = slots_with_token_effective(pkcs11)?;
        for slot in slots {
            let sess = pkcs11.open_rw_session(slot)?;
            if cert_der_and_id_for_hex(&sess, cert_id_hex).is_ok() {
                inner.session = Some(sess);
                return Ok(());
            }
        }
        inner.pkcs11 = None;
        inner.active_module_path = None;
    }

    let paths = merged_pkcs11_module_paths(inner)?;

    for path in paths {
        let pkcs11 = match Pkcs11::new(&path) {
            Ok(p) => p,
            Err(_) => continue,
        };
        if pkcs11
            .initialize(CInitializeArgs::new(CInitializeFlags::OS_LOCKING_OK))
            .is_err()
        {
            continue;
        }
        let slots = match slots_with_token_effective(&pkcs11) {
            Ok(s) if !s.is_empty() => s,
            _ => continue,
        };
        for slot in slots {
            let sess = match pkcs11.open_rw_session(slot) {
                Ok(s) => s,
                Err(_) => continue,
            };
            if cert_der_and_id_for_hex(&sess, cert_id_hex).is_ok() {
                inner.pkcs11 = Some(pkcs11);
                inner.active_module_path = Some(path.clone());
                inner.session = Some(sess);
                return Ok(());
            }
        }
    }

    Err(TokenError::BadCertId)
}

fn ensure_pkcs11(inner: &mut Inner) -> Result<(), TokenError> {
    if inner.pkcs11.is_some() {
        return Ok(());
    }

    let paths = merged_pkcs11_module_paths(inner)?;

    for path in paths.iter().take(MAX_MODULES_ROUTINE_LIST) {
        if let Ok(pkcs11) = Pkcs11::new(path) {
            if pkcs11.initialize(CInitializeArgs::new(CInitializeFlags::OS_LOCKING_OK)).is_ok() {
                if let Ok(slots) = slots_with_token_effective(&pkcs11) {
                    if !slots.is_empty() {
                        inner.pkcs11 = Some(pkcs11);
                        inner.active_module_path = Some(path.clone());
                        return Ok(());
                    }
                }
                let _ = pkcs11.finalize();
            }
        }
    }

    // Si ninguno tiene token, nos quedamos con el primero que funcione (no crash) para que la UI muestre 0 slots
    for path in paths.iter().take(1) {
        if let Ok(pkcs11) = Pkcs11::new(path) {
            if pkcs11.initialize(CInitializeArgs::new(CInitializeFlags::OS_LOCKING_OK)).is_ok() {
                inner.pkcs11 = Some(pkcs11);
                inner.active_module_path = Some(path.clone());
                return Ok(());
            }
        }
    }

    Err(TokenError::Driver(crate::adapters::pkcs11::driver::DriverPathError::NotFound))
}

fn pick_slot(pkcs11: &Pkcs11) -> Result<Slot, TokenError> {
    let slots = slots_with_token_effective(pkcs11)?;
    pick_slot_from_slice(&slots)
}

fn ensure_session_rw(inner: &mut Inner) -> Result<(), TokenError> {
    ensure_pkcs11(inner)?;
    if inner.session.is_some() {
        return Ok(());
    }
    let pkcs11 = inner.pkcs11.as_ref().expect("pkcs11");
    let slot = pick_slot(pkcs11)?;
    let sess = pkcs11.open_rw_session(slot)?;
    inner.session = Some(sess);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::signing_cert::{SigningCertSource, SigningPinUi};

    fn sample_cert(id: &str) -> SigningCertSummary {
        SigningCertSummary {
            id_hex: id.to_string(),
            label: id.to_string(),
            subject_dn: String::new(),
            source: SigningCertSource::Pkcs11,
            pin_ui: SigningPinUi::RequiredInApp,
            cert_thumbprint_sha1_hex: String::new(),
            win_my_key_binding: None,
        }
    }

    #[test]
    fn merge_signing_cert_summaries_dedupes_by_id_hex() {
        let mut merged = vec![sample_cert("aa")];
        let mut seen = HashSet::from(["aa".to_string()]);
        merge_signing_cert_summaries(
            &mut merged,
            vec![sample_cert("aa"), sample_cert("bb")],
            &mut seen,
        );
        assert_eq!(merged.len(), 2);
        assert!(merged.iter().any(|c| c.id_hex == "bb"));
    }
}

