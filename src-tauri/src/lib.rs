mod commands;
mod discord;
mod inject;
mod tray;

use std::sync::Mutex;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Listener, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent,
};
use tauri_plugin_deep_link::DeepLinkExt;

use discord::{DiscordState, DiscordTrackState};
use discord_rich_presence::DiscordIpc;
use inject::INJECT_JS;

pub struct AuthTokenState(pub Mutex<Option<String>>);

fn parse_auth_token(url_str: &str) -> Option<String> {
    if !url_str.starts_with("ytune://auth/callback") {
        return None;
    }
    url_str.split_once('?')
        .and_then(|(_, q)| q.split('&')
            .find(|p| p.starts_with("token="))
            .map(|p| p.trim_start_matches("token=").to_string())
        )
}

fn percent_decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(hex) = std::str::from_utf8(&b[i+1..i+3]) {
                if let Ok(byte) = u8::from_str_radix(hex, 16) {
                    out.push(byte as char);
                    i += 3;
                    continue;
                }
            }
        } else if b[i] == b'+' {
            out.push(' ');
        } else {
            out.push(b[i] as char);
        }
        i += 1;
    }
    out
}

fn parse_navigate_url(url_str: &str) -> Option<String> {
    if !url_str.starts_with("ytune://open") { return None; }
    let query = url_str.split_once('?').map(|(_, q)| q)?;
    let target = query.split('&')
        .find(|p| p.starts_with("url="))
        .map(|p| percent_decode(p.trim_start_matches("url=")))?;
    if target.contains("music.youtube.com") { Some(target) } else { None }
}

fn apply_navigate_url(app: &tauri::AppHandle, url: String) {
    if let Some(main) = app.get_webview_window("main") {
        if let Ok(parsed) = tauri::Url::parse(&url) {
            let _ = app.emit("ytune-navigating", ());
            let _ = main.navigate(parsed);
        }
    }
}

