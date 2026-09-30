//! `LyricsFetcher`: lazy, deduplicated lyrics lookups for the visualizer —
//! the file's own LYRICS tag first, then lrclib — plus the view state the
//! Lyrics mode renders from.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{unbounded, Sender};
use fl_core::lyrics::Lyrics;
use fl_core::stores::{lyrics_cache_key, LyricsCacheEntry};
use fl_core::{AppleDate, Track};
use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::state::AppState;

type St<'a> = State<'a, Arc<AppState>>;

/// lrclib's polite concurrency cap.
const WORKERS: usize = 4;
/// A failed lookup isn't retried for this long…
const ERROR_TTL: Duration = Duration::from_secs(300);
/// …unless it was only a transport failure.
const TRANSPORT_ERROR_TTL: Duration = Duration::from_secs(20);

struct Job {
    track: Track,
    save_to_file: bool,
    delay: Duration,
}

pub struct LyricsFetcher {
    in_flight: Mutex<HashSet<String>>,
    /// key → (when, transport-only)
    recent_errors: Mutex<HashMap<String, (Instant, bool)>>,
    tx: Sender<Job>,
}

pub fn cache_key(t: &Track) -> String {
    lyrics_cache_key(t.artist.as_deref().or(t.album_artist.as_deref()), Some(&t.title), t.duration)
}

impl LyricsFetcher {
    pub fn new(app: AppHandle) -> Arc<LyricsFetcher> {
        let (tx, rx) = unbounded::<Job>();
        let me = Arc::new(LyricsFetcher { in_flight: Mutex::default(), recent_errors: Mutex::default(), tx });
        for i in 0..WORKERS {
            let rx = rx.clone();
            let app = app.clone();
            std::thread::Builder::new()
                .name(format!("fl-lyrics-{i}"))
                .spawn(move || {
                    while let Ok(job) = rx.recv() {
                        let Some(st) = app.try_state::<Arc<AppState>>().map(|s| s.inner().clone()) else { continue };
                        if !job.delay.is_zero() {
                            std::thread::sleep(job.delay);
                        }
                        let key = cache_key(&job.track);
                        fetch(&st, &job);
                        st.lyrics.in_flight.lock().remove(&key);
                        let _ = app.emit("lyrics://changed", key);
                    }
                })
                .ok();
        }
        me
    }

    pub fn has_recent_error(&self, key: &str) -> bool {
        self.recent_errors.lock().get(key).is_some_and(|(when, transport)| {
            when.elapsed() < if *transport { TRANSPORT_ERROR_TTL } else { ERROR_TTL }
        })
    }

    /// `ensureLyrics`: no-op on a fresh cache hit, an in-flight request, or a
    /// key that just failed.
    fn ensure(&self, st: &AppState, track: &Track, save_to_file: bool, delay: Duration) {
        if track.is_mix_compilation {
            return;
        }
        let key = cache_key(track);
        if st.lyrics_cache.lock().is_fresh(&key, AppleDate::now()) {
            return;
        }
        if self.has_recent_error(&key) || !self.in_flight.lock().insert(key) {
            return;
        }
        let _ = self.tx.send(Job { track: track.clone(), save_to_file, delay });
    }
}

