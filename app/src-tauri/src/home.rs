//! Home: listening metrics (`HomeView.HomeMetrics`) and the lyric highlight
//! (`HomeHighlight`).

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{Duration, Local, Months};
use fl_core::apple_json::{self, AppleDate};
use fl_core::listening::{AlbumRank, RankedItem, RecentContext};
use fl_core::stores::{
    highlight_candidate_lines, lyrics_cache_key, pinned_highlight_path, HighlightPick, HIGHLIGHT_FILE_READ_CAP,
};
use fl_core::{ArtistResolver, Track};
use serde::Serialize;
use tauri::State;

use crate::state::AppState;

type St<'a> = State<'a, Arc<AppState>>;

/// Session state for `HomeHighlight`: picked once per launch unless pinned.
#[derive(Default)]
pub struct Highlight {
    pick: Option<HighlightPick>,
    pinned: bool,
    has_picked: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TopGenre {
    name: String,
    share: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HomeMetrics {
    has_history: bool,
    recently_played: Vec<RecentContext>,
    weekly_minutes: [f64; 7],
    weekly_day_labels: [String; 7],
    hours_listened: i64,
    tracks_played: usize,
    albums_played: usize,
    sessions: usize,
    top_genre: Option<TopGenre>,
    streak_days: usize,
    top_artists: Vec<RankedItem>,
    top_albums: Vec<AlbumRank>,
    footer_album_count: usize,
    footer_hours: i64,
}

/// `StatsRange.since`: calendar year / month back, or seven days.
fn since(range: &str) -> Option<AppleDate> {
    let now = Local::now();
    let d = match range {
        "year" => now.checked_sub_months(Months::new(12))?,
        "month" => now.checked_sub_months(Months::new(1))?,
        "week" => now - Duration::days(7),
        _ => return None,
    };
    Some(AppleDate::from_chrono(&d))
}

#[tauri::command]
pub fn home_metrics(st: St, range: String) -> HomeMetrics {
    let since = since(&range);
    let (album_count, total_secs) = {
        let mut l = st.library.write();
        let tracks = l.tracks();
        (l.albums().len(), tracks.iter().filter_map(|t| t.duration).sum::<f64>())
    };
    let l = st.listening.lock();
    HomeMetrics {
        has_history: l.has_history(),
        recently_played: l.recently_played(12),
        weekly_minutes: l.weekly_minutes(),
        weekly_day_labels: l.weekly_day_labels(),
        hours_listened: (l.total_seconds_listened(since) / 3600.0).round() as i64,
        tracks_played: l.tracks_played_count(since),
        albums_played: l.albums_played_count(since),
        sessions: l.session_count(since),
        top_genre: l.top_genre(since).map(|(name, share)| TopGenre { name, share }),
        streak_days: l.current_streak_days(),
        top_artists: l.top_artists(5, since),
        top_albums: l.top_albums_this_week(6),
        footer_album_count: album_count,
        footer_hours: (total_secs / 3600.0).round() as i64,
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HighlightDto {
    lyric: String,
    song_title: String,
    artist_display: Option<String>,
    /// Artwork-store id for the blurred banner.
    image: Option<String>,
    is_pinned: bool,
}

fn dto(h: &Highlight, st: &AppState) -> Option<HighlightDto> {
    let p = h.pick.as_ref()?;
    Some(HighlightDto {
        lyric: p.lyric.clone(),
        song_title: p.song_title.clone(),
        artist_display: p.artist_display.clone(),
        image: p.image_data.clone().map(|d| st.artwork.register_bytes("highlight", d)),
        is_pinned: h.pinned,
    })
}

/// `pickIfNeeded`: a pinned pick, else a random good line from cached
/// lyrics, else from up to 12 files' embedded lyrics. Runs once per session,
/// after the library's first load.
#[tauri::command]
pub async fn home_highlight(st: St<'_>) -> Result<Option<HighlightDto>, String> {
    let st = st.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        {
            let h = st.highlight.lock();
            if h.has_picked {
                return dto(&h, &st);
            }
        }
        if !st.library.read().has_completed_initial_load {
            return None;
        }
        let (pick, pinned) = match load_pinned(&st) {
            Some(p) => (Some(p), true),
            None => (roll(&st), false),
        };
        let mut h = st.highlight.lock();
        if !h.has_picked {
            *h = Highlight { pick, pinned, has_picked: true };
        }
        dto(&h, &st)
    })
    .await
    .map_err(|e| e.to_string())
}

fn load_pinned(st: &AppState) -> Option<HighlightPick> {
    match apple_json::load::<HighlightPick>(&pinned_highlight_path(&st.dirs.data)) {
        Ok(p) => p,
        Err(e) => {
            log::warn!("[HomeHighlight] Failed to load pin: {e}");
            None
        }
    }
}

fn roll(st: &AppState) -> Option<HighlightPick> {
    let tracks = st.library.read().tracks();
    if tracks.is_empty() {
        return None;
    }
    let resolver = ArtistResolver::new(&tracks);
    let credit = |t: &Track| t.artist.clone().or_else(|| t.album_artist.clone());

    // Phase A: already-cached lyrics.
    let mut by_key: HashMap<String, &Track> = HashMap::new();
    for t in tracks.iter() {
        by_key.entry(lyrics_cache_key(credit(t).as_deref(), Some(&t.title), t.duration)).or_insert(t);
    }
    let mut cached: Vec<(&Track, String)> = {
        let cache = st.lyrics_cache.lock();
        cache
            .entries
            .values()
            .filter(|e| !e.not_found)
            .filter_map(|e| {
                let raw = e.synced_lyrics.clone().filter(|s| !s.is_empty()).or_else(|| e.plain_lyrics.clone())?;
                (!raw.is_empty()).then_some((*by_key.get(&e.key)?, raw))
            })
            .collect()
    };
    fastrand::shuffle(&mut cached);
    for (t, raw) in cached {
        if let Some(line) = random_line(&raw, t.duration) {
            return Some(finalize(st, t, line, &resolver));
        }
    }

    // Phase B: a bounded number of embedded-lyrics reads.
    let mut order: Vec<&Track> = tracks.iter().collect();
    fastrand::shuffle(&mut order);
    for t in order.into_iter().take(HIGHLIGHT_FILE_READ_CAP) {
        let Some(raw) = fl_tags::read_lyrics(&t.path).filter(|s| !s.is_empty()) else { continue };
        if let Some(line) = random_line(&raw, t.duration) {
            return Some(finalize(st, t, line, &resolver));
        }
    }
    None
}

fn random_line(raw: &str, duration: Option<f64>) -> Option<String> {
    let lines = highlight_candidate_lines(raw, duration);
    (!lines.is_empty()).then(|| lines[fastrand::usize(..lines.len())].clone())
}

fn finalize(st: &AppState, t: &Track, line: String, resolver: &ArtistResolver) -> HighlightPick {
    let credit = t.artist.clone().or_else(|| t.album_artist.clone());
    let key = resolver.keys_for_credit(credit.as_deref()).into_iter().next();
    let image = key
        .and_then(|k| {
            let store = st.artists.store.lock();
            let remote = st.artists.remote.lock();
            store.resolved_profile_image(&k, &remote).map(<[u8]>::to_vec)
        })
        .or_else(|| t.artwork.as_ref().map(|a| a.to_vec()));
    HighlightPick {
        lyric: line,
        song_title: t.title.clone(),
        artist_display: ArtistResolver::display_string(credit.as_deref()),
        image_data: image,
    }
}

/// Pins the current pick to disk, or unpins it (it stays for this session).
#[tauri::command]
pub fn toggle_highlight_pin(st: St) -> Option<HighlightDto> {
    let mut h = st.highlight.lock();
    let path = pinned_highlight_path(&st.dirs.data);
    if h.pinned {
        let _ = std::fs::remove_file(&path);
        h.pinned = false;
    } else if let Some(p) = &h.pick {
        match apple_json::save(&path, p) {
            Ok(()) => h.pinned = true,
            Err(e) => log::warn!("[HomeHighlight] Failed to save pin: {e}"),
        }
    }
    dto(&h, &st)
}
