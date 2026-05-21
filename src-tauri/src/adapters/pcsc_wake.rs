//! Despierta lectores PC/SC antes de PKCS#11 (misma idea que hacen ReFirma u otras apps al abrir el lector).
//!
//! Nunca llamar mientras hay una sesión PKCS#11 abierta: muchos drivers (p. ej. ASE) se bloquean.

use std::time::Duration;

use pcsc::{Context, Protocols, Scope, ShareMode};

const WAKE_TOTAL_TIMEOUT: Duration = Duration::from_secs(2);

fn wake_inner() {
    let Ok(ctx) = Context::establish(Scope::User) else {
        tracing::debug!("PC/SC: no se pudo establecer contexto");
        return;
    };
    let Ok(readers) = ctx.list_readers_owned() else {
        tracing::debug!("PC/SC: sin lectores");
        return;
    };
    for reader in readers {
        let name = reader.to_string_lossy();
        match ctx.connect(&reader, ShareMode::Shared, Protocols::ANY) {
            Ok(card) => {
                tracing::debug!(reader = %name, "PC/SC: lector contactado");
                drop(card);
            }
            Err(e) => {
                tracing::debug!(reader = %name, error = %e, "PC/SC: connect falló");
            }
        }
    }
}

/// Contacta lectores con tope de tiempo; no bloquea la UI si el driver PC/SC no responde.
pub fn wake_smart_card_readers() {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        wake_inner();
        let _ = tx.send(());
    });
    match rx.recv_timeout(WAKE_TOTAL_TIMEOUT) {
        Ok(()) => {}
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            tracing::warn!("PC/SC wake: tiempo de espera agotado ({WAKE_TOTAL_TIMEOUT:?})");
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {}
    }
}