fn apply_auth_token(app: &tauri::AppHandle, token: String) {
    if let Some(state) = app.try_state::<AuthTokenState>() {
        *state.0.lock().unwrap() = Some(token.clone());
    }
    if let Some(w) = app.get_webview_window("main") {
        let token_js = token.replace('\\', "\\\\").replace('"', "\\\"");
        let _ = w.eval(&format!(
            "window.__ytune__?.setAuthToken?.(\"{}\")",
            token_js
        ));
        let _ = w.show();
        let _ = w.set_focus();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            // Always show/focus the main window when a second instance is blocked
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.set_focus();
            }
            // On Windows, deep links arrive as args to the blocked second instance.
            // single_instance forwards them here — deep_link's on_open_url won't fire.
            for arg in &args {
                if let Some(token) = parse_auth_token(arg) {
                    apply_auth_token(app, token);
                    break;
                }
                if let Some(url) = parse_navigate_url(arg) {
                    apply_navigate_url(app, url);
                    break;
                }
            }
        }))
        .manage(DiscordState(Mutex::new(None)))
        .manage(DiscordTrackState(Mutex::new((String::new(), 0, 0, false, String::new(), false))))
        .manage(AuthTokenState(Mutex::new(None)))
        .setup(|app| {
            // Using events (plugin:event|emit) instead of commands because Tauri 2 only allows
            // plugin commands from remote origins — user commands require a plugin + permission file.
            let _ = app.deep_link().register_all();

            // on_open_url fires when the app itself is launched with a deep link (first instance,
            // or some platforms). On Windows with single_instance, see the callback above instead.
            let handle_auth = app.handle().clone();
            app.deep_link().on_open_url(move |event| {
                for url in event.urls() {
                    let s = url.to_string();
                    if let Some(token) = parse_auth_token(&s) {
                        apply_auth_token(&handle_auth, token);
                        break;
                    }
                    if let Some(nav) = parse_navigate_url(&s) {
                        apply_navigate_url(&handle_auth, nav);
                        break;
                    }
                }
            });

            let handle = app.handle().clone();
            app.listen("ytune-state", move |event| {
                discord::handle_player_state(&handle, event.payload());
            });

            let handle_opener = app.handle().clone();
            app.listen("ytune-open-url", move |event| {
                if let Ok(url) = serde_json::from_str::<String>(event.payload()) {
                    use tauri_plugin_opener::OpenerExt;
                    let _ = handle_opener.opener().open_url(url, None::<&str>);
                }
            });

            let handle_popup = app.handle().clone();
            app.listen("ytune-toggle-popup", move |_| {
                tray::toggle_tray_popup(&handle_popup);
            });

            // Update available: show badge+item in YTM modal and forward install-update to popup
            let handle_upd = app.handle().clone();
            app.listen("ytune-update-available", move |event| {
                let version = serde_json::from_str::<serde_json::Value>(event.payload())
                    .ok()
                    .and_then(|v| v["version"].as_str().map(|s| s.to_string()))
                    .unwrap_or_default();
                if let Some(main) = handle_upd.get_webview_window("main") {
                    let safe = version.replace('\'', "\\'");
                    let _ = main.eval(&format!("window.__ytune__?.showUpdateNotif?.('{safe}')"));
                }
            });
            let handle_install = app.handle().clone();
            app.listen("ytune-install-update", move |_| {
                let _ = handle_install.emit("ytune-install-update", ());
            });

            let handle_restore = app.handle().clone();
            app.listen("ytune-auth-restore", move |event| {
                let token = serde_json::from_str::<String>(event.payload())
                    .unwrap_or_else(|_| event.payload().trim_matches('"').to_string());
                if token.is_empty() { return; }
                if let Some(state) = handle_restore.try_state::<AuthTokenState>() {
                    *state.0.lock().unwrap() = Some(token.clone());
                }
                // Notify popup so it can connect WS with the restored token
                let _ = handle_restore.emit("ytune-auth-ready", token);
            });

            let handle_room = app.handle().clone();
            app.listen("ytune-room-status", move |event| {
                if let Some(main) = handle_room.get_webview_window("main") {
                    let payload = event.payload();
                    let _ = main.eval(&format!(
                        "window.__ytune__?.setRoomStatus?.({payload})"
                    ));
                }
            });

            let handle_state_req = app.handle().clone();
            app.listen("ytune-discord-state-request", move |_| {
                let enabled = tray::discord_enabled_get(&handle_state_req);
                if let Some(main) = handle_state_req.get_webview_window("main") {
                    let _ = main.eval(&format!("window.__ytune__?.setDiscordState?.({})", enabled));
                }
            });

            let handle_discord = app.handle().clone();
            app.listen("ytune-discord-toggle", move |_| {
                let new_val = !tray::discord_enabled_get(&handle_discord);
                tray::discord_enabled_set(&handle_discord, new_val);
                if !new_val {
                    if let Some(d) = handle_discord.try_state::<discord::DiscordState>() {
                        let mut g = d.0.lock().unwrap();
                        if let Some(c) = g.as_mut() { let _ = c.clear_activity(); }
                    }
                }
                if let Some(main) = handle_discord.get_webview_window("main") {
                    let _ = main.eval(&format!("window.__ytune__?.setDiscordState?.({})", new_val));
                }
                let _ = handle_discord.emit("ytune-discord-state", new_val);
            });

            let handle2 = app.handle().clone();
            app.listen("ytune-viz", move |event| {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(event.payload()) {
                    let _ = handle2.emit("player-viz", v);
                }
            });

            WebviewWindowBuilder::new(
                app,
                "main",
                WebviewUrl::External("https://music.youtube.com".parse().unwrap()),
            )
            .title("ytune")
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36")
            .inner_size(1200.0, 800.0)
            .min_inner_size(400.0, 400.0)
            .on_navigation(|url| {
                println!("[ytune] navigating to: {}", url);
                true
            })
            .initialization_script(INJECT_JS)
            .build()?;

            #[cfg(debug_assertions)]
            {
                let h = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(4));
                    if let Some(w) = h.get_webview_window("main") {
                        let _ = w.show();
                        let _ = w.set_focus();
                    }
                });
            }

            discord::start_discord_thread(app.handle().clone());

            let widget = MenuItem::with_id(app, "widget", "Open widget", true, None::<&str>)?;
            let show   = MenuItem::with_id(app, "show",   "Show ytune",  true, None::<&str>)?;
            let quit   = MenuItem::with_id(app, "quit",   "Quit",        true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&widget, &show, &quit])?;

            let tray_icon = tauri::image::Image::from_bytes(
                include_bytes!("../icons/tray-icon.png")
            ).unwrap_or_else(|_| app.default_window_icon().unwrap().clone());

            TrayIconBuilder::new()
                .icon(tray_icon)
                .tooltip("ytune")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "widget" => tray::toggle_tray_popup(app),
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
                        tray::toggle_tray_popup(tray.app_handle());
                    }
                })
                .build(app)?;

            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    let _ = window.hide();
                    api.prevent_close();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::show_main_window,
            commands::hide_tray_popup,
            commands::resize_popup,
            commands::player_control,
            commands::player_seek,
            commands::player_volume,
            commands::discord_get,
            commands::discord_set,
            commands::get_auth_token,
            commands::set_popup_size,
            commands::navigate_ytm,
            commands::read_clipboard,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
