use tauri::{Runtime, WebviewUrl, WebviewWindowBuilder, Manager};

/// Finds the monitor whose bounds contain the given physical point.
/// Falls back to the closest monitor if none contains the point.
pub fn monitor_at<R: Runtime>(app: &tauri::AppHandle<R>, px: i32, py: i32) -> Option<tauri::Monitor> {
    let monitors = app.available_monitors().ok()?;
    monitors.iter()
        .find(|m| {
            let p = m.position();
            let s = m.size();
            px >= p.x && px < p.x + s.width  as i32 &&
            py >= p.y && py < p.y + s.height as i32
        })
        .or_else(|| {
            monitors.iter().min_by_key(|m| {
                let p = m.position();
                let s = m.size();
                let mx = p.x + s.width  as i32 / 2;
                let my = p.y + s.height as i32 / 2;
                (px - mx).abs() + (py - my).abs()
            })
        })
        .cloned()
}

fn popup_pos_path<R: Runtime>(app: &tauri::AppHandle<R>) -> Option<std::path::PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("popup_pos"))
}

fn discord_pref_path<R: Runtime>(app: &tauri::AppHandle<R>) -> Option<std::path::PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("discord_enabled"))
}

pub fn discord_enabled_get<R: Runtime>(app: &tauri::AppHandle<R>) -> bool {
    discord_pref_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|s| s.trim() != "false")
        .unwrap_or(true)
}

pub fn discord_enabled_set<R: Runtime>(app: &tauri::AppHandle<R>, enabled: bool) {
    if let Some(path) = discord_pref_path(app) {
        if let Some(parent) = path.parent() { let _ = std::fs::create_dir_all(parent); }
        let _ = std::fs::write(path, if enabled { "true" } else { "false" });
    }
}

fn discord_song_link_path<R: Runtime>(app: &tauri::AppHandle<R>) -> Option<std::path::PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("discord_song_link"))
}

pub fn discord_song_link_get<R: Runtime>(app: &tauri::AppHandle<R>) -> bool {
    discord_song_link_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|s| s.trim() == "true")
        .unwrap_or(false)
}

pub fn discord_song_link_set<R: Runtime>(app: &tauri::AppHandle<R>, enabled: bool) {
    if let Some(path) = discord_song_link_path(app) {
        if let Some(parent) = path.parent() { let _ = std::fs::create_dir_all(parent); }
        let _ = std::fs::write(path, if enabled { "true" } else { "false" });
    }
}

fn save_popup_pos<R: Runtime>(app: &tauri::AppHandle<R>, popup: &tauri::WebviewWindow<R>) {
    let Ok(pos) = popup.outer_position() else { return };
    // Save physical pixels — unambiguous across monitors and DPI settings
    if let Some(path) = popup_pos_path(app) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, format!("{},{}", pos.x, pos.y));
    }
}

fn load_popup_pos<R: Runtime>(app: &tauri::AppHandle<R>) -> Option<(i32, i32)> {
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
        let _ = popup.show();
        let _ = popup.set_focus();
    } else {
        if let Ok(popup) = WebviewWindowBuilder::new(
            app,
            "tray-popup",
            WebviewUrl::App("tray".into()),
        )
        .title("")
        .inner_size(330.0, 225.0)
        .decorations(false)
        .transparent(true)
        .skip_taskbar(true)
        .always_on_top(true)
        .resizable(true)
        .min_inner_size(275.0, 225.0)
        .max_inner_size(800.0, 600.0)
        .visible(false)
        .build()
        {
            if let Some((x, y)) = load_popup_pos(app) {
                let _ = popup.set_position(tauri::Position::Physical(tauri::PhysicalPosition { x, y }));
            } else {
                position_popup(app, &popup, 225.0);
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
