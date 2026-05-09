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
    #[error("PIN vacío")]
    EmptyPin,
    #[error("estado interno bloqueado")]
    MutexPoisoned,
}
