use tauri::{Runtime, Manager};
use crate::tray::position_popup;

#[tauri::command]
pub fn show_main_window<R: Runtime>(app: tauri::AppHandle<R>) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

#[tauri::command]
pub fn hide_tray_popup<R: Runtime>(app: tauri::AppHandle<R>) {
    if let Some(w) = app.get_webview_window("tray-popup") {
        let _ = w.hide();
    }
}

#[tauri::command]
pub fn player_control<R: Runtime>(app: tauri::AppHandle<R>, action: String) {
    if let Some(main) = app.get_webview_window("main") {
        if let Some(idx_str) = action.strip_prefix("queue_jump_") {
            if let Ok(idx) = idx_str.parse::<usize>() {
                let _ = main.eval(&format!("window.__ytune__?.queueJump({})", idx));
            }
            return;
        }
        let js = match action.as_str() {
            "play_pause" => "window.__ytune__?.playPause()",
            "next"       => "window.__ytune__?.next()",
            "previous"   => "window.__ytune__?.previous()",
            "like"       => "window.__ytune__?.like()",
            "dislike"    => "window.__ytune__?.dislike()",
            _ => return,
        };
        let _ = main.eval(js);
    }
}

#[tauri::command]
pub fn resize_popup<R: Runtime>(app: tauri::AppHandle<R>, height: f64) {
    if let Some(popup) = app.get_webview_window("tray-popup") {
        let _ = popup.set_size(tauri::Size::Logical(tauri::LogicalSize { width: 340.0, height }));
        position_popup(&app, &popup, height);
    }
}

#[tauri::command]
pub fn player_seek<R: Runtime>(app: tauri::AppHandle<R>, position: f64) {
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.eval(&format!("window.__ytune__?.seek({})", position));
    }
}

#[tauri::command]
pub fn player_volume<R: Runtime>(app: tauri::AppHandle<R>, volume: f64) {
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.eval(&format!("window.__ytune__?.setVolume({})", volume));
    }
}
