//! Errores de la capa token PKCS#11.

use thiserror::Error;

use super::driver::DriverPathError;

#[derive(Debug, Error)]
pub enum TokenError {
    #[error(transparent)]
    Driver(#[from] DriverPathError),
    #[error(transparent)]
    Pkcs11(#[from] cryptoki::error::Error),
    /// Prefijo reconocible por la UI (`PKCS11_NO_TOKEN:`); el resto es texto para el usuario.
    /// La UI lo intercepta y muestra un mensaje amigable; aquí dejamos un texto sin jerga por si llega al toast.
    #[error("PKCS11_NO_TOKEN: no se detecta el DNIe ni la tarjeta. Conecta el lector e inserta tu tarjeta.")]
    NoSlot,
    #[error("No se ha podido seleccionar el puerto del lector indicado.")]
    SlotIndex,
    #[error("Falta el PIN para firmar.")]
    NotLoggedIn,
    #[error("Introduce un PIN para firmar.")]
    EmptyPin,
    #[error("PIN incorrecto")]
    PinIncorrect,
    #[error("PIN bloqueado (demasiados intentos fallidos)")]
    PinLocked,
    #[error("Identificador de certificado no válido.")]
    BadCertId,
    #[error("certificado del almacén de Windows; no usa el token PKCS#11")]
    WinMyNotPkcs11,
    #[error("no se pudo interpretar el certificado X.509")]
    InvalidCertDer,
    #[error("No se ha encontrado la clave privada del certificado seleccionado en la tarjeta.")]
    NoPrivateKey,
    #[error("Tipo de clave no compatible para la firma (de momento solo se admite RSA).")]
    UnsupportedKeyType,
    #[error("Estado interno bloqueado; cierra y vuelve a abrir NexoSign.")]
    MutexPoisoned,
}
