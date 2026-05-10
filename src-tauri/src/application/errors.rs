//! Errores del caso de uso de firma por lotes.

use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum SignBatchError {
    #[error("entrada inválida: {0}")]
    InvalidInput(String),
    /// Mensaje libre; suele incluir detalle técnico para soporte (PKCS#11, CMS, DER).
    #[error("PDF inválido u operación PAdES fallida: {0}")]
    Pades(String),
    /// Error de sesión o módulo criptográfico (texto; el adaptador mapea el error concreto).
    #[error("sesión o módulo de firma: {0}")]
    Signer(String),
    #[error("IO en {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}
