//! Comprobación e instalación de actualizaciones vía GitHub Releases (Tauri Updater).

use std::sync::mpsc::sync_channel;
use std::time::Duration;

use tauri::AppHandle;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_updater::UpdaterExt;

const PERIODIC_CHECK_INTERVAL: Duration = Duration::from_secs(12 * 60 * 60);

pub fn current_version<R: tauri::Runtime>(app: &AppHandle<R>) -> String {
    app.package_info().version.to_string()
}

fn updater_enabled() -> bool {
    !cfg!(debug_assertions)
}

async fn fetch_update<R: tauri::Runtime>(
    app: &AppHandle<R>,
) -> Result<Option<tauri_plugin_updater::Update>, String> {
    let updater = app.updater().map_err(|e| e.to_string())?;
    updater.check().await.map_err(|e| e.to_string())
}

fn confirm_install<R: tauri::Runtime>(app: &AppHandle<R>, message: &str) -> bool {
    let (tx, rx) = sync_channel(1);
    let app_ui = app.clone();
    let message = message.to_string();

    if app
        .run_on_main_thread(move || {
            app_ui
                .dialog()
                .message(message)
                .title("NexoSign — actualización disponible")
                .kind(MessageDialogKind::Info)
                .buttons(MessageDialogButtons::OkCancelCustom(
                    "Instalar".to_string(),
                    "Ahora no".to_string(),
                ))
                .show(move |accepted| {
                    let _ = tx.send(accepted);
                });
        })
        .is_err()
    {
        tracing::warn!("no se pudo mostrar el diálogo de actualización");
        return false;
    }

    rx.recv().unwrap_or(false)
}

fn show_up_to_date<R: tauri::Runtime>(app: &AppHandle<R>, current: &str) {
    let (tx, rx) = sync_channel(1);
    let app_ui = app.clone();
    let message = format!("Ya tienes la última versión (v{current}).");

    if app
        .run_on_main_thread(move || {
            app_ui
                .dialog()
                .message(message)
                .title("NexoSign — actualizaciones")
                .kind(MessageDialogKind::Info)
                .show(move |_| {
                    let _ = tx.send(());
                });
        })
        .is_err()
    {
        return;
    }

    let _ = rx.recv();
}

/// Comprueba actualizaciones y, si procede, ofrece instalarlas con diálogo nativo.
pub async fn run_update_flow<R: tauri::Runtime>(app: AppHandle<R>, manual: bool) -> Result<(), String> {
    if !updater_enabled() {
        if manual {
            return Err(
                "Las actualizaciones solo están disponibles en la aplicación instalada.".to_string(),
            );
        }
        return Ok(());
    }

    let current = current_version(&app);

    let update = match fetch_update(&app).await {
        Ok(Some(update)) => update,
        Ok(None) => {
            if manual {
                show_up_to_date(&app, &current);
            }
            return Ok(());
        }
        Err(e) => {
            if manual {
                return Err(format!("No se pudo comprobar actualizaciones: {e}"));
            }
            tracing::warn!(error = %e, "comprobación periódica de actualización");
            return Ok(());
        }
    };

    let version = update.version.clone();
    let notes = update
        .body
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let message = match notes {
        Some(body) => format!(
            "Hay una nueva versión disponible: v{version}.\n\n{body}\n\n¿Instalar ahora?"
        ),
        None => format!("Hay una nueva versión disponible: v{version}.\n\n¿Instalar ahora?"),
    };

    if !confirm_install(&app, &message) {
        return Ok(());
    }

    update
        .download_and_install(
            |_chunk, _total| {},
            || tracing::info!("descarga de actualización completada"),
        )
        .await
        .map_err(|e| format!("No se pudo instalar la actualización: {e}"))?;

    app.restart();
}

/// Comprobación silenciosa cada 12 h mientras el proceso está activo (no al arrancar).
pub fn spawn_periodic_update_checks<R: tauri::Runtime>(app: AppHandle<R>) {
    if !updater_enabled() {
        return;
    }

    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(PERIODIC_CHECK_INTERVAL);
        interval.tick().await;
        loop {
            interval.tick().await;
            if let Err(e) = run_update_flow(app.clone(), false).await {
                tracing::warn!(error = %e, "comprobación periódica de actualización");
            }
        }
    });
}
