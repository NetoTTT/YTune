use std::sync::Mutex;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Listener, Manager, Runtime, WebviewUrl, WebviewWindowBuilder, WindowEvent,
};
use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};

const DISCORD_CLIENT_ID: &str = "YOUR_CLIENT_ID_HERE";

const INJECT_JS: &str = r#"
(function() {
    // initialization_script runs in every frame — skip anything that isn't the main YTM page
    if (location.hostname !== 'music.youtube.com') return;
    if (window.__ytune_injected) return;
    window.__ytune_injected = true;

    console.log('[ytune] script active, __TAURI_INTERNALS__:', typeof window.__TAURI_INTERNALS__ !== 'undefined');

    window.__ytune__ = {
        playPause: () => document.querySelector('#play-pause-button button')?.click(),
        next:      () => document.querySelector('.next-button.ytmusic-player-bar button')?.click(),
        previous:  () => document.querySelector('.previous-button.ytmusic-player-bar button')?.click(),
        seek:      (t) => { const v = document.querySelector('video'); if (v) v.currentTime = t; },
        like:      () => document.querySelector('ytmusic-like-button-renderer .like button')?.click(),
        dislike:   () => document.querySelector('ytmusic-like-button-renderer .dislike button')?.click(),
        setVolume: (v) => {
            // YTM stores its own volume state; setting video.volume directly gets overridden.
            // Use the player API so YTM updates its internal state too.
            const player = document.querySelector('ytmusic-player');
            if (player?.playerApi?.setVolume) { player.playerApi.setVolume(v); return; }
            const vid = getActiveVideo();
            if (vid) { vid.volume = Math.max(0, Math.min(1, v / 100)); vid.muted = (v === 0); }
        },
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
        return all.slice(Math.max(0, ci - 2), Math.min(all.length, ci + 4));
    }

    // Parse "m:ss" or "mm:ss" text to seconds
    function parseMmSs(text) {
        if (!text) return 0;
        const m = String(text).trim().match(/^(\d+):(\d{2})$/);
        return m ? +m[1] * 60 + +m[2] : 0;
    }

    // YTM uses a continuous DASH stream — video.currentTime and video.duration reflect
    // the position within the entire playlist stream, not the current song.
    // The player bar's time display always shows the correct per-song values.
    function getDisplayedTimes() {
        const el = document.querySelector('.time-info.ytmusic-player-bar');
        if (el) {
            const parts = el.textContent.split('/');
            if (parts.length === 2) {
                const cur = parseMmSs(parts[0]);
                const dur = parseMmSs(parts[1]);
                if (dur > 0) return { cur, dur };
            }
        }
        // Fallback: separate elements
        const curEl = document.querySelector('.current-time.ytmusic-player-bar');
        const durEl = document.querySelector('.duration.ytmusic-player-bar');
        const cur = parseMmSs(curEl?.textContent);
        const dur = parseMmSs(durEl?.textContent);
        return { cur, dur };
    }

    function getActiveVideo() {
        const all = Array.from(document.querySelectorAll('video'));
        // During song transitions YTM may have two video elements — old (paused at end)
        // and new (playing). Always prefer the one that is actually playing.
        return all.find(v => !v.paused && v.readyState >= 2 && v.duration > 0)
            || all.find(v => v.readyState >= 2 && v.duration > 0)
            || all[0]
            || null;
    }

    function getState() {
        const video = getActiveVideo();
        const times = getDisplayedTimes();
        return {
            title:       document.querySelector('.title.ytmusic-player-bar')?.title
                      || document.querySelector('.title.ytmusic-player-bar')?.textContent?.trim()
                      || '',
            artist:      document.querySelector('.subtitle.ytmusic-player-bar')?.textContent?.trim()
                      || '',
            thumbnail:   document.querySelector('ytmusic-player-bar ytmusic-thumbnail img')?.src
                      || document.querySelector('ytmusic-player-bar img[src*="googleusercontent"]')?.src
                      || '',
            liked:       document.querySelector('ytmusic-like-button-renderer .like button')
                           ?.getAttribute('aria-pressed') === 'true',
            disliked:    document.querySelector('ytmusic-like-button-renderer .dislike button')
                           ?.getAttribute('aria-pressed') === 'true',
            playing:     video ? !video.paused : false,
            volume:      (function() {
                             const p = document.querySelector('ytmusic-player');
                             if (p?.playerApi?.getVolume) return p.playerApi.getVolume();
                             return video ? (video.muted ? 0 : Math.round(video.volume * 100)) : 100;
                         })(),
            currentTime: times.cur || 0,
            duration:    times.dur || 0,
            queue:       getQueue(),
        };
    }

    let pollCount = 0;
    function startPolling() {
        console.log('[ytune] polling started');
        setInterval(() => {
            const state = getState();
            pollCount++;
            if (pollCount <= 3) {
                const t = getDisplayedTimes();
                const tEl = document.querySelector('.time-info.ytmusic-player-bar');
                console.log('[ytune] probe: .time-info="' + tEl?.textContent?.trim() +
                    '" → cur=' + t.cur + ' dur=' + t.dur +
                    ' video.currentTime=' + getActiveVideo()?.currentTime?.toFixed(1));
            }
            if (pollCount % 10 === 1) {
                console.log('[ytune] poll #' + pollCount + ' title="' + state.title + '" playing=' + state.playing);
            }
            if (!state.title) return;
            // Use Tauri's event system (allowed by core:event:allow-emit in core:default)
            // instead of a command (commands from remote origins require a plugin + permission file)
            window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {
                event: 'ytune-state',
                payload: state,
            }).then(() => { if (pollCount <= 3) console.log('[ytune] emit OK'); })
              .catch(e => { if (pollCount % 10 === 1) console.error('[ytune] emit error:', e); });
        }, 1000);
    }

    function waitForPlayerBar() {
        if (document.querySelector('ytmusic-player-bar')) {
            console.log('[ytune] player-bar found immediately');
            startPolling();
            return;
        }
        console.log('[ytune] waiting for ytmusic-player-bar...');
        const obs = new MutationObserver(() => {
            if (document.querySelector('ytmusic-player-bar')) {
                console.log('[ytune] player-bar appeared');
                obs.disconnect();
                startPolling();
            }
        });
        obs.observe(document.documentElement, { childList: true, subtree: true });
        setTimeout(() => { obs.disconnect(); startPolling(); }, 10000);
    }

    // initialization_script runs before the DOM exists — wait for DOMContentLoaded
    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', waitForPlayerBar);
    } else {
        waitForPlayerBar();
    }
})();
"#;

