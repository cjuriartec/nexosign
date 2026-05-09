use std::sync::{Arc, RwLock};

use serde_json::json;
use tauri::{AppHandle, Emitter};

use crate::adapters::http::LOCAL_API_PORT;
use crate::adapters::pkcs11::token::{Pkcs11Diagnostics, Pkcs11TokenManager, SessionStatusDto};
use crate::domain::allowed_origins::AllowedOrigins;
use crate::domain::signing_cert::SigningCertSummary;

/// Estado gestionado por Tauri (`.manage`) compartido con la API local.
type OriginsStore = Arc<RwLock<AllowedOrigins>>;

type Pkcs11Store = Arc<Pkcs11TokenManager>;

/// PKCS#11 bloquea el hilo (lector/tariffa); no debe ejecutarse en el runtime async de Tauri.
async fn pkcs11_blocking<R: Send + 'static>(
    f: impl FnOnce() -> Result<R, String> + Send + 'static,
) -> Result<R, String> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| format!("pkcs11 task: {e}"))?
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
