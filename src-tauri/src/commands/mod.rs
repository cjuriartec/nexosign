use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use serde::Serialize;
use serde_json::json;
use tauri::{AppHandle, Emitter};
use tokio_util::sync::CancellationToken;

use crate::infrastructure::local_api_listen::{base_url_for_port, LocalApiListenSnapshot, LocalApiRuntime};
use crate::domain::pending_batch_intent::PendingBatchIntent;
use crate::adapters::http::state::PendingBatchIntents;
use crate::adapters::persistence::queue_store;
use crate::adapters::persistence::{AllowedOriginsDb, Pkcs11PathsDb};
use crate::adapters::pkcs11::driver::find_all_pkcs11_modules;
use crate::adapters::pkcs11::token::{
    Pkcs11Diagnostics, Pkcs11ProbeCertificateListing, Pkcs11TokenManager, SessionStatusDto,
};
use crate::domain::allowed_origins::AllowedOrigins;
use crate::domain::signing_cert::SigningCertSummary;
#[cfg(windows)]
use crate::domain::signing_cert::dedupe_signing_certs_prefer_pkcs11;
use crate::infrastructure::origin_db::OriginDbPath;

pub mod batch_queue_history;
pub mod local_api;

/// Estado gestionado por Tauri (`.manage`) compartido con la API local.
type OriginsStore = Arc<RwLock<AllowedOrigins>>;

type Pkcs11Store = Arc<Pkcs11TokenManager>;

/// Mismo `Arc` que [`crate::adapters::http::state::SharedState::batch_cancel`] para cancelar lotes vía IPC.
#[derive(Clone)]
pub struct BatchCancelRegistry(pub Arc<Mutex<HashMap<String, CancellationToken>>>);

/// Mismo `Arc` que [`crate::adapters::http::state::SharedState::batch_signed_outputs`] (descargas HTTP).
#[derive(Clone)]
pub struct BatchSignedOutputsStore(pub Arc<Mutex<HashMap<String, Vec<PathBuf>>>>);


/// PKCS#11 bloquea el hilo (lector/tariffa); no debe ejecutarse en el runtime async de Tauri.
async fn pkcs11_blocking<R: Send + 'static>(
    f: impl FnOnce() -> Result<R, String> + Send + 'static,
) -> Result<R, String> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| format!("pkcs11 task: {e}"))?
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchSignIntentPayload {
    pub inputs: Vec<String>,
    pub output_dir: Option<String>,
}

/// Lee una solicitud guardada por `POST /api/v1/batch/sign/intent` (solo proceso NexoSign).
fn get_batch_sign_intent_from_store(
    request_id: &str,
    store: &Arc<Mutex<HashMap<String, PendingBatchIntent>>>,
    db_path: Option<&std::path::Path>,
) -> Result<Option<BatchSignIntentPayload>, String> {
    let mut g = store.lock().map_err(|e| e.to_string())?;
    let Some(ent) = g.get(request_id) else {
        return Ok(None);
    };
    if ent.is_expired() {
        if let Some(ref dir) = ent.staging_dir {
            let _ = std::fs::remove_dir_all(dir);
        }
        g.remove(request_id);
        if let Some(p) = db_path {
            let _ = queue_store::delete_intent_payload(p, request_id);
        }
        return Ok(None);
    }
    Ok(Some(BatchSignIntentPayload {
        inputs: ent
            .inputs
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect(),
        output_dir: ent
            .output_dir
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned()),
    }))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingIntentRowDto {
    pub request_id: String,
    pub file_count: usize,
    pub label: String,
    /// Unix epoch seconds (misma semántica que `PendingBatchIntent::created_unix`).
    pub created_at: u64,
}

fn label_for_pending_intent(ent: &PendingBatchIntent) -> String {
    let n = ent.inputs.len();
    let first = ent
        .inputs
        .first()
        .and_then(|p| p.file_name())
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    match n {
        0 => "Sin archivos".to_string(),
        1 => format!("1 PDF · {first}"),
        _ => format!("{n} PDF · {first}"),
    }
}

