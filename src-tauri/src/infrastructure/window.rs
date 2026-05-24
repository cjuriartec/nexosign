use tauri::{AppHandle, Manager};

/// Etiqueta de la ventana principal ([`tauri.conf.json`](../../tauri.conf.json)).
pub const MAIN_WINDOW_LABEL: &str = "main";

pub fn show_main_window<R: tauri::Runtime>(app: &AppHandle<R>) {
    if let Some(w) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// Ventana visible y con foco: el usuario ya está interactuando con NexoSign.
pub fn is_main_window_in_foreground<R: tauri::Runtime>(app: &AppHandle<R>) -> bool {
    app.get_webview_window(MAIN_WINDOW_LABEL)
        .map(|w| w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false))
        .unwrap_or(false)
}

pub fn show_main_window_if_background<R: tauri::Runtime>(app: &AppHandle<R>) {
    if !is_main_window_in_foreground(app) {
        show_main_window(app);
    }
}
