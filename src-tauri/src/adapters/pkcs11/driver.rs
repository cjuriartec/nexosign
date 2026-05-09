//! Resolución del módulo PKCS#11 nativo (`.dll` / `.so` / `.dylib`).

use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DriverPathError {
    #[error(
        "NEXOSIGN_PKCS11_MODULE apunta a un archivo que no existe: {0}"
    )]
    EnvPathMissing(PathBuf),
    #[error(
        "No se encontró ningún módulo PKCS#11. Instala OpenSC/DNIe o define NEXOSIGN_PKCS11_MODULE."
    )]
    NotFound,
}

/// Prioridad: variable `NEXOSIGN_PKCS11_MODULE`, luego rutas típicas por SO.
pub fn resolve_pkcs11_module_path() -> Result<PathBuf, DriverPathError> {
    if let Ok(p) = std::env::var("NEXOSIGN_PKCS11_MODULE") {
        let pb = PathBuf::from(p.trim());
        if pb.is_file() {
            return Ok(pb);
        }
        return Err(DriverPathError::EnvPathMissing(pb));
    }

    #[cfg(target_os = "windows")]
    {
        let candidates = [
            r"C:\Windows\System32\opensc-pkcs11.dll",
            r"C:\Windows\System32\pkcs11.dll",
            // DNIe Perú (Bit4Id, Gemalto/IDPrime, SafeNet, ePass, IDProtect/Athena)
            r"C:\Windows\System32\bit4ipki.dll",
            r"C:\Windows\System32\idprimepkcs1164.dll",
            r"C:\Windows\System32\etpkcs11.dll",
            r"C:\Windows\System32\eps2003csp11.dll",
            r"C:\Windows\System32\asepkcs.dll",
        ];
        if let Some(pb) = first_existing(&candidates) {
            return Ok(pb);
        }
    }

    #[cfg(target_os = "linux")]
    {
        let candidates = [
            "/usr/lib/x86_64-linux-gnu/opensc-pkcs11.so",
            "/usr/lib/opensc-pkcs11.so",
            "/usr/local/lib/opensc-pkcs11.so",
        ];
        if let Some(pb) = first_existing(&candidates) {
            return Ok(pb);
        }
    }

    #[cfg(target_os = "macos")]
    {
        let candidates = [
            "/opt/homebrew/lib/opensc-pkcs11.so",
            "/opt/homebrew/lib/pkcs11/opensc-pkcs11.so",
            "/usr/local/lib/opensc-pkcs11.so",
            "/Library/OpenSC/lib/opensc-pkcs11.so",
            // DNIe Perú en Mac (IDProtect/Athena, Bit4Id, SafeNet)
            "/usr/local/lib/libasepkcs.dylib",
            "/Library/Application Support/Athena/lib/libasepkcs.dylib",
            "/usr/local/lib/libbit4ipki.dylib",
            "/Library/bit4id/pkcs11/libbit4ipki.dylib",
            "/usr/local/lib/libetpkcs11.dylib",
        ];
        if let Some(pb) = first_existing(&candidates) {
            return Ok(pb);
        }
    }

    Err(DriverPathError::NotFound)
}

fn first_existing(paths: &[&str]) -> Option<PathBuf> {
    paths.iter().map(PathBuf::from).find(|p| p.is_file())
}
