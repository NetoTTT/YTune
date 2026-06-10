use tauri::{Runtime, WebviewUrl, WebviewWindowBuilder, Manager};

pub fn toggle_tray_popup<R: Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(popup) = app.get_webview_window("tray-popup") {
        if popup.is_visible().unwrap_or(false) {
            let _ = popup.hide();
            return;
        }
        let h = popup.inner_size().ok()
            .and_then(|s| app.primary_monitor().ok().flatten().map(|m| s.height as f64 / m.scale_factor()))
            .unwrap_or(205.0);
        position_popup(app, &popup, h);
        let _ = popup.show();
        let _ = popup.set_focus();
    } else {
        if let Ok(popup) = WebviewWindowBuilder::new(
            app,
            "tray-popup",
            WebviewUrl::App("tray".into()),
        )
        .title("")
        .inner_size(340.0, 205.0)
        .decorations(false)
        .transparent(true)
        .skip_taskbar(true)
        .always_on_top(true)
        .resizable(false)
        .visible(false)
        .build()
        {
            position_popup(app, &popup, 205.0);
            let _ = popup.show();
            let _ = popup.set_focus();
        }
    }
}

pub fn position_popup<R: Runtime>(app: &tauri::AppHandle<R>, popup: &tauri::WebviewWindow<R>, height: f64) {
    if let Ok(Some(monitor)) = app.primary_monitor() {
        let scale   = monitor.scale_factor();
        let size    = monitor.size();
        let w       = 340.0_f64;
        let margin  = 12.0_f64;
        let taskbar = 48.0_f64;
        let x = (size.width  as f64 / scale) - w - margin;
        let y = (size.height as f64 / scale) - height - margin - taskbar;
        let _ = popup.set_position(tauri::Position::Logical(tauri::LogicalPosition { x, y }));
    }
}
