use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};
use tauri::{Emitter, Manager};

pub const DISCORD_CLIENT_ID: &str = "1513965769199980634";

// Removes view/like count segments from YTM's subtitle field (e.g. "803 mil visualizações")
fn clean_artist(raw: &str) -> String {
    const STAT_KEYWORDS: &[&str] = &["visualiza", "marca", "gostei", "views", "likes"];
    raw.lines()
        .flat_map(|line| line.split('•'))
        .map(|s| s.trim().trim_matches('"').trim())
        .filter(|s| {
            !s.is_empty()
            && !STAT_KEYWORDS.iter().any(|kw| s.to_lowercase().contains(kw))
        })
        .collect::<Vec<_>>()
        .join(" • ")
}

pub struct DiscordState(pub Mutex<Option<DiscordIpcClient>>);

// (title, song_start_ts, last_discord_update_unix, last_playing, last_thumbnail, last_liked)
pub struct DiscordTrackState(pub Mutex<(String, i64, i64, bool, String, bool)>);

pub fn start_discord_thread(handle: tauri::AppHandle) {
    std::thread::spawn(move || {
        loop {
            println!("[discord] trying to connect (client_id={})", DISCORD_CLIENT_ID);
            let mut client = DiscordIpcClient::new(DISCORD_CLIENT_ID);
            match client.connect() {
                Ok(_) => {
                    println!("[discord] connected");
                    if let Some(state) = handle.try_state::<DiscordState>() {
                        *state.0.lock().unwrap() = Some(client);
                        loop {
                            std::thread::sleep(std::time::Duration::from_secs(5));
                            let gone = handle.try_state::<DiscordState>()
                                .map(|s| s.0.lock().unwrap().is_none())
                                .unwrap_or(true);
                            if gone {
                                println!("[discord] disconnected, will retry in 15s");
                                break;
                            }
                        }
                    }
                }
                Err(e) => println!("[discord] connect failed: {:?}, retrying in 15s", e),
            }
            std::thread::sleep(std::time::Duration::from_secs(15));
        }
    });
}

pub fn handle_player_state(app: &tauri::AppHandle, payload: &str) {
    let Ok(state) = serde_json::from_str::<serde_json::Value>(payload) else { return };

    let title        = state["title"].as_str().unwrap_or("").to_string();
    let artist       = clean_artist(state["artist"].as_str().unwrap_or(""));
    let playing      = state["playing"].as_bool().unwrap_or(false);
    let liked        = state["liked"].as_bool().unwrap_or(false);
    let current_time = state["currentTime"].as_f64().unwrap_or(0.0);
    let duration     = state["duration"].as_f64().unwrap_or(0.0);
    let thumbnail    = state["thumbnail"].as_str().unwrap_or("").to_string();

    let palette_str = match (state["paletteH"].as_array(), state["paletteS"].as_array()) {
        (Some(hs), Some(ss)) => format!("{:?}/{:?}", hs, ss),
        _ => format!("({}/{})", state["paletteH"], state["paletteS"]),
    };
    let cur_m = (current_time as u64) / 60;
    let cur_s = (current_time as u64) % 60;
    let dur_m = (duration as u64) / 60;
    let dur_s = (duration as u64) % 60;
    println!("[ytune] state: title={:?} playing={} time={}:{:02}/{}:{:02} palette={}",
        title, playing, cur_m, cur_s, dur_m, dur_s, palette_str);

    // Forward to tray popup
    let _ = app.emit("player_state_changed", &state);

    // Rate-limited Discord update: only when title/playing changes, or every 15s
    let now_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let should_update = if let Some(ts) = app.try_state::<DiscordTrackState>() {
        let mut t = ts.0.lock().unwrap();
        let song_start        = now_unix - current_time as i64;
        let song_changed      = t.0 != title;
        let play_changed      = t.3 != playing;
        let thumbnail_changed = t.4 != thumbnail && !thumbnail.is_empty();
        let liked_changed     = t.5 != liked;
        let stale             = now_unix - t.2 >= 15;

        if song_changed {
            *t = (title.clone(), song_start, now_unix, playing, thumbnail.clone(), liked);
            true
        } else if play_changed || thumbnail_changed || liked_changed || stale {
            t.2 = now_unix;
            t.3 = playing;
            t.4 = thumbnail.clone();
            t.5 = liked;
            true
        } else {
            false
        }
    } else {
        true
    };

    let start_ts = if let Some(ts) = app.try_state::<DiscordTrackState>() {
        ts.0.lock().unwrap().1
    } else {
        now_unix - current_time as i64
    };

    if should_update && crate::tray::discord_enabled_get(app) {
        if let Some(discord) = app.try_state::<DiscordState>() {
            let mut guard = discord.0.lock().unwrap();
            if let Some(client) = guard.as_mut() {
                let status_line = match (playing, liked) {
                    (false, true)  => "❤️ Liked · Paused",
                    (false, false) => "⏸ Paused",
                    (true,  true)  => "❤️ Liked",
                    (true,  false) => "",
                };

                let assets = if thumbnail.starts_with("https://") {
                    println!("[discord] thumb url: {}", &thumbnail[..thumbnail.len().min(80)]);
                    let a = activity::Assets::new().large_image(&thumbnail);
                    if status_line.is_empty() { a } else { a.large_text(status_line) }
                } else {
                    println!("[discord] no https thumb, using fallback. thumb={:?}", &thumbnail[..thumbnail.len().min(30)]);
                    let a = activity::Assets::new().large_image("ytmusic");
                    if status_line.is_empty() { a } else { a.large_text(status_line) }
                };

                let mut act = activity::Activity::new()
                    .activity_type(activity::ActivityType::Listening)
                    .status_display_type(activity::StatusDisplayType::Details)
                    .details(&title)
                    .state(&artist)
                    .assets(assets);
                if playing {
                    act = act.timestamps(activity::Timestamps::new().start(start_ts));
                }
                let result = client.set_activity(act);
                match result {
                    Ok(_)  => println!("[discord] set_activity ok title={:?} playing={} liked={}", title, playing, liked),
                    Err(e) => { println!("[discord] set_activity failed: {:?}", e); *guard = None; }
                }
            }
        }
    }
}
