//! Persistencia SQLite de orígenes aprobados por el usuario (complemento a env + defaults).

use std::path::Path;

use rusqlite::{params, Connection};

use crate::domain::allowed_origins::AllowedOrigins;
use crate::domain::origin_policy::normalize_origin;

pub struct AllowedOriginsDb {
    conn: Connection,
}

impl AllowedOriginsDb {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS allowed_origins (
                origin TEXT PRIMARY KEY NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );",
        )?;
        Ok(Self { conn })
    }

    pub fn list_origins(&self) -> rusqlite::Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT origin FROM allowed_origins ORDER BY created_at ASC")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect()
    }

    pub fn insert_origin(&self, origin: &str) -> rusqlite::Result<()> {
        let n = normalize_origin(origin);
        self.conn.execute(
            "INSERT OR REPLACE INTO allowed_origins (origin, created_at) VALUES (?1, datetime('now'))",
            params![n],
        )?;
        Ok(())
    }

    pub fn delete_origin(&self, origin: &str) -> rusqlite::Result<usize> {
        let n = normalize_origin(origin);
        self.conn
            .execute("DELETE FROM allowed_origins WHERE origin = ?1", params![n])
    }

    /// Añade a `store` todos los orígenes guardados (sin quitar env/defaults).
    pub fn merge_into_allowed_origins(&self, store: &mut AllowedOrigins) -> rusqlite::Result<()> {
        for o in self.list_origins()? {
            store.add_if_absent(&o);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_insert_list() {
        let tmp = std::env::temp_dir().join("nexosign-origins-test.db");
        let _ = std::fs::remove_file(&tmp);
        let db = AllowedOriginsDb::open(&tmp).unwrap();
        db.insert_origin("  https://app.example/  ").unwrap();
        let list = db.list_origins().unwrap();
        assert_eq!(list, vec!["https://app.example".to_string()]);
    }
}
