//! Persistencia del historial de colas en SQLite (`allowed_origins.sqlite`, tablas `queue_*`).

use tauri::AppHandle;
use tauri::Manager;
use tauri::State;

pub use crate::adapters::persistence::queue_store::BatchQueueHistoryPayload;
use crate::adapters::persistence::queue_store;
use crate::infrastructure::origin_db::OriginDbPath;

const LEGACY_JSON: &str = "batch_queue_history.json";

#[tauri::command]
pub fn load_batch_queue_history(
    app: AppHandle,
    db_path: State<'_, OriginDbPath>,
) -> Result<Option<BatchQueueHistoryPayload>, String> {
    let sqlite = db_path.0.as_ref();
    if sqlite.exists() {
        if let Ok(Some(payload)) = queue_store::load_queue_snapshot(sqlite) {
            return Ok(Some(payload));
        }
    }

    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let legacy_path = dir.join(LEGACY_JSON);
    if !legacy_path.exists() {
        return Ok(None);
    }
    let s = std::fs::read_to_string(&legacy_path).map_err(|e| e.to_string())?;
    let v: BatchQueueHistoryPayload =
        serde_json::from_str(&s).map_err(|e| format!("batch_queue_history JSON: {e}"))?;

    if sqlite.parent().is_some() {
        let _ = queue_store::save_queue_snapshot(sqlite, &v);
    }
    let _ = std::fs::remove_file(&legacy_path);

    Ok(Some(v))
}

#[tauri::command]
pub fn save_batch_queue_history(
    db_path: State<'_, OriginDbPath>,
    payload: BatchQueueHistoryPayload,
) -> Result<(), String> {
    let path = db_path.0.as_ref();
    queue_store::save_queue_snapshot(path, &payload)?;
    if let Some(dir) = path.parent() {
        let _ = std::fs::remove_file(dir.join(LEGACY_JSON));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::adapters::persistence::queue_store::{
        self, BatchQueueHistoryPayload, BatchQueueItemPayload, IntentQueueItemPayload,
    };

    #[test]
    fn queue_snapshot_matches_command_persistence_contract() {
        let path = std::env::temp_dir().join(format!(
            "nexosign-bqh-cmd-{}.sqlite",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        queue_store::init_queue_tables(&path).unwrap();

        let payload = BatchQueueHistoryPayload {
            items: vec![BatchQueueItemPayload {
                job_id: "jb".into(),
                status: "queued".into(),
                label: "t".into(),
                progress_pct: 0,
                created_at: 1,
                finished_at: None,
            }],
            active_batch_job_id: None,
            intent_items: vec![IntentQueueItemPayload {
                request_id: "ir".into(),
                label: "i".into(),
                file_count: 2,
                created_at: 2,
            }],
            active_intent_request_id: Some("ir".into()),
        };

        queue_store::save_queue_snapshot(&path, &payload).unwrap();
        let loaded = queue_store::load_queue_snapshot(&path)
            .unwrap()
            .expect("snapshot");
        assert_eq!(loaded.items.len(), 1);
        assert_eq!(loaded.intent_items.len(), 1);
        assert_eq!(
            loaded.active_intent_request_id.as_deref(),
            Some("ir")
        );
        let _ = std::fs::remove_file(&path);
    }
}
