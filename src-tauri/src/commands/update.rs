use serde::Serialize;
use tauri::AppHandle;

use crate::infrastructure::updater;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppVersionInfo {
    pub current: String,
}

#[tauri::command]
pub fn get_app_version(app: AppHandle) -> AppVersionInfo {
    AppVersionInfo {
        current: updater::current_version(&app),
    }
}

#[tauri::command]
pub async fn check_for_app_updates(app: AppHandle) -> Result<(), String> {
    updater::run_update_flow(app, true).await
}