fn fetch(st: &AppState, job: &Job) {
    let t = &job.track;
    let key = cache_key(t);
    let entry = |plain: Option<String>, synced: Option<String>, not_found: bool| LyricsCacheEntry {
        key: key.clone(),
        plain_lyrics: plain,
        synced_lyrics: synced,
        fetched_at: AppleDate::now(),
        not_found,
    };

    // 1. Embedded lyrics: no network at all.
    if let Some(stored) = fl_tags::read_lyrics(&t.path).filter(|s| !s.trim().is_empty()) {
        let synced = Lyrics::parse_lrc(&stored).is_synced;
        let e = if synced { entry(None, Some(stored), false) } else { entry(Some(stored), None, false) };
        st.lyrics_cache.lock().set(e);
        return;
    }

    // 2. lrclib, keyed on the lead artist.
    let artist = fl_net::lrclib::primary_artist(t.artist.as_deref().or(t.album_artist.as_deref())).unwrap_or_default();
    match fl_net::lrclib::fetch_lyrics(&artist, &t.title, t.album.as_deref(), t.duration) {
        Ok(resp) => {
            st.lyrics.recent_errors.lock().remove(&key);
            let found = resp.as_ref().filter(|r| {
                !r.instrumental.unwrap_or(false)
                    && (r.synced_lyrics.as_deref().is_some_and(|s| !s.is_empty())
                        || r.plain_lyrics.as_deref().is_some_and(|s| !s.is_empty()))
            });
            match found {
                Some(r) => {
                    st.lyrics_cache.lock().set(entry(r.plain_lyrics.clone(), r.synced_lyrics.clone(), false));
                    if job.save_to_file {
                        let payload = r.synced_lyrics.clone().filter(|s| !s.is_empty()).or_else(|| r.plain_lyrics.clone());
                        if let Some(p) = payload.filter(|p| !p.is_empty()) {
                            if let Err(e) = fl_tags::write_lyrics(&t.path, Some(&p)) {
                                log::info!("[LyricsFetcher] Failed to embed lyrics in {}: {e}", t.path.display());
                            }
                        }
                    }
                }
                None => st.lyrics_cache.lock().set(entry(None, None, true)),
            }
        }
        Err(e) => {
            let transport = matches!(&e, fl_net::NetError::Http(h) if fl_net::lrclib::is_transport(h));
            log::info!("[LyricsFetcher] {}: {e}", t.title);
            st.lyrics.recent_errors.lock().insert(key, (Instant::now(), transport));
        }
    }
}

#[derive(Serialize)]
pub struct LineDto {
    timestamp: Option<f64>,
    text: String,
}

/// What the Lyrics mode shows for a track.
#[derive(Serialize)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum LyricsState {
    Mix,
    Disabled,
    Loading,
    NotFound,
    Ready { lines: Vec<LineDto>, is_synced: bool },
}

/// `LyricsVisualizerView.lyricsArea` plus the `.task(id:)` fetches: kicks
/// off the current track (and warms the neighbours) and reports its state.
#[tauri::command]
pub fn visualizer_lyrics(st: St, track_id: String, neighbour_ids: Vec<String>) -> LyricsState {
    let Some(track) = st.tracks_by_id(&[track_id]).into_iter().next() else { return LyricsState::Loading };
    if track.is_mix_compilation {
        return LyricsState::Mix;
    }
    let (enabled, save) = {
        let s = st.settings.lock();
        (s.lyrics_lookup_enabled, s.save_lyrics_to_files)
    };
    if !enabled {
        return LyricsState::Disabled;
    }
    st.lyrics.ensure(&st, &track, save, Duration::ZERO);
    for (i, n) in st.tracks_by_id(&neighbour_ids).iter().enumerate() {
        st.lyrics.ensure(&st, n, save, Duration::from_secs_f64(0.6 + i as f64 * 0.3));
    }

    let key = cache_key(&track);
    let entry = st.lyrics_cache.lock().entries.get(&key).cloned();
    match entry {
        Some(e) if e.not_found => LyricsState::NotFound,
        Some(e) => match parsed(&e, track.duration.unwrap_or(0.0)) {
            Some(l) => LyricsState::Ready {
                is_synced: l.is_synced,
                lines: l.lines.into_iter().map(|x| LineDto { timestamp: x.timestamp, text: x.text }).collect(),
            },
            None => LyricsState::NotFound,
        },
        None if st.lyrics.has_recent_error(&key) => LyricsState::NotFound,
        None => LyricsState::Loading,
    }
}

/// `parsedLyrics(from:)`: synced LRC first, else plain text spread over the track.
fn parsed(e: &LyricsCacheEntry, duration: f64) -> Option<Lyrics> {
    if let Some(s) = e.synced_lyrics.as_deref().filter(|s| !s.is_empty()) {
        let l = Lyrics::parse_lrc(s);
        if !l.lines.is_empty() {
            return Some(l);
        }
    }
    e.plain_lyrics.as_deref().filter(|p| !p.is_empty()).map(|p| Lyrics::from_plain_text(p, duration))
}
