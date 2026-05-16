//! PKCS#11 token manager (initialize, list signing certs, PIN solo durante operaciones de firma).

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use cryptoki::context::{CInitializeArgs, CInitializeFlags, Pkcs11};
use cryptoki::mechanism::Mechanism;
use cryptoki::object::{Attribute, AttributeType, CertificateType, ObjectClass};
use cryptoki::session::UserType;
use cryptoki::slot::Slot;
use cryptoki::error::{Error as CryptokiError, RvError};
use cryptoki::types::AuthPin;
use x509_parser::prelude::*;

use crate::adapters::persistence::Pkcs11PathsDb;
use crate::adapters::pkcs11::driver::find_all_pkcs11_modules;
use crate::adapters::pkcs11::error::TokenError;
use crate::domain::cert_filter::der_is_signing_certificate;
use crate::domain::signing_cert::{is_win_my_cert_id, SigningCertSource, SigningCertSummary, SigningPinUi};

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
        });
    }

    Ok(out)
}

/// Lista certificados de firma visibles en un único fichero PKCS#11 (cierra sesión al terminar).
fn list_signing_certs_for_path(path: &std::path::Path) -> Result<Vec<SigningCertSummary>, TokenError> {
    let pkcs11 = Pkcs11::new(path)?;
    pkcs11.initialize(CInitializeArgs::new(CInitializeFlags::OS_LOCKING_OK))?;
    let slots = slots_with_token_effective(&pkcs11)?;
    if slots.is_empty() {
        return Ok(vec![]);
    }
    let slot = pick_slot_from_slice(&slots)?;
    let mut session = pkcs11.open_rw_session(slot)?;
    collect_signing_certs_from_session(&mut session)
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

/// Cierra sesión RW y descarga el módulo PKCS#11 en memoria (libera drivers tras PIN incorrecto o sesión colgada).
fn reset_pkcs11_inner_state(inner: &mut Inner) {
    if let Some(sess) = inner.session.take() {
        let _ = sess.logout();
    }
    inner.pkcs11 = None;
    inner.active_module_path = None;
    inner.logged_in = false;
    inner.pin = None;
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
        let mut inner = self.lock_inner()?;
        ensure_pkcs11(&mut inner)?;
        let pkcs11 = inner.pkcs11.as_ref().expect("initialized");
        Ok(slots_with_token_effective(pkcs11)?.len())
    }

    /// Lectores y tokens según el mismo PKCS#11 que usa la app (para depuración en UI).
    pub fn diagnose_slots(&self) -> Result<Pkcs11Diagnostics, TokenError> {
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

        Ok(Pkcs11Diagnostics {
            module_path,
            count_pkcs11_get_slot_list_true: count_slot_list_true,
            count_effective_for_nexosign: count_effective,
            slots,
        })
    }

    pub fn list_signing_certificates(&self) -> Result<Vec<SigningCertSummary>, TokenError> {
        let paths = {
            let mut inner = self.lock_inner()?;
            // Si ya hay login PKCS#11 válido, no abrimos otro handle al middleware (evita conflicto con el driver).
            if inner.logged_in {
                if let Some(ref mut sess) = inner.session {
                    return collect_signing_certs_from_session(sess);
                }
            }
            // Sin login (o estado inconsistente): libera sesión/módulo internos antes de sondear rutas.
            // Tras PIN incorrecto algunos drivers dejan una RW sesión colgada y bloquean nuevas aperturas.
            reset_pkcs11_inner_state(&mut inner);
            merged_pkcs11_module_paths(&inner)?
        };

        let mut merged = Vec::new();
        let mut seen = HashSet::new();
        for path in paths {
            match list_signing_certs_for_path(&path) {
                Ok(certs) => {
                    for c in certs {
                        if seen.insert(c.id_hex.clone()) {
                            merged.push(c);
                        }
                    }
                }
                Err(_) => continue,
            }
        }
        Ok(merged)
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

    for path in &paths {
        if let Ok(pkcs11) = Pkcs11::new(path) {
            if pkcs11.initialize(CInitializeArgs::new(CInitializeFlags::OS_LOCKING_OK)).is_ok() {
                if let Ok(slots) = slots_with_token_effective(&pkcs11) {
                    if !slots.is_empty() {
                        inner.pkcs11 = Some(pkcs11);
                        inner.active_module_path = Some(path.clone());
                        return Ok(());
                    }
                }
            }
        }
    }

    // Si ninguno tiene token, nos quedamos con el primero que funcione (no crash) para que la UI muestre 0 slots
    for path in &paths {
        if let Ok(pkcs11) = Pkcs11::new(path) {
            let _ = pkcs11.initialize(CInitializeArgs::new(CInitializeFlags::OS_LOCKING_OK));
            inner.pkcs11 = Some(pkcs11);
            inner.active_module_path = Some(path.clone());
            return Ok(());
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

