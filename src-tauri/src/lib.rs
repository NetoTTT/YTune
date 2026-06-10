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

use discord::{DiscordState, DiscordTrackState};
use discord_rich_presence::DiscordIpc;
use inject::INJECT_JS;

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
        .manage(DiscordTrackState(Mutex::new((String::new(), 0, 0, false, String::new(), false))))
        .setup(|app| {
            // Listen for player state events emitted by the injected JS in the YTM webview.
            // Using events (plugin:event|emit) instead of commands because Tauri 2 only allows
            // plugin commands from remote origins — user commands require a plugin + permission file.
            let handle = app.handle().clone();
            app.listen("ytune-state", move |event| {
                discord::handle_player_state(&handle, event.payload());
            });

            // Relay raw FFT data from the YTM webview to the popup window.
            // event.payload() is already a JSON string (e.g. "[0,12,45,...]") — deserialize
            // it back to a Value so the JS side receives an actual array, not a double-encoded string.
            let handle_popup = app.handle().clone();
            app.listen("ytune-toggle-popup", move |_| {
                tray::toggle_tray_popup(&handle_popup);
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
                // eval into YTM for reliable delivery; emit for the popup to sync
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

            // Open DevTools after 4s so YTM has finished loading
            #[cfg(debug_assertions)]
            {
                let h = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(4));
                    if let Some(w) = h.get_webview_window("main") {
                        w.open_devtools();
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
