//! `PlaylistRebuildCoordinator`: rebuilds a Spotify playlist into the library
//! plus a matching FLACtastic playlist. Per track, in playlist order: reuse a
//! library copy, else match it to Amazon Music (Odesli) and download through
//! Lucida (Amazon first, then the Spotify URL), retrying transient failures.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crossbeam_channel::unbounded;
use fl_core::model::relative_path;
use fl_core::{Track, Uid};
use fl_net::lucida::Options;
use fl_net::remote::{RemoteCoverArt, RemotePlaylist, RemoteTrack};
use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::downloads::{find_existing_match, Downloads, Outcome};
use crate::lucida::Lucida;
use crate::state::AppState;

/// Tracks matched + downloaded at once (a sliding window). Kept low: time per
/// track is Lucida's server-side work, and more concurrency only adds
/// initiation pressure on its rate limiter.
const MAX_CONCURRENT: usize = 3;
const LAUNCH_STAGGER: Duration = Duration::from_millis(750);
const MAX_DOWNLOAD_ATTEMPTS: usize = 3;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackFailure {
    /// 1-based playlist position.
    pub index: usize,
    pub title: String,
    pub artist: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReusedTrack {
    pub index: usize,
    pub title: String,
    pub artist: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub playlist_name: String,
    pub total: usize,
    pub downloaded: usize,
    pub reused: Vec<ReusedTrack>,
    pub failures: Vec<TrackFailure>,
    pub already_in_playlist: usize,
    pub is_resume: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Phase {
    Idle,
    FetchingArtwork,
    Running { current: usize, total: usize },
    Finished { summary: Summary },
}

enum TrackResult {
    Reused(String, Option<Uid>),
    Downloaded(String),
    Failed(TrackFailure),
    SkippedNoPath,
    Cancelled,
    AlreadyInPlaylist,
}

pub struct Rebuild {
    app: AppHandle,
    phase: Mutex<Phase>,
    cancel: Arc<AtomicBool>,
    /// Download jobs in flight for this rebuild, cancelled with it.
    active_jobs: Mutex<HashSet<String>>,
}

impl Rebuild {
    pub fn new(app: AppHandle) -> Arc<Self> {
        Arc::new(Rebuild {
            app,
            phase: Mutex::new(Phase::Idle),
            cancel: Arc::default(),
            active_jobs: Mutex::default(),
        })
    }

    pub fn phase(&self) -> Phase {
        self.phase.lock().clone()
    }

    fn set_phase(&self, p: Phase) {
        *self.phase.lock() = p.clone();
        let _ = self.app.emit("rebuild://changed", p);
    }

    pub fn is_running(&self) -> bool {
        matches!(*self.phase.lock(), Phase::Running { .. } | Phase::FetchingArtwork)
    }

    pub fn start(self: &Arc<Self>, playlist: RemotePlaylist, options: Options) {
        if self.is_running() {
            return;
        }
        self.cancel.store(false, Ordering::SeqCst);
        self.set_phase(Phase::FetchingArtwork);
        let me = self.clone();
        std::thread::spawn(move || me.run(playlist, options));
    }

    /// Tears down the in-flight downloads; finished tracks stay in the playlist.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
        let downloads = self.app.state::<Arc<Downloads>>();
        for id in self.active_jobs.lock().drain() {
            downloads.cancel(&id);
        }
    }

    pub fn dismiss_summary(&self) {
        if matches!(*self.phase.lock(), Phase::Finished { .. }) {
            self.set_phase(Phase::Idle);
        }
    }

    fn run(self: &Arc<Self>, playlist: RemotePlaylist, options: Options) {
        let st = self.app.state::<Arc<AppState>>().inner().clone();
        let total = playlist.tracks.len();
        let Some(root) = st.root_path() else {
            self.set_phase(Phase::Finished {
                summary: Summary {
                    playlist_name: playlist.title.clone(),
                    total,
                    downloaded: 0,
                    reused: vec![],
                    failures: vec![TrackFailure {
                        index: 0,
                        title: playlist.title.clone(),
                        artist: String::new(),
                        reason: "No music folder selected.".into(),
                    }],
                    already_in_playlist: 0,
                    is_resume: false,
                },
            });
            return;
        };

        // Reuse a same-named playlist (case-insensitive) rather than duplicate.
        let wanted = playlist.title.trim().to_lowercase();
        let existing = st.playlists.lock().playlists.iter().find(|p| p.name.trim().to_lowercase() == wanted).cloned();
        let is_resume = existing.is_some();
        let target = existing.unwrap_or_else(|| st.playlists.lock().create(&playlist.title));
        let existing_paths: HashSet<String> = target.entries.iter().map(|e| e.relative_path.clone()).collect();
        let existing_ids: HashSet<Uid> = target.entries.iter().filter_map(|e| e.track_id).collect();

        let artwork = RemoteCoverArt::best(&playlist.cover_art).and_then(|b| fl_net::deezer::download_image(&b.url).ok());
        st.playlists.lock().update_metadata(
            target.id,
            &playlist.title,
            playlist.creator.as_ref().map(|c| format!("by {c}")),
            artwork,
        );
        let _ = self.app.emit("playlists://changed", ());

        let tracks = Arc::new(playlist.tracks.clone());
        self.set_phase(Phase::Running { current: 0, total });

        let mut downloaded = 0;
        let mut reused = Vec::new();
        let mut failures = Vec::new();
        let mut already = 0;
        let mut completed = 0;

        let existing_paths = Arc::new(existing_paths);
        let existing_ids = Arc::new(existing_ids);
        let (tx, rx) = unbounded::<(usize, TrackResult)>();
        let mut next_to_launch = 0usize;
        let mut next_to_append = 0usize;
        let mut in_flight = 0usize;
        let mut buffered: HashMap<usize, TrackResult> = HashMap::new();
        let mut stop_launching = false;
        let mut launch_tick = 0usize;

        let mut launch = |count: usize, next_to_launch: &mut usize, in_flight: &mut usize, stop: bool| {
            let mut launched = 0;
            while launched < count && *next_to_launch < total && !stop {
                let position = *next_to_launch;
                let delay = LAUNCH_STAGGER * (launch_tick % MAX_CONCURRENT) as u32;
                let (me, tracks, tx, root, options) = (self.clone(), tracks.clone(), tx.clone(), root.clone(), options.clone());
                let (paths, ids) = (existing_paths.clone(), existing_ids.clone());
                std::thread::spawn(move || {
                    if !delay.is_zero() {
                        std::thread::sleep(delay);
                    }
                    let r = me.process_track(&tracks[position], position, &root, &options, &paths, &ids);
                    let _ = tx.send((position, r));
                });
                *next_to_launch += 1;
                *in_flight += 1;
                launch_tick += 1;
                launched += 1;
            }
        };

        launch(MAX_CONCURRENT, &mut next_to_launch, &mut in_flight, false);
        while in_flight > 0 {
            let Ok((position, result)) = rx.recv() else { break };
            in_flight -= 1;
            completed += 1;
            self.set_phase(Phase::Running { current: completed, total });
            let was_cancelled = matches!(result, TrackResult::Cancelled);
            buffered.insert(position, result);
            // Flush every contiguous result so the playlist keeps its order.
            while let Some(r) = buffered.remove(&next_to_append) {
                let track = &tracks[next_to_append];
                match r {
                    TrackResult::Reused(rel, id) => {
                        st.playlists.lock().append_entries(&[rel], &[id], target.id);
                        reused.push(ReusedTrack {
                            index: next_to_append + 1,
                            title: track.title.clone(),
                            artist: track.artist_label(),
                        });
                        let _ = self.app.emit("playlists://changed", ());
                    }
                    TrackResult::Downloaded(rel) => {
                        st.playlists.lock().append_entries(&[rel], &[None], target.id);
                        downloaded += 1;
                        let _ = self.app.emit("playlists://changed", ());
                    }
                    TrackResult::Failed(f) => failures.push(f),
                    TrackResult::AlreadyInPlaylist => already += 1,
                    TrackResult::SkippedNoPath | TrackResult::Cancelled => {}
                }
                next_to_append += 1;
            }
            if was_cancelled || self.cancel.load(Ordering::SeqCst) {
                stop_launching = true;
            }
            if !stop_launching {
                launch(1, &mut next_to_launch, &mut in_flight, stop_launching);
            }
        }

        st.refresh_library(&self.app);
        self.set_phase(Phase::Finished {
            summary: Summary {
                playlist_name: playlist.title,
                total,
                downloaded,
                reused,
                failures,
                already_in_playlist: already,
                is_resume,
            },
        });
    }

    fn process_track(
        self: &Arc<Self>,
        track: &RemoteTrack,
        position: usize,
        root: &Path,
        options: &Options,
        existing_paths: &HashSet<String>,
        existing_ids: &HashSet<Uid>,
    ) -> TrackResult {
        let st = self.app.state::<Arc<AppState>>();
        let artist = track.artist_label();
        let failure = |reason: String| {
            TrackResult::Failed(TrackFailure { index: position + 1, title: track.title.clone(), artist: artist.clone(), reason })
        };
        if self.cancel.load(Ordering::SeqCst) {
            return TrackResult::Cancelled;
        }

        let library: Arc<Vec<Track>> = st.library.read().tracks();
        if let Some(existing) = find_existing_match(track, &library) {
            return match relative_path(&existing.path, root) {
                Some(rel) if existing_paths.contains(&rel) || existing_ids.contains(&existing.id) => {
                    TrackResult::AlreadyInPlaylist
                }
                Some(rel) => TrackResult::Reused(rel, Some(existing.id)),
                None => TrackResult::SkippedNoPath,
            };
        }

        let lucida = self.app.state::<Arc<Lucida>>().inner().clone();
        let amazon = track.url.as_deref().and_then(|u| lucida.amazon.amazon_url(u));
        let sources: Vec<String> = amazon.into_iter().chain(track.url.clone()).collect();
        if sources.is_empty() {
            return failure("No source URL.".into());
        }

        let mut outcome = Outcome::Failed("No source URL.".into());
        'rounds: for attempt in 0..MAX_DOWNLOAD_ATTEMPTS {
            let mut all_permanent = true;
            for source in &sources {
                if self.cancel.load(Ordering::SeqCst) {
                    outcome = Outcome::Cancelled;
                    break 'rounds;
                }
                outcome = self.download(&lucida, track, source, options);
                match &outcome {
                    Outcome::Completed(_) | Outcome::Skipped(_) | Outcome::Cancelled => break 'rounds,
                    Outcome::Failed(reason) => {
                        if !is_permanent_failure(reason) {
                            all_permanent = false;
                        }
                    }
                }
            }
            if all_permanent {
                break;
            }
            if attempt < MAX_DOWNLOAD_ATTEMPTS - 1 {
                std::thread::sleep(transient_backoff(attempt));
            }
        }

        match outcome {
            Outcome::Completed(p) => relative_path(&p, root).map_or(TrackResult::SkippedNoPath, TrackResult::Downloaded),
            Outcome::Skipped(p) => {
                let tid = st.library.read().tracks().iter().find(|t| t.path == p).map(|t| t.id);
                relative_path(&p, root).map_or(TrackResult::SkippedNoPath, |rel| TrackResult::Reused(rel, tid))
            }
            Outcome::Failed(reason) => failure(reason),
            Outcome::Cancelled => TrackResult::Cancelled,
        }
    }

    /// Lucida embeds the source's tags + cover; the rebuild trusts those.
    fn download(&self, lucida: &Arc<Lucida>, track: &RemoteTrack, source: &str, options: &Options) -> Outcome {
        let t = track.with_source(source);
        let mut o = options.clone();
        o.add_metadata = true;
        lucida.set_options(o, &t.id);
        let downloads = self.app.state::<Arc<Downloads>>().inner().clone();
        let (id, outcome) = downloads.enqueue_and_await_with(t, true, |id| {
            self.active_jobs.lock().insert(id.to_owned());
        });
        self.active_jobs.lock().remove(&id);
        outcome
    }
}

