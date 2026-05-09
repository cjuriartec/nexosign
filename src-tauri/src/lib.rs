pub mod adapters;
pub mod application;
mod commands;
pub mod domain;
pub mod infrastructure;
pub mod ports;

use std::sync::{Arc, RwLock};
use std::sync::Once;

use adapters::pkcs11::token::Pkcs11TokenManager;
use domain::allowed_origins::AllowedOrigins;

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

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(origins.clone())
        .manage(pkcs11.clone())
        .invoke_handler(tauri::generate_handler![
            greet,
            commands::get_local_api_base_url,
            commands::list_allowed_origins,
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
            let handle = app.handle().clone();
            infrastructure::local_server::spawn_local_api(handle, origins.clone(), pkcs11.clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}
