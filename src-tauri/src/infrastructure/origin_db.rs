//! Ruta al fichero SQLite de orígenes (compartida entre comandos Tauri).
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone)]
pub struct OriginDbPath(pub Arc<PathBuf>);
