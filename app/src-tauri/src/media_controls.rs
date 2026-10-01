//! `NowPlayingController`: the OS media session (Windows SMTC / Linux MPRIS)
//! for hardware media keys, headset buttons and the system's now-playing
//! flyout. Player state goes out; transport commands come back.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use crossbeam_channel::{unbounded, Sender};
use souvlaki::{MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig};
use tauri::{AppHandle, Manager};

use crate::player_actor::{Cmd, PlayerSnapshot};
use crate::state::AppState;

static UPDATES: OnceLock<Sender<PlayerSnapshot>> = OnceLock::new();

/// Called from the player's state callback.
pub fn publish(snap: &PlayerSnapshot) {
    if let Some(tx) = UPDATES.get() {
        let _ = tx.send(snap.clone());
    }
}

/// Elapsed-time drift beyond which the position is re-sent (a seek).
const DRIFT_TOLERANCE: f64 = 1.0;

#[derive(Default)]
struct Pushed {
    track: Option<String>,
    playing: Option<bool>,
    duration: Option<f64>,
    elapsed: f64,
    at: Option<Instant>,
}

pub fn spawn(app: AppHandle) {
    let (tx, rx) = unbounded::<PlayerSnapshot>();
    if UPDATES.set(tx).is_err() {
        return;
    }
    #[cfg(windows)]
    let hwnd = app.get_webview_window("main").and_then(|w| w.hwnd().ok()).map(|h| h.0 as usize);
    std::thread::Builder::new()
        .name("media-controls".into())
        .spawn(move || {
            let config = PlatformConfig {
                display_name: "FLACtastic",
                dbus_name: "flactastic",
                #[cfg(windows)]
                hwnd: hwnd.map(|h| h as *mut std::ffi::c_void),
                #[cfg(not(windows))]
                hwnd: None,
            };
            let mut controls = match MediaControls::new(config) {
                Ok(c) => c,
                Err(e) => {
                    log::warn!("[MediaControls] unavailable: {e:?}");
                    return;
                }
            };
            let app2 = app.clone();
            if let Err(e) = controls.attach(move |e| on_event(&app2, e)) {
                log::warn!("[MediaControls] attach failed: {e:?}");
                return;
            }
            let mut pushed = Pushed::default();
            let art_dir = app.try_state::<Arc<AppState>>().map(|s| s.dirs.cache.join("now-playing"));
            while let Ok(mut snap) = rx.recv() {
                // Only the latest state matters.
                while let Ok(newer) = rx.try_recv() {
                    snap = newer;
                }
                update(&app, &mut controls, &mut pushed, &snap, art_dir.as_deref());
            }
        })
        .expect("media controls thread");
}

fn on_event(app: &AppHandle, e: MediaControlEvent) {
    let Some(st) = app.try_state::<Arc<AppState>>() else { return };
    let cmd = match e {
        MediaControlEvent::Play => Cmd::Play,
        MediaControlEvent::Pause | MediaControlEvent::Stop => Cmd::Pause,
        MediaControlEvent::Toggle => Cmd::TogglePlayPause,
        MediaControlEvent::Next => Cmd::Next,
        MediaControlEvent::Previous => Cmd::Previous,
        MediaControlEvent::SetPosition(MediaPosition(p)) => Cmd::Seek(p.as_secs_f64()),
        MediaControlEvent::Raise => {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
            return;
        }
        MediaControlEvent::Quit => {
            app.exit(0);
            return;
        }
        // Seek/skip-by, rate and volume aren't offered (as on the Mac).
        _ => return,
    };
    st.player.send(cmd);
}

fn update(app: &AppHandle, c: &mut MediaControls, pushed: &mut Pushed, snap: &PlayerSnapshot, art_dir: Option<&std::path::Path>) {
    let Some(id) = snap.current_track_id.clone() else {
        if pushed.playing.is_some() {
            let _ = c.set_playback(MediaPlayback::Stopped);
            *pushed = Pushed::default();
        }
        return;
    };
    let expected = pushed.elapsed
        + if pushed.playing == Some(true) { pushed.at.map_or(0.0, |t| t.elapsed().as_secs_f64()) } else { 0.0 };
    let changed = pushed.track.as_deref() != Some(id.as_str())
        || pushed.playing != Some(snap.is_playing)
        || pushed.duration != snap.duration;
    let seeked = (snap.current_time - expected).abs() > DRIFT_TOLERANCE;
    if !changed && !seeked {
        return;
    }

    if pushed.track.as_deref() != Some(id.as_str()) || pushed.duration != snap.duration {
        let Some(st) = app.try_state::<Arc<AppState>>() else { return };
        if let Some(track) = st.tracks_by_id(&[id.clone()]).into_iter().next() {
            let cover = art_dir.and_then(|d| cover_file(d, &id, track.artwork.as_deref()));
            let cover_url = cover.as_ref().map(|p| file_url(p));
            let _ = c.set_metadata(MediaMetadata {
                title: Some(&track.title),
                artist: track.artist.as_deref(),
                album: track.album.as_deref(),
                cover_url: cover_url.as_deref(),
                duration: snap.duration.or(track.duration).map(Duration::from_secs_f64),
            });
        }
    }
    let progress = Some(MediaPosition(Duration::from_secs_f64(snap.current_time.max(0.0))));
    let _ = c.set_playback(if snap.is_playing { MediaPlayback::Playing { progress } } else { MediaPlayback::Paused { progress } });

    pushed.track = Some(id);
    pushed.playing = Some(snap.is_playing);
    pushed.duration = snap.duration;
    pushed.elapsed = snap.current_time;
    pushed.at = Some(Instant::now());
}

/// The system reads artwork from a file; keep only the current one.
fn cover_file(dir: &std::path::Path, id: &str, bytes: Option<&[u8]>) -> Option<PathBuf> {
    let bytes = bytes?;
    let _ = std::fs::create_dir_all(dir);
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let _ = std::fs::remove_file(e.path());
        }
    }
    let ext = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) { "png" } else { "jpg" };
    let p = dir.join(format!("{id}.{ext}"));
    std::fs::write(&p, bytes).ok()?;
    Some(p)
}

/// `file://C:\...` on Windows (souvlaki strips the scheme and opens the
/// rest as a path); `file:///...` on Linux.
fn file_url(p: &std::path::Path) -> String {
    format!("file://{}", p.display())
}
