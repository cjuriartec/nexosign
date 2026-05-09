//! Errores de la capa token PKCS#11.

use thiserror::Error;

use super::driver::DriverPathError;

#[derive(Debug, Error)]
pub enum TokenError {
    #[error(transparent)]
    Driver(#[from] DriverPathError),
    #[error(transparent)]
    Pkcs11(#[from] cryptoki::error::Error),
    #[error("no hay slots con token (¿tarjeta/DNIe insertado?)")]
    NoSlot,
    #[error("índice de slot inválido")]
    SlotIndex,
    #[error("sin sesión PKCS#11 iniciada (PIN requerido para firmar)")]
    NotLoggedIn,
    #[error("PIN vacío")]
    EmptyPin,
    #[error("identificador de certificado inválido")]
    BadCertId,
    #[error("no se encontró clave privada para el certificado indicado")]
    NoPrivateKey,
    #[error("tipo de clave no soportado para firma PAdES en esta fase (solo RSA)")]
    UnsupportedKeyType,
    #[error("estado interno bloqueado")]
    MutexPoisoned,
}
