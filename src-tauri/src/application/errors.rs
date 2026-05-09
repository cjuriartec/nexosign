//! Errores del caso de uso de firma por lotes.

use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum SignBatchError {
    #[error("entrada inválida: {0}")]
    InvalidInput(String),
    #[error("PDF inválido u operación PAdES fallida: {0}")]
    Pades(String),
    #[error(transparent)]
    Token(#[from] crate::adapters::pkcs11::error::TokenError),
    #[error("IO en {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}
