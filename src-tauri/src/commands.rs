use tauri::{Emitter, Runtime, Manager};
use discord_rich_presence::DiscordIpc;
use crate::tray::{monitor_at, discord_enabled_get, discord_enabled_set};

#[tauri::command]
pub fn discord_get<R: Runtime>(app: tauri::AppHandle<R>) -> bool {
    discord_enabled_get(&app)
}

#[tauri::command]
pub fn discord_set<R: Runtime>(app: tauri::AppHandle<R>, enabled: bool) {
    discord_enabled_set(&app, enabled);
    if !enabled {
        if let Some(discord) = app.try_state::<crate::discord::DiscordState>() {
            let mut guard = discord.0.lock().unwrap();
            if let Some(client) = guard.as_mut() {
                let _ = client.clear_activity();
            }
        }
    }
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.eval(&format!("window.__ytune__?.setDiscordState?.({})", enabled));
    }
    let _ = app.emit("ytune-discord-state", enabled);
}

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
    let Some(popup) = app.get_webview_window("tray-popup") else { return };
    let Ok(cur_size) = popup.inner_size() else { return };
    let Ok(pos)      = popup.outer_position() else { return };

    // Find the monitor the popup is actually on using physical coords
    let cx = pos.x + cur_size.width  as i32 / 2;
    let cy = pos.y + cur_size.height as i32 / 2;
    let Some(monitor) = monitor_at(&app, cx, cy) else { return };

    let scale    = monitor.scale_factor();
    let margin   = (8.0 * scale) as i32;
    let w_phys   = (340.0 * scale).round() as i32;
    let start_h  = cur_size.height as i32;
    let target_h = (height * scale).round() as i32;
    let bottom   = pos.y + start_h;
    let mp_x     = monitor.position().x;
    let mp_y     = monitor.position().y;
    let ms_w     = monitor.size().width as i32;
    let ms_h     = monitor.size().height as i32;
    let target_x = pos.x.clamp(mp_x + margin, mp_x + ms_w - w_phys - margin);

    std::thread::spawn(move || {
        const STEPS: u32 = 12;
        const STEP_MS: u64 = 12;
        for i in 1..=STEPS {
            let t     = i as f64 / STEPS as f64;
            let eased = 1.0 - (1.0 - t).powi(3);
            let h     = (start_h as f64 + (target_h - start_h) as f64 * eased).round() as i32;
            let y     = (bottom - h).clamp(mp_y + margin, mp_y + ms_h - h - margin);
            let _ = popup.set_size(tauri::Size::Physical(tauri::PhysicalSize {
                width: w_phys as u32, height: h as u32,
            }));
            let _ = popup.set_position(tauri::Position::Physical(tauri::PhysicalPosition {
                x: target_x, y,
            }));
            std::thread::sleep(std::time::Duration::from_millis(STEP_MS));
        }
    });
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
