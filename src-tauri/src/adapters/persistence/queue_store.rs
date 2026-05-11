//! Tablas de colas en el mismo SQLite que orígenes/PKCS#11 (`allowed_origins.sqlite`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use rusqlite::{params, Connection, OptionalExtension};

use crate::domain::pending_batch_intent::PendingBatchIntent;
use serde::{Deserialize, Serialize};

/// Misma forma que `commands::batch_queue_history` (IPC / frontend).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchQueueHistoryPayload {
    #[serde(default)]
    pub items: Vec<BatchQueueItemPayload>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_batch_job_id: Option<String>,
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

pub fn ensure_queue_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS queue_batch_items (
            job_id TEXT PRIMARY KEY NOT NULL,
            status TEXT NOT NULL,
            label TEXT NOT NULL,
            progress_pct INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL,
            finished_at INTEGER,
            sort_order INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS queue_intent_items (
            request_id TEXT PRIMARY KEY NOT NULL,
            label TEXT NOT NULL,
            file_count INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL,
            sort_order INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS queue_ui_state (
            key TEXT PRIMARY KEY NOT NULL,
            value TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS intent_pending_payload (
            request_id TEXT PRIMARY KEY NOT NULL,
            inputs_json TEXT NOT NULL,
            output_dir TEXT,
            staging_dir TEXT,
            created_unix INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS batch_job_enqueue (
            job_id TEXT PRIMARY KEY NOT NULL,
            queued_at_unix INTEGER NOT NULL
        );
        "#,
    )?;
    Ok(())
}

/// Abre la BD de aplicación. El esquema de colas debe existir (véase [`ensure_queue_schema`] en arranque).
pub fn open_app_db(path: &Path) -> rusqlite::Result<Connection> {
    Connection::open(path)
}

/// Garantiza tablas `intent_pending_payload`, `batch_job_enqueue`, etc. Llamar una vez al iniciar el proceso.
pub fn init_queue_tables(db_path: &Path) -> Result<(), String> {
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
    ensure_queue_schema(&conn).map_err(|e| e.to_string())
}

pub fn upsert_intent_payload(
    db_path: &Path,
    request_id: &str,
    intent: &PendingBatchIntent,
) -> Result<(), String> {
    let conn = open_app_db(db_path).map_err(|e| e.to_string())?;
    let inputs: Vec<String> = intent
        .inputs
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    let inputs_json = serde_json::to_string(&inputs).map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT OR REPLACE INTO intent_pending_payload (request_id, inputs_json, output_dir, staging_dir, created_unix) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            request_id,
            inputs_json,
            intent
                .output_dir
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned()),
            intent
                .staging_dir
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned()),
            intent.created_unix as i64,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn delete_intent_payload(db_path: &Path, request_id: &str) -> Result<(), String> {
    let conn = open_app_db(db_path).map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM intent_pending_payload WHERE request_id = ?1",
        params![request_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn upsert_batch_job_enqueue(
    db_path: &Path,
    job_id: &str,
    queued_at_unix: i64,
) -> Result<(), String> {
    let conn = open_app_db(db_path).map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT OR REPLACE INTO batch_job_enqueue (job_id, queued_at_unix) VALUES (?1, ?2)",
        params![job_id, queued_at_unix],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn delete_batch_job_enqueue(db_path: &Path, job_id: &str) -> Result<(), String> {
    let conn = open_app_db(db_path).map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM batch_job_enqueue WHERE job_id = ?1",
        params![job_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// `job_id` cuyo encolado en SQLite es anterior a `cutoff_unix` (reloj de pared del vigía).
pub fn list_batch_job_ids_enqueued_before(
    db_path: &Path,
    cutoff_unix: i64,
) -> Result<Vec<String>, String> {
    if !db_path.exists() {
        return Ok(vec![]);
    }
    let conn = open_app_db(db_path).map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare("SELECT job_id FROM batch_job_enqueue WHERE queued_at_unix < ?1")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![cutoff_unix], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

/// Limpia filas de encolados anteriores a un corte (p. ej. arranque tras cierre inesperado).
pub fn purge_batch_job_enqueue_before(db_path: &Path, cutoff_unix: i64) -> Result<(), String> {
    if !db_path.exists() {
        return Ok(());
    }
    let conn = open_app_db(db_path).map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM batch_job_enqueue WHERE queued_at_unix < ?1",
        params![cutoff_unix],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Carga intents desde SQLite al mapa en RAM (arranque). Omite caducados y los borra de disco.
pub fn hydrate_pending_intents_from_db(
    db_path: &Path,
    map: &Arc<Mutex<HashMap<String, PendingBatchIntent>>>,
) -> Result<(), String> {
    if !db_path.exists() {
        return Ok(());
    }
    let conn = open_app_db(db_path).map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT request_id, inputs_json, output_dir, staging_dir, created_unix FROM intent_pending_payload",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, i64>(4)? as u64,
            ))
        })
        .map_err(|e| e.to_string())?;

    let mut to_remove: Vec<String> = Vec::new();
    let mut insertions: Vec<(String, PendingBatchIntent)> = Vec::new();

    for row in rows {
        let (rid, inputs_json, od, sd, cu) = row.map_err(|e| e.to_string())?;
        let paths: Vec<String> = match serde_json::from_str(&inputs_json) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(
                    request_id = %rid,
                    error = %e,
                    "inputs_json corrupto en intent_pending_payload; fila eliminada"
                );
                to_remove.push(rid);
                continue;
            }
        };
        let inputs: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
        let intent = PendingBatchIntent::restore_from_storage(
            inputs,
            od.map(PathBuf::from),
            sd.map(PathBuf::from),
            cu,
        );
        if intent.is_expired() {
            if let Some(ref dir) = intent.staging_dir {
                let _ = std::fs::remove_dir_all(dir);
            }
            to_remove.push(rid);
        } else {
            insertions.push((rid, intent));
        }
    }

    for rid in &to_remove {
        let _ = conn.execute(
            "DELETE FROM intent_pending_payload WHERE request_id = ?1",
            params![rid],
        );
    }

    let mut g = map.lock().map_err(|e| e.to_string())?;
    for (rid, intent) in insertions {
        g.insert(rid, intent);
    }
    Ok(())
}

pub fn save_queue_snapshot(db_path: &Path, payload: &BatchQueueHistoryPayload) -> Result<(), String> {
    let mut conn = open_app_db(db_path).map_err(|e| e.to_string())?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM queue_batch_items", [])
        .map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM queue_intent_items", [])
        .map_err(|e| e.to_string())?;
    tx.execute(
        "DELETE FROM queue_ui_state WHERE key IN ('active_batch_job_id', 'active_intent_request_id')",
        [],
    )
    .map_err(|e| e.to_string())?;

    for (i, it) in payload.items.iter().enumerate() {
        tx.execute(
            "INSERT INTO queue_batch_items (job_id, status, label, progress_pct, created_at, finished_at, sort_order) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                it.job_id,
                it.status,
                it.label,
                it.progress_pct as i64,
                it.created_at,
                it.finished_at,
                i as i64,
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    for (i, it) in payload.intent_items.iter().enumerate() {
        tx.execute(
            "INSERT INTO queue_intent_items (request_id, label, file_count, created_at, sort_order) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                it.request_id,
                it.label,
                it.file_count as i64,
                it.created_at,
                i as i64,
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    if let Some(ref id) = payload.active_batch_job_id {
        tx.execute(
            "INSERT INTO queue_ui_state (key, value) VALUES ('active_batch_job_id', ?1)",
            params![id],
        )
        .map_err(|e| e.to_string())?;
    }
    if let Some(ref id) = payload.active_intent_request_id {
        tx.execute(
            "INSERT INTO queue_ui_state (key, value) VALUES ('active_intent_request_id', ?1)",
            params![id],
        )
        .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(())
}

pub fn load_queue_snapshot(db_path: &Path) -> Result<Option<BatchQueueHistoryPayload>, String> {
    if !db_path.exists() {
        return Ok(None);
    }
    let conn = open_app_db(db_path).map_err(|e| e.to_string())?;
    let n_batch: i64 = conn
        .query_row("SELECT COUNT(*) FROM queue_batch_items", [], |r| r.get(0))
        .unwrap_or(0);
    let n_intent: i64 = conn
        .query_row("SELECT COUNT(*) FROM queue_intent_items", [], |r| r.get(0))
        .unwrap_or(0);
    let n_ui: i64 = conn
        .query_row("SELECT COUNT(*) FROM queue_ui_state", [], |r| r.get(0))
        .unwrap_or(0);

    if n_batch == 0 && n_intent == 0 && n_ui == 0 {
        return Ok(None);
    }

    let mut stmt = conn
        .prepare(
            "SELECT job_id, status, label, progress_pct, created_at, finished_at FROM queue_batch_items ORDER BY sort_order ASC",
        )
        .map_err(|e| e.to_string())?;
    let items: Vec<BatchQueueItemPayload> = stmt
        .query_map([], |row| {
            Ok(BatchQueueItemPayload {
                job_id: row.get(0)?,
                status: row.get(1)?,
                label: row.get(2)?,
                progress_pct: row.get::<_, i64>(3)? as u32,
                created_at: row.get(4)?,
                finished_at: row.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    let mut stmt = conn
        .prepare(
            "SELECT request_id, label, file_count, created_at FROM queue_intent_items ORDER BY sort_order ASC",
        )
        .map_err(|e| e.to_string())?;
    let intent_items: Vec<IntentQueueItemPayload> = stmt
        .query_map([], |row| {
            Ok(IntentQueueItemPayload {
                request_id: row.get(0)?,
                label: row.get(1)?,
                file_count: row.get::<_, i64>(2)? as u32,
                created_at: row.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    let active_batch_job_id: Option<String> = conn
        .query_row(
            "SELECT value FROM queue_ui_state WHERE key = 'active_batch_job_id'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;

    let active_intent_request_id: Option<String> = conn
        .query_row(
            "SELECT value FROM queue_ui_state WHERE key = 'active_intent_request_id'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;

    Ok(Some(BatchQueueHistoryPayload {
        items,
        active_batch_job_id,
        intent_items,
        active_intent_request_id,
    }))
}

/// Clave en `queue_ui_state` para el tiempo máximo de lote (segundos) guardado en ajustes.
pub const QUEUE_UI_KEY_BATCH_JOB_MAX_SECS: &str = "batch_job_max_secs";

pub fn get_batch_job_max_secs_stored(db_path: &Path) -> Result<Option<u64>, String> {
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
    let val: Option<String> = conn
        .query_row(
            "SELECT value FROM queue_ui_state WHERE key = ?1",
            params![QUEUE_UI_KEY_BATCH_JOB_MAX_SECS],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let Some(s) = val else {
        return Ok(None);
    };
    s.parse::<u64>()
        .map(Some)
        .map_err(|_| "batch_job_max_secs almacenado no es un entero válido".into())
}

pub fn set_batch_job_max_secs_stored(db_path: &Path, secs: u64) -> Result<(), String> {
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT OR REPLACE INTO queue_ui_state (key, value) VALUES (?1, ?2)",
        params![QUEUE_UI_KEY_BATCH_JOB_MAX_SECS, secs.to_string()],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn temp_db_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "nexosign-queue-store-test-{}-{}",
            name,
            std::process::id()
        ))
    }

    #[test]
    fn init_queue_tables_creates_expected_tables() {
        let path = temp_db_path("schema");
        let _ = std::fs::remove_file(&path);
        init_queue_tables(&path).unwrap();
        let conn = Connection::open(&path).unwrap();
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='intent_pending_payload'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn upsert_and_delete_intent_payload_roundtrip() {
        let path = temp_db_path("intent");
        let _ = std::fs::remove_file(&path);
        init_queue_tables(&path).unwrap();
        let intent = PendingBatchIntent::restore_from_storage(
            vec![PathBuf::from("/tmp/a.pdf")],
            None,
            None,
            42,
        );
        upsert_intent_payload(&path, "rid-1", &intent).unwrap();
        delete_intent_payload(&path, "rid-1").unwrap();
        let conn = Connection::open(&path).unwrap();
        let cnt: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM intent_pending_payload WHERE request_id='rid-1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(cnt, 0);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn hydrate_removes_corrupt_inputs_json_row() {
        let path = temp_db_path("corrupt");
        let _ = std::fs::remove_file(&path);
        init_queue_tables(&path).unwrap();
        let conn = Connection::open(&path).unwrap();
        conn.execute(
            "INSERT INTO intent_pending_payload (request_id, inputs_json, output_dir, staging_dir, created_unix) VALUES (?1, ?2, NULL, NULL, ?3)",
            params!["bad-rid", "NOT_JSON_ARRAY", 99_i64],
        )
        .unwrap();
        drop(conn);
        let map = Arc::new(Mutex::new(HashMap::new()));
        hydrate_pending_intents_from_db(&path, &map).unwrap();
        assert!(map.lock().unwrap().is_empty());
        let conn = Connection::open(&path).unwrap();
        let cnt: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM intent_pending_payload WHERE request_id='bad-rid'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(cnt, 0);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn save_and_load_queue_snapshot_roundtrip() {
        let path = temp_db_path("snapshot");
        let _ = std::fs::remove_file(&path);
        init_queue_tables(&path).unwrap();
        let payload = BatchQueueHistoryPayload {
            items: vec![BatchQueueItemPayload {
                job_id: "j1".into(),
                status: "running".into(),
                label: "Lote".into(),
                progress_pct: 50,
                created_at: 1000,
                finished_at: None,
            }],
            active_batch_job_id: Some("j1".into()),
            intent_items: vec![IntentQueueItemPayload {
                request_id: "i1".into(),
                label: "1 PDF".into(),
                file_count: 1,
                created_at: 2000,
            }],
            active_intent_request_id: Some("i1".into()),
        };
        save_queue_snapshot(&path, &payload).unwrap();
        let loaded = load_queue_snapshot(&path).unwrap().expect("snapshot");
        assert_eq!(loaded.items.len(), 1);
        assert_eq!(loaded.items[0].job_id, "j1");
        assert_eq!(loaded.active_batch_job_id.as_deref(), Some("j1"));
        assert_eq!(loaded.intent_items.len(), 1);
        assert_eq!(loaded.active_intent_request_id.as_deref(), Some("i1"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn batch_job_enqueue_list_purge_delete() {
        let path = temp_db_path("enqueue");
        let _ = std::fs::remove_file(&path);
        init_queue_tables(&path).unwrap();
        upsert_batch_job_enqueue(&path, "old-job", 100).unwrap();
        upsert_batch_job_enqueue(&path, "new-job", 500).unwrap();
        let stale = list_batch_job_ids_enqueued_before(&path, 200).unwrap();
        assert!(stale.contains(&"old-job".to_string()));
        assert!(!stale.contains(&"new-job".to_string()));
        purge_batch_job_enqueue_before(&path, 200).unwrap();
        let conn = Connection::open(&path).unwrap();
        let cnt: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM batch_job_enqueue WHERE job_id='old-job'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(cnt, 0);
        delete_batch_job_enqueue(&path, "new-job").unwrap();
        let cnt2: i64 = conn
            .query_row("SELECT COUNT(*) FROM batch_job_enqueue", [], |r| r.get(0))
            .unwrap();
        assert_eq!(cnt2, 0);
        let _ = std::fs::remove_file(&path);
    }
}
