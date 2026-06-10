use tauri::{Runtime, WebviewUrl, WebviewWindowBuilder, Manager};

fn popup_pos_path<R: Runtime>(app: &tauri::AppHandle<R>) -> Option<std::path::PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("popup_pos"))
}

fn save_popup_pos<R: Runtime>(app: &tauri::AppHandle<R>, popup: &tauri::WebviewWindow<R>) {
    let Ok(pos) = popup.outer_position() else { return };
    let Ok(Some(monitor)) = app.primary_monitor() else { return };
    let scale = monitor.scale_factor();
    let x = pos.x as f64 / scale;
    let y = pos.y as f64 / scale;
    if let Some(path) = popup_pos_path(app) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, format!("{},{}", x, y));
    }
}

fn load_popup_pos<R: Runtime>(app: &tauri::AppHandle<R>) -> Option<(f64, f64)> {
    let path = popup_pos_path(app)?;
    let content = std::fs::read_to_string(path).ok()?;
    let (xs, ys) = content.split_once(',')?;
    Some((xs.trim().parse().ok()?, ys.trim().parse().ok()?))
}

pub fn toggle_tray_popup<R: Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(popup) = app.get_webview_window("tray-popup") {
        if popup.is_visible().unwrap_or(false) {
            save_popup_pos(app, &popup);
            let _ = popup.hide();
            return;
        }
        // Existing hidden window — show at wherever it already is (user moved it this session)
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
            // Restore last saved position; fall back to default bottom-right
            if let Some((x, y)) = load_popup_pos(app) {
                let _ = popup.set_position(tauri::Position::Logical(tauri::LogicalPosition { x, y }));
            } else {
                position_popup(app, &popup, 205.0);
            }
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