pub struct DiscordState(pub Mutex<Option<DiscordIpcClient>>);

#[tauri::command]
fn show_main_window<R: Runtime>(app: tauri::AppHandle<R>) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

#[tauri::command]
fn hide_tray_popup<R: Runtime>(app: tauri::AppHandle<R>) {
    if let Some(w) = app.get_webview_window("tray-popup") {
        let _ = w.hide();
    }
}

#[tauri::command]
fn player_control<R: Runtime>(app: tauri::AppHandle<R>, action: String) {
    if let Some(main) = app.get_webview_window("main") {
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
fn resize_popup<R: Runtime>(app: tauri::AppHandle<R>, height: f64) {
    if let Some(popup) = app.get_webview_window("tray-popup") {
        let _ = popup.set_size(tauri::Size::Logical(tauri::LogicalSize { width: 340.0, height }));
        position_popup(&app, &popup, height);
    }
}

#[tauri::command]
fn player_seek<R: Runtime>(app: tauri::AppHandle<R>, position: f64) {
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.eval(&format!("window.__ytune__?.seek({})", position));
    }
}

#[tauri::command]
fn player_volume<R: Runtime>(app: tauri::AppHandle<R>, volume: f64) {
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.eval(&format!("window.__ytune__?.setVolume({})", volume));
    }
}

fn toggle_tray_popup<R: Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(popup) = app.get_webview_window("tray-popup") {
        if popup.is_visible().unwrap_or(false) {
            let _ = popup.hide();
            return;
        }
        // Use the popup's current logical height for positioning
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

fn position_popup<R: Runtime>(app: &tauri::AppHandle<R>, popup: &tauri::WebviewWindow<R>, height: f64) {
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

fn handle_player_state(app: &tauri::AppHandle, payload: &str) {
    let Ok(state) = serde_json::from_str::<serde_json::Value>(payload) else { return };

    let title        = state["title"].as_str().unwrap_or("").to_string();
    let artist       = state["artist"].as_str().unwrap_or("").to_string();
    let playing      = state["playing"].as_bool().unwrap_or(false);
    let current_time = state["currentTime"].as_f64().unwrap_or(0.0);
    let duration     = state["duration"].as_f64().unwrap_or(0.0);

    let cur_m = (current_time as u64) / 60;
    let cur_s = (current_time as u64) % 60;
    let dur_m = (duration as u64) / 60;
    let dur_s = (duration as u64) % 60;
    println!("[ytune] state: title={:?} playing={} time={}:{:02}/{}:{:02}",
        title, playing, cur_m, cur_s, dur_m, dur_s);

    // Forward to tray popup
    let _ = app.emit("player_state_changed", &state);

    // Update Discord Rich Presence
    if let Some(discord) = app.try_state::<DiscordState>() {
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
            // Listen for player state events emitted by the injected JS in the YTM webview.
            // Using events (plugin:event|emit) instead of commands because Tauri 2 only allows
            // plugin commands from remote origins — user commands require a plugin + permission file.
            let handle = app.handle().clone();
            app.listen("ytune-state", move |event| {
                handle_player_state(&handle, event.payload());
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
        .on_window_event(|window, event| {
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
            resize_popup,
            player_control,
            player_seek,
            player_volume,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
