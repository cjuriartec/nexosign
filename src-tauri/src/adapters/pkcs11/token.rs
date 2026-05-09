//! PKCS#11 token manager (initialize, list signing certs, PIN session).

use std::sync::Mutex;
use std::time::{Duration, Instant};

use cryptoki::context::{CInitializeArgs, CInitializeFlags, Pkcs11};
use cryptoki::object::{Attribute, AttributeType, CertificateType, ObjectClass};
use cryptoki::session::UserType;
use cryptoki::slot::Slot;
use cryptoki::types::AuthPin;
use x509_parser::prelude::*;

use crate::adapters::pkcs11::driver::find_all_pkcs11_modules;
use crate::adapters::pkcs11::error::TokenError;
use crate::domain::cert_filter::der_is_signing_certificate;
use crate::domain::signing_cert::SigningCertSummary;

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

fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn subject_dn_from_der(der: &[u8]) -> String {
    let Ok((_, cert)) = X509Certificate::from_der(der) else {
        return String::new();
    };
    cert.subject().to_string()
}

pub struct Pkcs11TokenManager {
    inner: Mutex<Inner>,
}

struct Inner {
    pkcs11: Option<Pkcs11>,
    active_module_path: Option<std::path::PathBuf>,
    session: Option<cryptoki::session::Session>,
    logged_in: bool,
    last_activity: Option<Instant>,
    idle_timeout: Duration,
}

impl Pkcs11TokenManager {
    pub fn new() -> Self {
        let idle_secs = std::env::var("NEXOSIGN_TOKEN_IDLE_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(900_u64);

        Self {
            inner: Mutex::new(Inner {
                pkcs11: None,
                active_module_path: None,
                session: None,
                logged_in: false,
                last_activity: None,
                idle_timeout: Duration::from_secs(idle_secs),
            }),
        }
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
        let mut inner = self.lock_inner()?;
        maybe_idle_logout(&mut inner)?;
        ensure_session_rw(&mut inner)?;
        inner.touch_activity();
        let session = inner.session.as_mut().expect("session");

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
            });
        }

        Ok(out)
    }

    pub fn login(&self, pin: String) -> Result<(), TokenError> {
        if pin.is_empty() {
            return Err(TokenError::EmptyPin);
        }
        let mut inner = self.lock_inner()?;
        maybe_idle_logout(&mut inner)?;
        ensure_session_rw(&mut inner)?;
        {
            let session = inner.session.as_mut().expect("session");
            let auth = AuthPin::new(pin.into());
            session.login(UserType::User, Some(&auth))?;
        }
        inner.logged_in = true;
        inner.touch_activity();
        Ok(())
    }

    pub fn logout(&self) -> Result<(), TokenError> {
        let mut inner = self.lock_inner()?;
        if let Some(ref s) = inner.session {
            if inner.logged_in {
                let _ = s.logout();
            }
        }
        inner.logged_in = false;
        inner.last_activity = None;
        Ok(())
    }

    pub fn session_status(&self) -> SessionStatusDto {
        let mut inner = match self.lock_inner() {
            Ok(g) => g,
            Err(_) => {
                return SessionStatusDto {
                    logged_in: false,
                    idle_timeout_secs: 900,
                    seconds_until_auto_logout: None,
                };
            }
        };
        let _ = maybe_idle_logout(&mut inner);
        let secs_left = match (inner.logged_in, inner.last_activity) {
            (true, Some(t)) => {
                let elapsed = t.elapsed();
                if elapsed >= inner.idle_timeout {
                    Some(0)
                } else {
                    Some((inner.idle_timeout - elapsed).as_secs())
                }
            }
            _ => None,
        };
        SessionStatusDto {
            logged_in: inner.logged_in,
            idle_timeout_secs: inner.idle_timeout.as_secs(),
            seconds_until_auto_logout: secs_left,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionStatusDto {
    pub logged_in: bool,
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

fn ensure_pkcs11(inner: &mut Inner) -> Result<(), TokenError> {
    if inner.pkcs11.is_some() {
        return Ok(());
    }

    let paths = find_all_pkcs11_modules()?;

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
    if slots.is_empty() {
        return Err(TokenError::NoSlot);
    }
    let idx = std::env::var("NEXOSIGN_PKCS11_SLOT")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0);
    slots.get(idx).copied().ok_or(TokenError::SlotIndex)
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

fn maybe_idle_logout(inner: &mut Inner) -> Result<(), TokenError> {
    if !inner.logged_in {
        return Ok(());
    }
    let Some(last) = inner.last_activity else {
        return Ok(());
    };
    if last.elapsed() >= inner.idle_timeout {
        if let Some(ref s) = inner.session {
            let _ = s.logout();
        }
        inner.logged_in = false;
        inner.last_activity = None;
    }
    Ok(())
}

impl Inner {
    fn touch_activity(&mut self) {
        self.last_activity = Some(Instant::now());
    }
}
