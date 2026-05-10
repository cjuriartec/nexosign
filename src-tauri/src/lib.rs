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
use tauri::{Emitter, Manager};
use tauri_plugin_deep_link::DeepLinkExt;
use tokio_util::sync::CancellationToken;

use crate::adapters::http::PendingBatchIntent;
use crate::adapters::http::state::PendingBatchIntents;
use crate::commands::BatchCancelRegistry;

static INIT_TRACING: Once = Once::new();

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
        ])
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

            if let Err(e) =
                queue_store::hydrate_pending_intents_from_db(&db_path, &pending_batch_intents)
            {
                tracing::warn!(error = %e, "hidratar intents pendientes desde SQLite");
            }

            app.manage(OriginDbPath(Arc::new(db_path.clone())));

            let emit_for_deep_link = app.handle().clone();
            app.handle().deep_link().on_open_url(move |event| {
                let urls: Vec<String> =
                    event.urls().into_iter().map(|u| u.to_string()).collect();
                let _ = emit_for_deep_link.emit(
                    "nexosign-deep-link",
                    serde_json::json!({ "urls": urls }),
                );
            });

            let handle = app.handle().clone();
            infrastructure::local_server::spawn_local_api(
                handle,
                origins.clone(),
                pkcs11.clone(),
                batch_cancel.clone(),
                pending_batch_intents.clone(),
                db_path.clone(),
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
