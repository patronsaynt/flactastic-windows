//! `DiscordPresenceService`: pushes the playing track to the local Discord
//! client ("Listening to FLACtastic"), deduped so Discord's ~5 updates / 20 s
//! limit never swallows a real track change.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fl_core::Uid;
use fl_net::discord::{self, Activity, DiscordIpc};
use tauri::{AppHandle, Manager};

use crate::state::AppState;

/// A `startMs` move this large is a seek, not polling jitter.
const SEEK_DRIFT_MS: i64 = 1500;

/// Equality decides whether to send a fresh `SET_ACTIVITY`. Timestamps are
/// deliberately left out (they jitter every poll).
#[derive(Debug, Clone, PartialEq)]
struct Key {
    active_track: Option<Uid>,
    enabled: bool,
    has_timestamps: bool,
}

struct Snapshot {
    key: Key,
    title: String,
    state: String,
    album: String,
    artist: String,
    start_ms: Option<i64>,
    end_ms: Option<i64>,
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn snapshot(st: &AppState) -> Snapshot {
    let enabled = st.settings.lock().discord_rich_presence_enabled;
    let idle = Snapshot {
        key: Key { active_track: None, enabled, has_timestamps: false },
        title: String::new(),
        state: String::new(),
        album: String::new(),
        artist: String::new(),
        start_ms: None,
        end_ms: None,
    };
    let Some(p) = st.player.snapshot() else { return idle };
    let Some(id) = p.current_track_id.as_deref().filter(|_| p.is_playing) else { return idle };
    let Some(track) = st.tracks_by_id(&[id.to_owned()]).into_iter().next() else { return idle };

    let (mut start_ms, mut end_ms) = (None, None);
    if let Some(dur) = p.duration.or(track.duration).filter(|d| *d > 0.0) {
        let s = now_ms() - (p.current_time * 1000.0) as i64;
        start_ms = Some(s);
        end_ms = Some(s + (dur * 1000.0) as i64);
    }
    // "A ; B" / "A;B" tags read as "A, B".
    let raw = track.artist.clone().or(track.album_artist.clone()).unwrap_or_default();
    let artist = raw.split(';').map(str::trim).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(", ");
    let album = track.album.clone().unwrap_or_default();
    // The album already shows under the artwork (large_text).
    let state = if !artist.is_empty() {
        format!("by {artist}")
    } else {
        album.clone()
    };
    Snapshot {
        key: Key { active_track: Some(track.id), enabled, has_timestamps: start_ms.is_some() },
        title: track.title.clone(),
        state,
        album,
        artist,
        start_ms,
        end_ms,
    }
}

pub fn spawn(app: AppHandle) {
    std::thread::Builder::new()
        .name("discord-presence".into())
        .spawn(move || {
            let mut ipc = DiscordIpc::new(discord::CLIENT_ID);
            let mut artwork: HashMap<String, Option<String>> = HashMap::new();
            let mut last_key: Option<Key> = None;
            let mut last_start: Option<i64> = None;
            loop {
                let Some(st) = app.try_state::<Arc<AppState>>().map(|s| s.inner().clone()) else {
                    std::thread::sleep(Duration::from_secs(2));
                    continue;
                };
                let snap = snapshot(&st);
                let seeked = matches!((snap.start_ms, last_start), (Some(n), Some(o)) if (n - o).abs() >= SEEK_DRIFT_MS);
                if last_key.as_ref() != Some(&snap.key) || seeked {
                    let ok = if snap.key.enabled && snap.key.active_track.is_some() {
                        let k = format!("{}|{}", snap.artist.to_lowercase(), snap.album.to_lowercase());
                        let art = artwork
                            .entry(k)
                            .or_insert_with(|| discord::itunes_album_artwork(&snap.artist, &snap.album))
                            .clone();
                        ipc.set_activity(&Activity {
                            details: &snap.title,
                            state: &snap.state,
                            album: &snap.album,
                            artwork_url: art.as_deref(),
                            start_ms: snap.start_ms,
                            end_ms: snap.end_ms,
                        })
                    } else {
                        ipc.clear_activity()
                    };
                    // Only a delivered update counts; a failed write retries
                    // on the next tick.
                    if ok {
                        last_key = Some(snap.key.clone());
                        last_start = snap.start_ms;
                    }
                }
                // Poll fast only while playing (to catch seeks).
                let inactive = !snap.key.enabled || snap.key.active_track.is_none();
                std::thread::sleep(Duration::from_secs(if inactive { 10 } else { 2 }));
            }
        })
        .expect("presence thread");
}
