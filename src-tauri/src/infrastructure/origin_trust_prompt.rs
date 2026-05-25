//! Diálogo nativo para confiar en orígenes de cliente (el evento solo en webview no es fiable desde HTTP).

use std::sync::{Arc, RwLock, mpsc::sync_channel};

use tauri::AppHandle;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

use crate::adapters::persistence::AllowedOriginsDb;
use crate::domain::allowed_origins::AllowedOrigins;
use crate::domain::origin_policy::is_well_formed_origin;
/// Muestra el diálogo en el hilo principal y persiste el origen si el usuario acepta.
pub fn prompt_client_origin_trust_blocking(
    app: &AppHandle,
    origins: &Arc<RwLock<AllowedOrigins>>,
    db_path: Option<&std::path::Path>,
    origin: &str,
) -> bool {
    if !is_well_formed_origin(origin) {
        return false;
    }

    if origins
        .read()
        .map(|g| g.is_allowed_origin(origin))
        .unwrap_or(false)
    {
        return true;
    }

    let app = app.clone();
    let origin_owned = origin.to_string();
    let origins = Arc::clone(origins);
    let db_path = db_path.map(|p| p.to_path_buf());

    let (tx, rx) = sync_channel(1);

    let app_sched = app.clone();
    let app_ui = app.clone();
    if app_sched
        .run_on_main_thread(move || {
            let message = format!(
                "Un cliente solicita usar la API local de firma.\n\n\
                 Origen:\n{origin_owned}\n\n\
                 ¿Confiar en este origen?"
            );

            let origins = Arc::clone(&origins);
            let db_path = db_path.clone();
            let origin_for_store = origin_owned.clone();

            app_ui.dialog()
                .message(message)
                .title("NexoSign — origen no reconocido")
                .kind(MessageDialogKind::Warning)
                .buttons(MessageDialogButtons::OkCancelCustom(
                    "Confiar".to_string(),
                    "Cancelar".to_string(),
                ))
                .show(move |accepted| {
                    if accepted {
                        if let Some(ref path) = db_path {
                            if let Ok(db) = AllowedOriginsDb::open(path) {
                                if let Err(e) = db.insert_origin(&origin_for_store) {
                                    tracing::warn!(error = %e, "persistir origen permitido");
                                }
                            }
                        }
                        if let Ok(mut guard) = origins.write() {
                            guard.add_if_absent(&origin_for_store);
                        }
                    }
                    let _ = tx.send(accepted);
                });
        })
        .is_err()
    {
        tracing::warn!("run_on_main_thread origen: no se pudo programar el diálogo");
        return false;
    }

    rx.recv().unwrap_or(false)
}