/// Lista intenciones `POST …/batch/sign/intent` aún no consumidas (purga caducadas).
#[tauri::command]
pub fn list_pending_batch_intents(
    pending: tauri::State<'_, PendingBatchIntents>,
    db_path: tauri::State<'_, OriginDbPath>,
) -> Result<Vec<PendingIntentRowDto>, String> {
    let sqlite = db_path.0.as_ref();
    let mut g = pending.0.lock().map_err(|e| e.to_string())?;
    let keys: Vec<String> = g.keys().cloned().collect();
    let mut rows = Vec::new();
    for k in keys {
        let Some(ent) = g.get(&k) else {
            continue;
        };
        if ent.is_expired() {
            if let Some(ref dir) = ent.staging_dir {
                let _ = std::fs::remove_dir_all(dir);
            }
            g.remove(&k);
            let _ = queue_store::delete_intent_payload(sqlite, &k);
            continue;
        }
        rows.push(PendingIntentRowDto {
            request_id: k.clone(),
            file_count: ent.inputs.len(),
            label: label_for_pending_intent(ent),
            created_at: ent.created_unix,
        });
    }
    rows.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(rows)
}

/// Quita una intención pendiente (p. ej. usuario descarta en Colas); borra staging si aplica.
#[tauri::command]
pub fn remove_pending_batch_intent(
    request_id: String,
    pending: tauri::State<'_, PendingBatchIntents>,
    db_path: tauri::State<'_, OriginDbPath>,
) -> Result<(), String> {
    let mut g = pending.0.lock().map_err(|e| e.to_string())?;
    if let Some(ent) = g.remove(&request_id) {
        if let Some(ref dir) = ent.staging_dir {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
    let _ = queue_store::delete_intent_payload(db_path.0.as_ref(), &request_id);
    Ok(())
}

/// Lee una solicitud guardada por `POST /api/v1/batch/sign/intent` (solo proceso NexoSign).
#[tauri::command]
pub fn get_batch_sign_intent(
    request_id: String,
    pending: tauri::State<'_, PendingBatchIntents>,
    db_path: tauri::State<'_, OriginDbPath>,
) -> Result<Option<BatchSignIntentPayload>, String> {
    get_batch_sign_intent_from_store(&request_id, &pending.0, Some(db_path.0.as_ref()))
}

#[tauri::command]
pub fn get_local_api_base_url(local_api: tauri::State<'_, std::sync::Arc<LocalApiRuntime>>) -> String {
    base_url_for_port(local_api.configured_port())
}

#[tauri::command]
pub fn get_local_api_status(
    local_api: tauri::State<'_, std::sync::Arc<LocalApiRuntime>>,
) -> LocalApiListenSnapshot {
    local_api.snapshot()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchJobMaxSecsConfig {
    /// Valor que usa el servidor (env > ajustes > default).
    pub effective_secs: u64,
    /// Si `NEXOSIGN_BATCH_JOB_MAX_SECS` está definida, el ajuste en UI no aplica hasta quitarla.
    pub locked_by_env: bool,
    /// Valor guardado en SQLite (o default si nunca se guardó).
    pub stored_secs: u64,
}

#[tauri::command]
pub fn get_batch_job_max_secs_config() -> BatchJobMaxSecsConfig {
    use crate::ports::batch_job_snapshot::{
        env_batch_job_max_secs_override, queue_max_wall_clock_secs, stored_queue_max_wall_clock_secs,
    };
    BatchJobMaxSecsConfig {
        effective_secs: queue_max_wall_clock_secs(),
        locked_by_env: env_batch_job_max_secs_override().is_some(),
        stored_secs: stored_queue_max_wall_clock_secs(),
    }
}

#[tauri::command]
pub fn set_batch_job_max_secs(
    secs: u64,
    db_path: tauri::State<'_, OriginDbPath>,
) -> Result<(), String> {
    use crate::ports::batch_job_snapshot::{
        env_batch_job_max_secs_override, init_stored_queue_max_secs_from_db, MAX_QUEUE_MAX_SECS,
        MIN_QUEUE_MAX_SECS,
    };
    if env_batch_job_max_secs_override().is_some() {
        return Err(
            "NEXOSIGN_BATCH_JOB_MAX_SECS está definida en el entorno; tiene prioridad. "
                .to_string()
                + "Quítala para usar el valor de Ajustes.",
        );
    }
    if !(MIN_QUEUE_MAX_SECS..=MAX_QUEUE_MAX_SECS).contains(&secs) {
        return Err(format!(
            "el valor debe estar entre {MIN_QUEUE_MAX_SECS} y {MAX_QUEUE_MAX_SECS} segundos"
        ));
    }
    queue_store::set_batch_job_max_secs_stored(db_path.0.as_ref(), secs)?;
    init_stored_queue_max_secs_from_db(Some(secs));
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClearLocalApiTempCacheResult {
    /// Se borró `{TMP}/nexosign-intent-uploads`.
    pub intent_uploads_removed: bool,
    /// Se borró `{TMP}/nexosign-batch-signed`.
    pub batch_signed_removed: bool,
    /// Se vació el mapa en RAM de rutas para `GET …/files/{i}`.
    pub signed_job_paths_cleared: bool,
}

/// Quita carpetas temporales usadas por la API local (subidas multipart y PDF firmados para descarga)
/// y sincroniza el mapa en memoria de la API para que el manifiesto no apunte a ficheros ya borrados.
#[tauri::command]
pub fn clear_local_api_temp_cache(
    store: tauri::State<'_, BatchSignedOutputsStore>,
) -> Result<ClearLocalApiTempCacheResult, String> {
    let mut out = ClearLocalApiTempCacheResult {
        intent_uploads_removed: false,
        batch_signed_removed: false,
        signed_job_paths_cleared: false,
    };
    {
        let mut g = store.0.lock().map_err(|e| e.to_string())?;
        g.clear();
        out.signed_job_paths_cleared = true;
    }
    let tmp = std::env::temp_dir();
    let intent = tmp.join("nexosign-intent-uploads");
    if intent.exists() {
        std::fs::remove_dir_all(&intent).map_err(|e| format!("nexosign-intent-uploads: {e}"))?;
        out.intent_uploads_removed = true;
    }
    let signed = tmp.join("nexosign-batch-signed");
    if signed.exists() {
        std::fs::remove_dir_all(&signed).map_err(|e| format!("nexosign-batch-signed: {e}"))?;
        out.batch_signed_removed = true;
    }
    Ok(out)
}

#[tauri::command]
pub fn list_allowed_origins(
    state: tauri::State<'_, OriginsStore>,
) -> Result<Vec<String>, String> {
    state
        .read()
        .map(|o| o.origins().to_vec())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_allowed_origin(
    origin: String,
    state: tauri::State<'_, OriginsStore>,
    db_path: tauri::State<'_, OriginDbPath>,
) -> Result<(), String> {
    let db = AllowedOriginsDb::open(db_path.0.as_ref()).map_err(|e| e.to_string())?;
    db.insert_origin(&origin).map_err(|e| e.to_string())?;
    state
        .write()
        .map_err(|e| e.to_string())?
        .add_if_absent(&origin);
    Ok(())
}

#[tauri::command]
pub fn remove_allowed_origin(
    origin: String,
    state: tauri::State<'_, OriginsStore>,
    db_path: tauri::State<'_, OriginDbPath>,
) -> Result<(), String> {
    let db = AllowedOriginsDb::open(db_path.0.as_ref()).map_err(|e| e.to_string())?;
    db.delete_origin(&origin).map_err(|e| e.to_string())?;
    state
        .write()
        .map_err(|e| e.to_string())?
        .remove_matching(&origin);
    Ok(())
}

#[tauri::command]
pub fn cancel_batch_job(
    job_id: String,
    registry: tauri::State<'_, BatchCancelRegistry>,
) -> Result<bool, String> {
    let token = registry
        .0
        .lock()
        .map_err(|e| e.to_string())?
        .remove(&job_id);
    Ok(match token {
        Some(t) => {
            t.cancel();
            true
        }
        None => false,
    })
}

#[tauri::command]
pub fn list_pkcs11_driver_paths(
    db_path: tauri::State<'_, OriginDbPath>,
) -> Result<Vec<String>, String> {
    let db = Pkcs11PathsDb::open(db_path.0.as_ref()).map_err(|e| e.to_string())?;
    db.list_paths_ordered().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_pkcs11_driver_path(
    path: String,
    db_path: tauri::State<'_, OriginDbPath>,
    pkcs11: tauri::State<'_, Pkcs11Store>,
) -> Result<(), String> {
    let db = Pkcs11PathsDb::open(db_path.0.as_ref()).map_err(|e| e.to_string())?;
    db.insert_path(&path).map_err(|e| e.to_string())?;
    pkcs11
        .reset_pkcs11_driver_state()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn remove_pkcs11_driver_path(
    path: String,
    db_path: tauri::State<'_, OriginDbPath>,
    pkcs11: tauri::State<'_, Pkcs11Store>,
) -> Result<(), String> {
    let db = Pkcs11PathsDb::open(db_path.0.as_ref()).map_err(|e| e.to_string())?;
    db.delete_path(&path).map_err(|e| e.to_string())?;
    pkcs11
        .reset_pkcs11_driver_state()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn reset_pkcs11_driver_paths_to_defaults(
    db_path: tauri::State<'_, OriginDbPath>,
    pkcs11: tauri::State<'_, Pkcs11Store>,
) -> Result<(), String> {
    let db = Pkcs11PathsDb::open(db_path.0.as_ref()).map_err(|e| e.to_string())?;
    db.reset_to_builtin_defaults().map_err(|e| e.to_string())?;
    pkcs11
        .reset_pkcs11_driver_state()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_pkcs11_driver_paths_order(
    paths: Vec<String>,
    db_path: tauri::State<'_, OriginDbPath>,
    pkcs11: tauri::State<'_, Pkcs11Store>,
) -> Result<(), String> {
    let mut db = Pkcs11PathsDb::open(db_path.0.as_ref()).map_err(|e| e.to_string())?;
    db.set_paths_ordered(&paths).map_err(|e| e.to_string())?;
    pkcs11
        .reset_pkcs11_driver_state()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_pkcs11_preferred_module(
    db_path: tauri::State<'_, OriginDbPath>,
) -> Result<Option<String>, String> {
    let db = Pkcs11PathsDb::open(db_path.0.as_ref()).map_err(|e| e.to_string())?;
    db.get_preferred_module_path().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_pkcs11_preferred_module(
    path: Option<String>,
    db_path: tauri::State<'_, OriginDbPath>,
    pkcs11: tauri::State<'_, Pkcs11Store>,
) -> Result<(), String> {
    let db = Pkcs11PathsDb::open(db_path.0.as_ref()).map_err(|e| e.to_string())?;
    db.set_preferred_module_path(path.as_deref()).map_err(|e| e.to_string())?;
    pkcs11
        .reset_pkcs11_driver_state()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_pkcs11_effective_module_paths(
    db_path: tauri::State<'_, OriginDbPath>,
) -> Result<Vec<String>, String> {
    let db = Pkcs11PathsDb::open(db_path.0.as_ref()).map_err(|e| e.to_string())?;
    let ordered: Vec<PathBuf> = db
        .list_paths_ordered()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(PathBuf::from)
        .collect();
    let paths = find_all_pkcs11_modules(Some(&ordered)).map_err(|e| e.to_string())?;
    Ok(paths
        .into_iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect())
}

#[tauri::command]
pub fn demo_emit_progress(app: AppHandle) -> Result<(), String> {
    app.emit(
        "progreso",
        json!({
            "actual": 1,
            "total": 10,
            "job_id": "cmd-demo",
        }),
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn probe_pkcs11_module_path(
    state: tauri::State<'_, Pkcs11Store>,
) -> Result<String, String> {
    let mgr = Arc::clone(&*state);
    pkcs11_blocking(move || mgr.probe_module_path().map_err(|e| e.to_string())).await
}

#[tauri::command]
pub async fn pkcs11_diagnose_slots(
    state: tauri::State<'_, Pkcs11Store>,
) -> Result<Pkcs11Diagnostics, String> {
    let mgr = Arc::clone(&*state);
    pkcs11_blocking(move || mgr.diagnose_slots().map_err(|e| e.to_string())).await
}

#[tauri::command]
pub async fn pkcs11_slot_count(state: tauri::State<'_, Pkcs11Store>) -> Result<usize, String> {
    let mgr = Arc::clone(&*state);
    pkcs11_blocking(move || mgr.slot_count_with_token().map_err(|e| e.to_string())).await
}

#[tauri::command]
pub async fn pkcs11_probe_certificate_listing(
    state: tauri::State<'_, Pkcs11Store>,
) -> Result<Pkcs11ProbeCertificateListing, String> {
    let mgr = Arc::clone(&*state);
    pkcs11_blocking(move || mgr.probe_certificate_listing().map_err(|e| e.to_string())).await
}

#[cfg(windows)]
fn append_windows_my_signing_certs(mut v: Vec<SigningCertSummary>) -> Vec<SigningCertSummary> {
    match crate::adapters::windows_cert_store::list_my_store_signing_rsa_certs() {
        Ok(mut w) => v.append(&mut w),
        Err(e) => tracing::warn!(error = %e, "listado certificados almacén Windows MY"),
    }
    dedupe_signing_certs_prefer_pkcs11(v)
}

#[cfg(not(windows))]
fn append_windows_my_signing_certs(v: Vec<SigningCertSummary>) -> Vec<SigningCertSummary> {
    v
}

#[tauri::command]
pub async fn pkcs11_list_signing_with_pin(
    state: tauri::State<'_, Pkcs11Store>,
    pin: String,
) -> Result<Vec<SigningCertSummary>, String> {
    let mgr = Arc::clone(&*state);
    pkcs11_blocking(move || {
        let v = mgr
            .list_pkcs11_signing_with_pin(pin)
            .map_err(|e| e.to_string())?;
        Ok(append_windows_my_signing_certs(v))
    })
    .await
}

#[tauri::command]
pub async fn list_signing_certificates(
    state: tauri::State<'_, Pkcs11Store>,
) -> Result<Vec<SigningCertSummary>, String> {
    let mgr = Arc::clone(&*state);
    pkcs11_blocking(move || {
        let v = mgr.list_signing_certificates().map_err(|e| e.to_string())?;
        Ok(append_windows_my_signing_certs(v))
    })
    .await
}

#[tauri::command]
pub async fn pkcs11_login(
    state: tauri::State<'_, Pkcs11Store>,
    pin: String,
    cert_id_hex: Option<String>,
) -> Result<(), String> {
    let mgr = Arc::clone(&*state);
    pkcs11_blocking(move || {
        match cert_id_hex
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            Some(hex) if crate::domain::signing_cert::is_win_my_cert_id(hex) => Ok(()),
            Some(hex) => mgr.login_for_certificate(pin, hex),
            None => mgr.login(pin),
        }
        .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn pkcs11_verify_pin(
    state: tauri::State<'_, Pkcs11Store>,
    pin: String,
    cert_id_hex: String,
) -> Result<(), String> {
    let mgr = Arc::clone(&*state);
    pkcs11_blocking(move || {
        let cert_id = cert_id_hex.trim();
        if crate::domain::signing_cert::is_win_my_cert_id(cert_id) {
            return Ok(());
        }
        mgr.verify_pin(pin, cert_id).map_err(|e| e.to_string())
    })
    .await
}

/// Valida PDFs del mismo modo que la API batch (tamaño máx., `.pdf`, rutas absolutas).
#[tauri::command]
pub fn validate_batch_pdf_paths(paths: Vec<String>) -> Result<(), String> {
    let pb: Vec<std::path::PathBuf> = paths.into_iter().map(std::path::PathBuf::from).collect();
    crate::infrastructure::batch_pdf_validation::validate_batch_pdf_inputs(&pb)
}

/// Devuelve rutas aceptadas y rechazadas por archivo (tamaño, extensión, etc.).
#[tauri::command]
pub fn partition_batch_pdf_paths(
    paths: Vec<String>,
) -> (
    Vec<String>,
    Vec<crate::infrastructure::batch_pdf_validation::RejectedPdfPath>,
) {
    let pb: Vec<std::path::PathBuf> = paths.into_iter().map(std::path::PathBuf::from).collect();
    let (ok, rej) = crate::infrastructure::batch_pdf_validation::partition_pdf_paths(pb);
    let ok_s: Vec<String> = ok.into_iter().map(|p| p.display().to_string()).collect();
    (ok_s, rej)
}

#[tauri::command]
pub async fn pkcs11_logout(state: tauri::State<'_, Pkcs11Store>) -> Result<(), String> {
    let mgr = Arc::clone(&*state);
    pkcs11_blocking(move || mgr.logout().map_err(|e| e.to_string())).await
}

#[tauri::command]
pub async fn pkcs11_session_status(
    state: tauri::State<'_, Pkcs11Store>,
) -> Result<SessionStatusDto, String> {
    let mgr = Arc::clone(&*state);
    tokio::task::spawn_blocking(move || mgr.session_status())
        .await
        .map_err(|e| format!("pkcs11 task: {e}"))
}

/// Libera sesión y módulo PKCS#11 en memoria (tras PIN incorrecto o lector «colgado»).
#[tauri::command]
pub async fn pkcs11_reset_connection(state: tauri::State<'_, Pkcs11Store>) -> Result<(), String> {
    let mgr = Arc::clone(&*state);
    pkcs11_blocking(move || mgr.reset_pkcs11_driver_state().map_err(|e| e.to_string())).await
}

fn collect_pdfs_recursive(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_pdfs_recursive(&path, out)?;
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("pdf"))
            .unwrap_or(false)
        {
            out.push(path);
        }
    }
    Ok(())
}

/// Lista rutas absolutas a `.pdf` bajo una carpeta (recursivo), ordenadas.
#[tauri::command]
pub fn enumerate_pdfs_under_folder(path: String) -> Result<Vec<String>, String> {
    let p = std::path::PathBuf::from(path.trim());
    if !p.is_absolute() {
        return Err("la ruta de carpeta debe ser absoluta".into());
    }
    if !p.is_dir() {
        return Err("no es un directorio existente".into());
    }
    let mut out = Vec::new();
    collect_pdfs_recursive(&p, &mut out).map_err(|e| e.to_string())?;
    out.sort();
    Ok(out
        .into_iter()
        .map(|x| x.to_string_lossy().into_owned())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn get_batch_sign_intent_from_store_cleans_expired_staging() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let rid = "rid-expired".to_string();
        let staging = std::env::temp_dir().join(format!("nexosign-cmd-expired-{}", std::process::id()));
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(staging.join("a.pdf"), b"%PDF-1.4\n").unwrap();

        let ent = PendingBatchIntent {
            inputs: vec![std::path::PathBuf::from("/tmp/a.pdf")],
            output_dir: None,
            staging_dir: Some(staging.clone()),
            created_unix: now.saturating_sub(crate::ports::queue_max_wall_clock_secs() + 5),
        };
        let store = Arc::new(Mutex::new(HashMap::from([(rid.clone(), ent)])));

        let out = get_batch_sign_intent_from_store(&rid, &store, None).unwrap();
        assert!(out.is_none());
        assert!(!staging.exists(), "staging debe borrarse al expirar");
        assert!(store.lock().unwrap().is_empty());
    }

    #[test]
    fn validate_partition_and_enumerate_pdf_commands() {
        let dir = std::env::temp_dir().join(format!(
            "nexosign-cmd-pdf-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let ok_pdf = dir.join("good.pdf");
        std::fs::write(&ok_pdf, b"%PDF-1.7\n").unwrap();
        let ok_abs = ok_pdf.canonicalize().unwrap();
        let ok_s = ok_abs.to_string_lossy().into_owned();

        validate_batch_pdf_paths(vec![ok_s.clone()]).unwrap();

        let bad_txt = dir.join("x.txt");
        std::fs::write(&bad_txt, b"hi").unwrap();
        let bad_s = bad_txt.canonicalize().unwrap().to_string_lossy().into_owned();

        let (accepted, rejected) = partition_batch_pdf_paths(vec![ok_s.clone(), bad_s]);
        assert_eq!(accepted.len(), 1);
        assert_eq!(rejected.len(), 1);

        let listed = enumerate_pdfs_under_folder(dir.to_string_lossy().into_owned()).unwrap();
        assert!(listed.iter().any(|p| p.contains("good.pdf")));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
