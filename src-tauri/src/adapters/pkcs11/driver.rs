//! Resolución del módulo PKCS#11 nativo (`.dll` / `.so` / `.dylib`).
//!
//! Las rutas por defecto por sistema operativo pueden persistirse en SQLite (`pkcs11_driver_paths`);
//! aquí solo definimos el orden incorporado y la fusión con la variable de entorno.

use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DriverPathError {
    /// Variable de entorno reservada a usuarios avanzados; el mensaje conserva el nombre
    /// para que pueda diagnosticarse, pero evita jerga adicional.
    #[error(
        "La ruta indicada en NEXOSIGN_PKCS11_MODULE no existe: {0}. Corrige la ruta o elimina la variable para usar el controlador detectado automáticamente."
    )]
    EnvPathMissing(PathBuf),
    #[error(
        "No encontramos el controlador del lector. Instala el software del fabricante (por ejemplo el del DNIe o OpenSC) o añade su ruta en Ajustes → «Lector de DNIe y tarjetas»."
    )]
    NotFound,
}

/// Rutas incorporadas por plataforma (misma lista que antes en código); sirven para SQLite seed y fallback.
pub fn builtin_pkcs11_path_strings() -> &'static [&'static str] {
    #[cfg(target_os = "windows")]
    {
        &[
            // RENIEC DNI electrónico 3.0 (Perú) — IDPlug Classic / IDEMIA
            r"C:\Program Files\IDEMIA\IDPlugClassic\DLLs\idplug-pkcs11.dll",
            r"C:\Program Files (x86)\IDEMIA\IDPlugClassic\DLLs\idplug-pkcs11.dll",
            // DNIe 3.0 (España) — Bit4id / IDPrime
            r"C:\Windows\System32\bit4ipki.dll",
            r"C:\Windows\System32\idprimepkcs1164.dll",
            r"C:\Windows\System32\idprimepkcs1132.dll",
            r"C:\Windows\System32\etpkcs11.dll",
            r"C:\Windows\System32\eps2003csp11.dll",
            // DNIe 2.0 (España) — FNMT / Athena
            r"C:\Windows\System32\asepkcs.dll",
            r"C:\Windows\System32\opensc-pkcs11.dll",
            r"C:\Windows\System32\pkcs11.dll",
        ]
    }

    #[cfg(target_os = "linux")]
    {
        &[
            "/usr/lib/x86_64-linux-gnu/opensc-pkcs11.so",
            "/usr/lib/opensc-pkcs11.so",
            "/usr/local/lib/opensc-pkcs11.so",
        ]
    }

    #[cfg(target_os = "macos")]
    {
        &[
            "/usr/local/lib/libasepkcs.dylib",
            "/Library/Application Support/Athena/lib/libasepkcs.dylib",
            "/usr/local/lib/libbit4ipki.dylib",
            "/Library/bit4id/pkcs11/libbit4ipki.dylib",
            "/usr/local/lib/libetpkcs11.dylib",
            "/opt/homebrew/lib/opensc-pkcs11.so",
            "/opt/homebrew/lib/pkcs11/opensc-pkcs11.so",
            "/usr/local/lib/opensc-pkcs11.so",
            "/Library/OpenSC/lib/opensc-pkcs11.so",
        ]
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        &[]
    }
}

fn builtin_candidate_paths() -> Vec<PathBuf> {
    builtin_pkcs11_path_strings()
        .iter()
        .map(|s| PathBuf::from(*s))
        .collect()
}

/// Rutas candidatas existentes en disco, ordenadas por prioridad.
///
/// - Si existe `NEXOSIGN_PKCS11_MODULE` y el archivo existe, solo se usa esa ruta.
/// - Si no, se **fusionan** las rutas guardadas en SQLite (orden de la BD primero) con las rutas
///   incorporadas por SO; así no dependemos de un solo driver y se pueden añadir ubicaciones en BD
///   sin sustituir el resto de candidatos.
/// - Se deduplican rutas (sin distinguir mayúsculas en la clave).
pub fn find_all_pkcs11_modules(
    db_ordered: Option<&[PathBuf]>,
) -> Result<Vec<PathBuf>, DriverPathError> {
    if let Ok(p) = std::env::var("NEXOSIGN_PKCS11_MODULE") {
        let pb = PathBuf::from(p.trim());
        if pb.is_file() {
            return Ok(vec![pb]);
        }
        return Err(DriverPathError::EnvPathMissing(pb));
    }

    let mut merged: Vec<PathBuf> = Vec::new();
    let mut seen = std::collections::HashSet::<String>::new();

    let mut push_unique = |pb: PathBuf| {
        let key = pb.to_string_lossy().to_lowercase();
        if seen.insert(key) {
            merged.push(pb);
        }
    };

    if let Some(paths) = db_ordered {
        for p in paths {
            push_unique(p.clone());
        }
    }
    for p in builtin_candidate_paths() {
        push_unique(p);
    }

    let mut found = Vec::new();
    for pb in merged {
        if pb.is_file() {
            found.push(pb);
        }
    }

    if found.is_empty() {
        return Err(DriverPathError::NotFound);
    }

    Ok(found)
}
