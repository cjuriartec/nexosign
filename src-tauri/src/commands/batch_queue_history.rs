//! Persistencia del historial de colas de firma en `app_data_dir/batch_queue_history.json`.

use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri::Manager;

const FILE_NAME: &str = "batch_queue_history.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchQueueHistoryPayload {
    #[serde(default)]
    pub items: Vec<BatchQueueItemPayload>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_batch_job_id: Option<String>,
    /// Intents desde API (`POST …/intent`) pendientes de firmar en el asistente.
    #[serde(default)]
    pub intent_items: Vec<IntentQueueItemPayload>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_intent_request_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntentQueueItemPayload {
    pub request_id: String,
    pub label: String,
    #[serde(default)]
    pub file_count: u32,
    #[serde(default)]
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchQueueItemPayload {
    pub job_id: String,
    pub status: String,
    pub label: String,
    #[serde(default)]
    pub progress_pct: u32,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<i64>,
}

#[tauri::command]
pub fn load_batch_queue_history(app: AppHandle) -> Result<Option<BatchQueueHistoryPayload>, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let path = dir.join(FILE_NAME);
    if !path.exists() {
        return Ok(None);
    }
    let s = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let v: BatchQueueHistoryPayload =
        serde_json::from_str(&s).map_err(|e| format!("batch_queue_history JSON: {e}"))?;
    Ok(Some(v))
}

#[tauri::command]
pub fn save_batch_queue_history(
    app: AppHandle,
    payload: BatchQueueHistoryPayload,
) -> Result<(), String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(FILE_NAME);
    let s =
        serde_json::to_string_pretty(&payload).map_err(|e| format!("serializar historial: {e}"))?;
    std::fs::write(&path, s).map_err(|e| e.to_string())?;
    Ok(())
}
