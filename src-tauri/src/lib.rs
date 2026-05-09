pub mod adapters;
mod commands;
pub mod domain;
pub mod infrastructure;

use std::sync::{Arc, RwLock};
use std::sync::Once;

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

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(origins.clone())
        .invoke_handler(tauri::generate_handler![
            greet,
            commands::get_local_api_base_url,
            commands::list_allowed_origins,
            commands::demo_emit_progress,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            infrastructure::local_server::spawn_local_api(handle, origins.clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}