/// Permanent problems (no matching release, unsupported/region-locked
/// source) that retrying can't fix.
pub fn is_permanent_failure(reason: &str) -> bool {
    let r = reason.to_lowercase();
    [
        "no match",
        "no matching",
        "not found",
        "no results",
        "no result",
        "unsupported",
        "no source",
        "region",
        "not available",
        "unavailable in",
        "invalid",
        "doesn't look",
        "couldn't find",
        "could not find",
    ]
    .iter()
    .any(|p| r.contains(p))
}

/// ~3 s, ~8 s, ~20 s plus up to 1.5 s jitter.
fn transient_backoff(attempt: usize) -> Duration {
    let base = [3.0, 8.0, 20.0][attempt.min(2)];
    Duration::from_secs_f64(base + fastrand::f64() * 1.5)
}

// MARK: - Commands

#[tauri::command]
pub fn rebuild_state(st: State<Arc<Rebuild>>) -> Phase {
    st.phase()
}

#[tauri::command]
pub fn rebuild_start(st: State<Arc<Rebuild>>, playlist: RemotePlaylist, options: Options) {
    st.start(playlist, options);
}

#[tauri::command]
pub fn rebuild_cancel(st: State<Arc<Rebuild>>) {
    st.cancel();
}

#[tauri::command]
pub fn rebuild_dismiss(st: State<Arc<Rebuild>>) {
    st.dismiss_summary();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permanent_failures() {
        assert!(is_permanent_failure("No matching release found"));
        assert!(is_permanent_failure("Region locked"));
        assert!(!is_permanent_failure("Server busy, try again"));
        assert!(!is_permanent_failure("Lucida couldn't start this download."));
    }
}
