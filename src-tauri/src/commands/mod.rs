use serde_json::json;
use tauri::{AppHandle, Emitter};

use crate::adapters::http::LOCAL_API_PORT;

#[tauri::command]
pub fn get_local_api_base_url() -> String {
    format!("http://127.0.0.1:{LOCAL_API_PORT}")
}

#[tauri::command]
pub fn list_allowed_origins(
    state: tauri::State<'_, std::sync::Arc<std::sync::RwLock<crate::domain::allowed_origins::AllowedOrigins>>,
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
