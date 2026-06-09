use std::sync::Mutex;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent,
};
use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};

// Replace with your own Discord application client ID from https://discord.com/developers/applications
const DISCORD_CLIENT_ID: &str = "YOUR_CLIENT_ID_HERE";

// Injected into music.youtube.com to extract player state and expose controls
const INJECT_JS: &str = r#"
(function() {
    if (window.__ytune_injected) return;
    window.__ytune_injected = true;

    window.__ytune__ = {
        playPause: () => document.querySelector('tp-yt-paper-icon-button.play-pause-button')?.click(),
        next:      () => document.querySelector('tp-yt-paper-icon-button.next-button')?.click(),
        previous:  () => document.querySelector('tp-yt-paper-icon-button.previous-button')?.click(),
        seek:      (t) => { const v = document.querySelector('video'); if (v) v.currentTime = t; },
    };

    function getQueue() {
        const items = document.querySelectorAll('ytmusic-player-queue-item');
        if (items.length < 2) return [];
        const all = Array.from(items).map(el => ({
            title:   el.querySelector('.song-title')?.textContent?.trim() || '',
            artist:  el.querySelector('.byline')?.textContent?.trim() || '',
            current: el.hasAttribute('selected'),
        }));
        const ci = all.findIndex(i => i.current);
        if (ci === -1) return [];
        // Send 2 before + current + 3 after
        return all.slice(Math.max(0, ci - 2), Math.min(all.length, ci + 4));
    }

    function getState() {
        const video = document.querySelector('video');
        return {
            title:       document.querySelector('.ytmusic-player-bar .title')?.textContent?.trim() || '',
            artist:      document.querySelector('.ytmusic-player-bar .byline')?.textContent?.trim() || '',
            playing:     document.querySelector('tp-yt-paper-icon-button.play-pause-button')
                             ?.getAttribute('aria-label') === 'Pause',
            currentTime: video?.currentTime || 0,
            duration:    video?.duration    || 0,
            queue:       getQueue(),
        };
    }

    setInterval(() => {
        const state = getState();
        if (!state.title) return;
        window.__TAURI_INTERNALS__?.invoke('update_player_state', state).catch(() => {});
    }, 1000);
})();
"#;

struct DiscordState(Mutex<Option<DiscordIpcClient>>);

#[tauri::command]
fn show_main_window(app: tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

#[tauri::command]
fn hide_tray_popup(app: tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("tray-popup") {
        let _ = w.hide();
    }
}

// Called from tray popup buttons; executes the matching control in the YTM webview
#[tauri::command]
fn player_control(app: tauri::AppHandle, action: String) {
    if let Some(main) = app.get_webview_window("main") {
        let js = match action.as_str() {
            "play_pause" => "window.__ytune__?.playPause()",
            "next"       => "window.__ytune__?.next()",
            "previous"   => "window.__ytune__?.previous()",
            _ => return,
        };
        let _ = main.eval(js);
    }
}

// Seeks the YTM player to a given position in seconds
#[tauri::command]
fn player_seek(app: tauri::AppHandle, position: f64) {
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.eval(&format!("window.__ytune__?.seek({})", position));
    }
}

// Called by the injected JS every second with full player state
#[tauri::command]
fn update_player_state(
    app: tauri::AppHandle,
    discord: tauri::State<DiscordState>,
    title: String,
    artist: String,
    playing: bool,
    current_time: f64,
    duration: f64,
    queue: Vec<serde_json::Value>,
) {
    // Forward state to the tray popup
    let _ = app.emit("player_state_changed", serde_json::json!({
        "title": title,
        "artist": artist,
        "playing": playing,
        "currentTime": current_time,
        "duration": duration,
        "queue": queue,
    }));

    // Update Discord Rich Presence
    let mut guard = discord.0.lock().unwrap();
    if let Some(client) = guard.as_mut() {
        let status = if playing { "Playing" } else { "Paused" };
        let result = client.set_activity(
            activity::Activity::new()
                .details(&title)
                .state(&format!("{} — {}", artist, status))
                .assets(activity::Assets::new().large_image("ytmusic")),
        );
        if result.is_err() {
            *guard = None;
        }
    }
}

fn toggle_tray_popup(app: &tauri::AppHandle) {
    if let Some(popup) = app.get_webview_window("tray-popup") {
        if popup.is_visible().unwrap_or(false) {
            let _ = popup.hide();
            return;
        }
        position_popup(app, &popup);
        let _ = popup.show();
        let _ = popup.set_focus();
    } else {
        // Lazy-create the popup window the first time it's needed
        if let Ok(popup) = WebviewWindowBuilder::new(
            app,
            "tray-popup",
            WebviewUrl::App("tray".into()),
        )
        .title("")
        .inner_size(340.0, 170.0)
        .decorations(false)
        .skip_taskbar(true)
        .always_on_top(true)
        .resizable(false)
        .visible(false)
        .build()
        {
            position_popup(app, &popup);
            let _ = popup.show();
            let _ = popup.set_focus();
        }
    }
}

// Positions the popup at the bottom-right of the primary monitor, above the taskbar
fn position_popup(app: &tauri::AppHandle, popup: &tauri::WebviewWindow) {
    if let Ok(Some(monitor)) = app.primary_monitor() {
        let scale = monitor.scale_factor();
        let size  = monitor.size();
        let w      = 340.0_f64;
        let h      = 170.0_f64;
        let margin = 12.0_f64;
        let taskbar = 48.0_f64; // approximate taskbar height
        let x = (size.width as f64 / scale) - w - margin;
        let y = (size.height as f64 / scale) - h - margin - taskbar;
        let _ = popup.set_position(tauri::Position::Logical(tauri::LogicalPosition { x, y }));
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .manage(DiscordState(Mutex::new(None)))
        .setup(|app| {
            // Try to connect Discord in a background thread so startup isn't blocked
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let mut client = DiscordIpcClient::new(DISCORD_CLIENT_ID);
                if client.connect().is_ok() {
                    if let Some(state) = handle.try_state::<DiscordState>() {
                        *state.0.lock().unwrap() = Some(client);
                    }
                }
            });

            let show = MenuItem::with_id(app, "show", "Show ytune", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit",  "Quit",       true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;

            let tray_icon = tauri::image::Image::from_bytes(
                include_bytes!("../icons/tray-icon.png")
            ).unwrap_or_else(|_| app.default_window_icon().unwrap().clone());

            TrayIconBuilder::new()
                .icon(tray_icon)
                .tooltip("ytune")
                .menu(&menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                    "quit" => std::process::exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        toggle_tray_popup(tray.app_handle());
                    }
                })
                .build(app)?;

            Ok(())
        })
        .on_page_load(|webview, _payload| {
            // Re-inject controls script every time the YTM page loads/reloads
            if webview.label() == "main" {
                let _ = webview.eval(INJECT_JS);
            }
        })
        .on_window_event(|window, event| {
            // Hide to tray instead of quitting when the user closes the main window
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    let _ = window.hide();
                    api.prevent_close();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            show_main_window,
            hide_tray_popup,
            player_control,
            player_seek,
            update_player_state,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
