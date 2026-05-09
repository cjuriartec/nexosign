//! Rutas candidatas a módulos PKCS#11 persistidas en SQLite (misma BD que orígenes).
//!
//! Las filas se **combinan** en tiempo de ejecución con las rutas incorporadas por SO (`driver.rs`):
//! la tabla guarda ubicaciones extra o el orden preferido, sin sustituir por completo el catálogo de candidatos.

use std::path::Path;

use rusqlite::{params, Connection};

use crate::adapters::pkcs11::driver::builtin_pkcs11_path_strings;

pub struct Pkcs11PathsDb {
    conn: Connection,
}

impl Pkcs11PathsDb {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS pkcs11_driver_paths (
                path TEXT PRIMARY KEY NOT NULL,
                sort_order INTEGER NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_pkcs11_paths_sort ON pkcs11_driver_paths(sort_order);
            CREATE TABLE IF NOT EXISTS pkcs11_settings (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                preferred_module_path TEXT
            );
            INSERT OR IGNORE INTO pkcs11_settings (id, preferred_module_path) VALUES (1, NULL);",
        )?;
        let mut s = Self { conn };
        s.seed_builtin_defaults_if_empty()?;
        Ok(s)
    }

    /// Si la tabla está vacía, inserta las rutas por SO definidas en `driver.rs`.
    fn seed_builtin_defaults_if_empty(&mut self) -> rusqlite::Result<()> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM pkcs11_driver_paths",
            [],
            |row| row.get(0),
        )?;
        if n > 0 {
            return Ok(());
        }
        let tx = self.conn.transaction()?;
        for (i, p) in builtin_pkcs11_path_strings().iter().enumerate() {
            tx.execute(
                "INSERT OR IGNORE INTO pkcs11_driver_paths (path, sort_order) VALUES (?1, ?2)",
                params![*p, i as i64],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Rutas en orden de prioridad (menor `sort_order` primero).
    pub fn list_paths_ordered(&self) -> rusqlite::Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT path FROM pkcs11_driver_paths ORDER BY sort_order ASC, path ASC",
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect()
    }

    pub fn insert_path(&self, raw: &str) -> rusqlite::Result<()> {
        let path = raw.trim();
        if path.is_empty() {
            return Ok(());
        }
        let max: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(sort_order), -1) FROM pkcs11_driver_paths",
            [],
            |row| row.get(0),
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO pkcs11_driver_paths (path, sort_order, created_at) VALUES (?1, ?2, datetime('now'))",
            params![path, max + 1],
        )?;
        Ok(())
    }

    pub fn delete_path(&self, raw: &str) -> rusqlite::Result<usize> {
        let path = raw.trim();
        self.conn
            .execute("DELETE FROM pkcs11_driver_paths WHERE path = ?1", params![path])
    }

    /// Reemplaza la lista completa de rutas en el orden deseado (prioridad: primera fila = más alta).
    pub fn set_paths_ordered(&mut self, paths: &[String]) -> rusqlite::Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM pkcs11_driver_paths", [])?;
        for (i, raw) in paths.iter().enumerate() {
            let p = raw.trim();
            if p.is_empty() {
                continue;
            }
            tx.execute(
                "INSERT INTO pkcs11_driver_paths (path, sort_order, created_at) VALUES (?1, ?2, datetime('now'))",
                params![p, i as i64],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn get_preferred_module_path(&self) -> rusqlite::Result<Option<String>> {
        self.conn.query_row(
            "SELECT preferred_module_path FROM pkcs11_settings WHERE id = 1",
            [],
            |row| row.get::<_, Option<String>>(0),
        )
    }

    /// `None` o cadena vacía = modo automático (solo orden de candidatos).
    pub fn set_preferred_module_path(&self, path: Option<&str>) -> rusqlite::Result<()> {
        let v = path.map(str::trim).filter(|s| !s.is_empty());
        self.conn.execute(
            "UPDATE pkcs11_settings SET preferred_module_path = ?1 WHERE id = 1",
            params![v],
        )?;
        Ok(())
    }

    /// Restaura solo las rutas incorporadas por plataforma (borra el resto).
    pub fn reset_to_builtin_defaults(&self) -> rusqlite::Result<()> {
        self.conn.execute("DELETE FROM pkcs11_driver_paths", [])?;
        for (i, p) in builtin_pkcs11_path_strings().iter().enumerate() {
            self.conn.execute(
                "INSERT INTO pkcs11_driver_paths (path, sort_order, created_at) VALUES (?1, ?2, datetime('now'))",
                params![*p, i as i64],
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(any(
        target_os = "windows",
        target_os = "linux",
        target_os = "macos"
    ))]
    fn seeds_and_lists() {
        let tmp = std::env::temp_dir().join(format!(
            "nexosign-p11-paths-{}.sqlite",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&tmp);
        let db = Pkcs11PathsDb::open(&tmp).unwrap();
        let list = db.list_paths_ordered().unwrap();
        assert!(!list.is_empty());
        let _ = std::fs::remove_file(&tmp);
    }
}
