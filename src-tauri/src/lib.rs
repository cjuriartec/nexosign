pub mod adapters;
pub mod application;
mod commands;
pub mod domain;
pub mod infrastructure;
pub mod ports;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Once, RwLock};

use adapters::persistence::queue_store;
use adapters::persistence::{AllowedOriginsDb, Pkcs11PathsDb};
use adapters::pkcs11::token::Pkcs11TokenManager;
use domain::allowed_origins::AllowedOrigins;
use infrastructure::origin_db::OriginDbPath;
use tauri::{Emitter, Manager, WindowEvent};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri_plugin_deep_link::DeepLinkExt;
use tokio_util::sync::CancellationToken;

use crate::domain::pending_batch_intent::PendingBatchIntent;
use crate::adapters::http::state::PendingBatchIntents;
use crate::commands::{BatchCancelRegistry, BatchSignedOutputsStore};

/// Etiqueta de la ventana principal ([`tauri.conf.json`](../../tauri.conf.json), [`capabilities/default.json`](../../capabilities/default.json)).
const MAIN_WINDOW_LABEL: &str = "main";

static INIT_TRACING: Once = Once::new();

fn show_main_window<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(w) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

#[cfg(desktop)]
fn try_install_tray<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let show = match MenuItem::with_id(app, "tray_show", "Abrir NexoSign", true, None::<&str>) {
        Ok(i) => i,
        Err(e) => {
            tracing::warn!(error = %e, "menú bandeja: ítem Abrir");
            return;
        }
    };
    let quit = match MenuItem::with_id(app, "tray_quit", "Salir", true, None::<&str>) {
        Ok(i) => i,
        Err(e) => {
            tracing::warn!(error = %e, "menú bandeja: ítem Salir");
            return;
        }
    };
    let menu = match Menu::with_items(app, &[&show, &quit]) {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(error = %e, "menú bandeja");
            return;
        }
    };

    let mut builder = TrayIconBuilder::new()
        .menu(&menu)
        .tooltip("NexoSign")
        .on_menu_event(|app, event| {
            if event.id == "tray_quit" {
                app.exit(0);
            } else if event.id == "tray_show" {
                show_main_window(app);
            }
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    if let Err(e) = builder.build(app) {
        tracing::warn!(error = %e, "icono de bandeja del sistema");
    }
}

fn init_tracing() {
    INIT_TRACING.call_once(|| {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                    tracing_subscriber::EnvFilter::new("nexosign=info,tower_http=info,warn")
                }),
            )
            .try_init();
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    init_tracing();

    let origins = Arc::new(RwLock::new(AllowedOrigins::from_env()));
    let app_db_slot = Arc::new(Mutex::new(None::<std::path::PathBuf>));
    let pkcs11 = Arc::new(Pkcs11TokenManager::new(app_db_slot.clone()));
    let batch_cancel = Arc::new(Mutex::new(HashMap::<String, CancellationToken>::new()));
    let pending_batch_intents: Arc<Mutex<HashMap<String, PendingBatchIntent>>> =
        Arc::new(Mutex::new(HashMap::new()));

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_deep_link::init())
        .manage(origins.clone())
        .manage(pkcs11.clone())
        .manage(BatchCancelRegistry(batch_cancel.clone()))
        .manage(PendingBatchIntents(pending_batch_intents.clone()))
        .invoke_handler(tauri::generate_handler![
            greet,
            commands::get_local_api_base_url,
            commands::list_allowed_origins,
            commands::add_allowed_origin,
            commands::remove_allowed_origin,
            commands::cancel_batch_job,
            commands::demo_emit_progress,
            commands::probe_pkcs11_module_path,
            commands::list_pkcs11_driver_paths,
            commands::add_pkcs11_driver_path,
            commands::remove_pkcs11_driver_path,
            commands::reset_pkcs11_driver_paths_to_defaults,
            commands::set_pkcs11_driver_paths_order,
            commands::get_pkcs11_preferred_module,
            commands::set_pkcs11_preferred_module,
            commands::list_pkcs11_effective_module_paths,
            commands::pkcs11_diagnose_slots,
            commands::pkcs11_slot_count,
            commands::list_signing_certificates,
            commands::pkcs11_login,
            commands::pkcs11_verify_pin,
            commands::pkcs11_logout,
            commands::pkcs11_session_status,
            commands::pkcs11_reset_connection,
            commands::get_batch_sign_intent,
            commands::list_pending_batch_intents,
            commands::remove_pending_batch_intent,
            commands::enumerate_pdfs_under_folder,
            commands::validate_batch_pdf_paths,
            commands::partition_batch_pdf_paths,
            commands::batch_queue_history::load_batch_queue_history,
            commands::batch_queue_history::save_batch_queue_history,
            commands::clear_local_api_temp_cache,
        ])
        .on_window_event(|window, event| {
            if window.label() != MAIN_WINDOW_LABEL {
                return;
            }
            if let WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .setup(move |app| {
            let app_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
            std::fs::create_dir_all(&app_dir).map_err(|e| e.to_string())?;
            let db_path = app_dir.join("allowed_origins.sqlite");
            {
                let db = AllowedOriginsDb::open(&db_path).map_err(|e| e.to_string())?;
                let mut g = origins.write().map_err(|e| e.to_string())?;
                db.merge_into_allowed_origins(&mut *g)
                    .map_err(|e| e.to_string())?;
            }
            let _ = Pkcs11PathsDb::open(&db_path).map_err(|e| e.to_string())?;
            if let Ok(mut slot) = app_db_slot.lock() {
                *slot = Some(db_path.clone());
            }

            if let Err(e) = queue_store::init_queue_tables(&db_path) {
                tracing::warn!(error = %e, "crear tablas de colas SQLite");
            }

            if let Err(e) =
                queue_store::hydrate_pending_intents_from_db(&db_path, &pending_batch_intents)
            {
                tracing::warn!(error = %e, "hidratar intents pendientes desde SQLite");
            }

            let now_secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let stale_cutoff =
                now_secs.saturating_sub(crate::ports::BATCH_JOB_MAX_WALL_CLOCK_SECS);
            if let Err(e) = queue_store::purge_batch_job_enqueue_before(&db_path, stale_cutoff) {
                tracing::warn!(error = %e, "purgar batch_job_enqueue obsoleto");
            }

            app.manage(OriginDbPath(Arc::new(db_path.clone())));

            let batch_signed_outputs: Arc<
                Mutex<HashMap<String, Vec<std::path::PathBuf>>>,
            > = Arc::new(Mutex::new(HashMap::new()));

            app.manage(BatchSignedOutputsStore(batch_signed_outputs.clone()));

            let emit_for_deep_link = app.handle().clone();
            app.handle().deep_link().on_open_url(move |event| {
                show_main_window(&emit_for_deep_link);
                let urls: Vec<String> =
                    event.urls().into_iter().map(|u| u.to_string()).collect();
                let _ = emit_for_deep_link.emit(
                    "nexosign-deep-link",
                    serde_json::json!({ "urls": urls }),
                );
            });

            #[cfg(desktop)]
            try_install_tray(app.handle());

            let handle = app.handle().clone();
            infrastructure::local_server::spawn_local_api(
                handle,
                origins.clone(),
                pkcs11.clone(),
                batch_cancel.clone(),
                pending_batch_intents.clone(),
                db_path.clone(),
                batch_signed_outputs.clone(),
            );

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}
