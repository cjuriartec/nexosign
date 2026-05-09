pub mod adapters;
pub mod application;
mod commands;
pub mod domain;
pub mod infrastructure;
pub mod ports;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Once, RwLock};

use adapters::persistence::AllowedOriginsDb;
use adapters::pkcs11::token::Pkcs11TokenManager;
use domain::allowed_origins::AllowedOrigins;
use infrastructure::origin_db::OriginDbPath;
use tauri::{Emitter, Manager};
use tauri_plugin_deep_link::DeepLinkExt;
use tokio_util::sync::CancellationToken;

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
    let pkcs11 = Arc::new(Pkcs11TokenManager::new());
    let batch_cancel = Arc::new(Mutex::new(HashMap::<String, CancellationToken>::new()));

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_deep_link::init())
        .manage(origins.clone())
        .manage(pkcs11.clone())
        .manage(BatchCancelRegistry(batch_cancel.clone()))
        .invoke_handler(tauri::generate_handler![
            greet,
            commands::get_local_api_base_url,
            commands::list_allowed_origins,
            commands::add_allowed_origin,
            commands::remove_allowed_origin,
            commands::cancel_batch_job,
            commands::demo_emit_progress,
            commands::probe_pkcs11_module_path,
            commands::pkcs11_diagnose_slots,
            commands::pkcs11_slot_count,
            commands::list_signing_certificates,
            commands::pkcs11_login,
            commands::pkcs11_logout,
            commands::pkcs11_session_status,
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
            app.manage(OriginDbPath(Arc::new(db_path)));

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
