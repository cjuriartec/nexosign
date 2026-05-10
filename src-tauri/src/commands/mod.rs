use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use serde::Serialize;
use serde_json::json;
use tauri::{AppHandle, Emitter};
use tokio_util::sync::CancellationToken;

use crate::adapters::http::LOCAL_API_PORT;
use crate::adapters::http::state::PendingBatchIntents;
use crate::adapters::persistence::{AllowedOriginsDb, Pkcs11PathsDb};
use crate::adapters::pkcs11::driver::find_all_pkcs11_modules;
use crate::adapters::pkcs11::token::{Pkcs11Diagnostics, Pkcs11TokenManager, SessionStatusDto};
use crate::domain::allowed_origins::AllowedOrigins;
use crate::domain::signing_cert::SigningCertSummary;
use crate::infrastructure::origin_db::OriginDbPath;

/// Estado gestionado por Tauri (`.manage`) compartido con la API local.
type OriginsStore = Arc<RwLock<AllowedOrigins>>;

type Pkcs11Store = Arc<Pkcs11TokenManager>;

/// Mismo `Arc` que [`crate::adapters::http::state::SharedState::batch_cancel`] para cancelar lotes vía IPC.
#[derive(Clone)]
pub struct BatchCancelRegistry(pub Arc<Mutex<HashMap<String, CancellationToken>>>);

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
#[tauri::command]
pub fn get_batch_sign_intent(
    request_id: String,
    pending: tauri::State<'_, PendingBatchIntents>,
) -> Result<Option<BatchSignIntentPayload>, String> {
    let mut g = pending.0.lock().map_err(|e| e.to_string())?;
    let Some(ent) = g.get(&request_id) else {
        return Ok(None);
    };
    if ent.is_expired() {
        g.remove(&request_id);
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

#[tauri::command]
pub fn get_local_api_base_url() -> String {
    format!("http://127.0.0.1:{LOCAL_API_PORT}")
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
pub async fn list_signing_certificates(
    state: tauri::State<'_, Pkcs11Store>,
) -> Result<Vec<SigningCertSummary>, String> {
    let mgr = Arc::clone(&*state);
    pkcs11_blocking(move || mgr.list_signing_certificates().map_err(|e| e.to_string())).await
}

#[tauri::command]
pub async fn pkcs11_login(state: tauri::State<'_, Pkcs11Store>, pin: String) -> Result<(), String> {
    let mgr = Arc::clone(&*state);
    pkcs11_blocking(move || mgr.login(pin).map_err(|e| e.to_string())).await
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
