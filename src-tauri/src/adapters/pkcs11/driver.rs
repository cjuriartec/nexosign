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

/// Devuelve todas las rutas de módulos PKCS#11 que existen físicamente en el sistema, ordenadas por prioridad.
pub fn find_all_pkcs11_modules() -> Result<Vec<PathBuf>, DriverPathError> {
    if let Ok(p) = std::env::var("NEXOSIGN_PKCS11_MODULE") {
        let pb = PathBuf::from(p.trim());
        if pb.is_file() {
            return Ok(vec![pb]);
        }
        return Err(DriverPathError::EnvPathMissing(pb));
    }

    let mut found = Vec::new();

    #[cfg(target_os = "windows")]
    let candidates = [
        // DNIe Perú (Bit4Id, Gemalto/IDPrime, SafeNet, ePass, IDProtect/Athena)
        r"C:\Windows\System32\bit4ipki.dll",
        r"C:\Windows\System32\idprimepkcs1164.dll",
        r"C:\Windows\System32\etpkcs11.dll",
        r"C:\Windows\System32\eps2003csp11.dll",
        r"C:\Windows\System32\asepkcs.dll",
        // Fallback: OpenSC genérico
        r"C:\Windows\System32\opensc-pkcs11.dll",
        r"C:\Windows\System32\pkcs11.dll",
    ];

    #[cfg(target_os = "linux")]
    let candidates = [
        "/usr/lib/x86_64-linux-gnu/opensc-pkcs11.so",
        "/usr/lib/opensc-pkcs11.so",
        "/usr/local/lib/opensc-pkcs11.so",
    ];

    #[cfg(target_os = "macos")]
    let candidates = [
        // DNIe Perú en Mac (IDProtect/Athena, Bit4Id, SafeNet) oficiales
        "/usr/local/lib/libasepkcs.dylib",
        "/Library/Application Support/Athena/lib/libasepkcs.dylib",
        "/usr/local/lib/libbit4ipki.dylib",
        "/Library/bit4id/pkcs11/libbit4ipki.dylib",
        "/usr/local/lib/libetpkcs11.dylib",
        // Fallback: OpenSC genérico
        "/opt/homebrew/lib/opensc-pkcs11.so",
        "/opt/homebrew/lib/pkcs11/opensc-pkcs11.so",
        "/usr/local/lib/opensc-pkcs11.so",
        "/Library/OpenSC/lib/opensc-pkcs11.so",
    ];

    for c in candidates {
        let pb = PathBuf::from(c);
        if pb.is_file() {
            found.push(pb);
        }
    }

    if found.is_empty() {
        return Err(DriverPathError::NotFound);
    }

    Ok(found)
}
