//! Tauri commands: the UI's entire API into the app.

use std::path::PathBuf;
use std::sync::Arc;

use fl_audio::output_manager::OutputStatus;
use fl_audio::player::RepeatMode;
use fl_core::Uid;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::dto::{AlbumDto, LibrarySnapshot, TrackDto};
use crate::player_actor::{Cmd, PlayerSnapshot};
use crate::state::AppState;

type St<'a> = State<'a, Arc<AppState>>;

// MARK: - App / window

/// The UI has painted its first frame: show the window (avoids a white flash).
#[tauri::command]
pub fn app_ready(app: AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

#[tauri::command]
pub fn app_version() -> &'static str {
    include_str!("../../../version.txt").trim()
}

// MARK: - Settings

#[tauri::command]
pub fn get_settings(st: St) -> Value {
    serde_json::to_value(&*st.settings.lock()).unwrap_or(Value::Null)
}

/// Sets one `flactastic.*` key, as `@AppStorage` / `Settings` didSet would.
#[tauri::command]
pub fn set_setting(app: AppHandle, st: St, key: String, value: Value) -> Result<Value, String> {
    let updated = {
        let mut s = st.settings.lock();
        let mut json = serde_json::to_value(&*s).map_err(|e| e.to_string())?;
        let obj = json.as_object_mut().ok_or("settings")?;
        if value.is_null() {
            obj.remove(&key);
        } else {
            obj.insert(key.clone(), value);
        }
        let mut next: fl_core::settings::Settings = serde_json::from_value(json).map_err(|e| e.to_string())?;
        next.normalise();
        *s = next;
        s.clone()
    };
    st.save_settings();
    if key == "flactastic.countedPlayFraction" {
        st.player.send(Cmd::SetCountedPlayFraction(updated.counted_play_fraction));
    }
    let v = serde_json::to_value(&updated).unwrap_or(Value::Null);
    let _ = app.emit("settings://changed", v.clone());
    Ok(v)
}

// MARK: - Library

#[tauri::command]
pub fn open_library(app: AppHandle, st: St, path: String) -> Result<(), String> {
    let root = PathBuf::from(&path);
    if !root.is_dir() {
        return Err(format!("{path} isn't a folder"));
    }
    {
        let mut s = st.settings.lock();
        s.last_root_path = Some(path);
    }
    st.save_settings();
    st.inner().open_library(&app, root);
    Ok(())
}

/// Launch: reopen the saved library, or report that there's nothing to load.
#[tauri::command]
pub fn bootstrap_library(app: AppHandle, st: St) -> bool {
    let saved = st.settings.lock().last_root_path.clone();
    if let Some(p) = saved.map(PathBuf::from).filter(|p| p.is_dir()) {
        if st.root_path().as_deref() != Some(p.as_path()) {
            st.inner().open_library(&app, p);
        }
        return true;
    }
    false
}

#[tauri::command]
pub fn refresh_library(app: AppHandle, st: St) {
    st.inner().refresh_library(&app);
}

#[tauri::command]
pub fn library_snapshot(st: St) -> LibrarySnapshot {
    let root = st.root_path();
    let mut l = st.library.write();
    let tracks = l.tracks();
    let albums = l.albums();
    let resolver = fl_core::ArtistResolver::new(&tracks);
    LibrarySnapshot {
        revision: l.revision(),
        root: root.as_ref().map(|r| r.to_string_lossy().into_owned()),
        scan_state: l.scan_state.clone(),
        has_completed_initial_load: l.has_completed_initial_load,
        tracks: tracks.iter().map(|t| TrackDto::from_track(t, root.as_deref()).with_links(t, &resolver)).collect(),
        albums: albums.iter().map(|a| AlbumDto::from_album(a, &resolver)).collect(),
    }
}

/// Moves files to the Recycle Bin / trash and drops them from the library.
/// Returns how many couldn't be trashed.
#[tauri::command]
pub fn remove_tracks(app: AppHandle, st: St, ids: Vec<String>) -> usize {
    let tracks = st.tracks_by_id(&ids);
    let failures = st.library.write().remove_tracks(&tracks);
    let (rev, state, done) = {
        let l = st.library.read();
        (l.revision(), l.scan_state.clone(), l.has_completed_initial_load)
    };
    let _ = app.emit(
        "library://changed",
        crate::state::LibraryChanged { revision: rev, scan_state: state, has_completed_initial_load: done },
    );
    failures
}

// MARK: - Playback

#[tauri::command]
pub fn play_tracks(st: St, ids: Vec<String>, start: usize, source: Option<String>, shuffle: Option<bool>) {
    let tracks = st.tracks_by_id(&ids);
    if tracks.is_empty() {
        return;
    }
    let start = start.min(tracks.len() - 1);
    st.player.send(Cmd::StartQueue { tracks, start, source, play: true, shuffle });
}

#[tauri::command]
pub fn play_next(st: St, ids: Vec<String>) {
    st.player.send(Cmd::PlayNext(st.tracks_by_id(&ids)));
}

#[tauri::command]
pub fn add_to_queue(st: St, ids: Vec<String>) {
    st.player.send(Cmd::AddToQueue(st.tracks_by_id(&ids)));
}

#[tauri::command]
pub fn transport(st: St, action: String) -> Result<(), String> {
    let cmd = match action.as_str() {
        "toggle" => Cmd::TogglePlayPause,
        "play" => Cmd::Play,
        "pause" => Cmd::Pause,
        "next" => Cmd::Next,
        "previous" => Cmd::Previous,
        "volumeUp" => Cmd::VolumeUp,
        "volumeDown" => Cmd::VolumeDown,
        "shuffle" => Cmd::ToggleShuffle,
        other => return Err(format!("unknown transport action {other}")),
    };
    st.player.send(cmd);
    Ok(())
}

#[tauri::command]
pub fn seek(st: St, seconds: f64) {
    st.player.send(Cmd::Seek(seconds));
}

#[tauri::command]
pub fn set_volume(st: St, volume: f32) {
    st.player.send(Cmd::SetVolume(volume));
}

#[tauri::command]
pub fn set_repeat(st: St, mode: RepeatMode) {
    st.player.send(Cmd::SetRepeat(mode));
}

#[tauri::command]
pub fn jump_to(st: St, index: usize) {
    st.player.send(Cmd::JumpTo(index));
}

#[tauri::command]
pub fn remove_from_queue(st: St, index: usize) {
    st.player.send(Cmd::RemoveFromQueue(index));
}

#[tauri::command]
pub fn move_queue_track(st: St, source: String, destination: String) -> Result<(), String> {
    let (Some(s), Some(d)) = (Uid::parse(&source), Uid::parse(&destination)) else {
        return Err("bad track id".into());
    };
    st.player.send(Cmd::MoveTrack { source: s, destination: d });
    Ok(())
}

#[tauri::command]
pub fn set_queue_visible(st: St, visible: bool) {
    st.player.send(Cmd::SetQueueVisible(visible));
}

#[tauri::command]
pub fn player_snapshot(st: St) -> Option<PlayerSnapshot> {
    st.player.snapshot()
}

#[tauri::command]
pub fn set_spectrum(st: St, enabled: bool) {
    st.player.send(Cmd::SetSpectrum(enabled));
}

// MARK: - Output

#[tauri::command]
pub async fn output_status(st: St<'_>) -> Result<Option<OutputStatus>, String> {
    let p = st.player.clone();
    tauri::async_runtime::spawn_blocking(move || p.output_status()).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn select_output_device(st: St, id: Option<String>) {
    st.player.send(Cmd::SelectDevice(id));
}

#[tauri::command]
pub fn select_output_sample_rate(st: St, rate: Option<f64>) {
    st.player.send(Cmd::SelectSampleRate(rate));
}

#[tauri::command]
pub fn select_output_bit_depth(st: St, bits: Option<i64>) {
    st.player.send(Cmd::SelectBitDepth(bits));
}

#[tauri::command]
pub fn set_exclusive_output(st: St, enabled: bool) {
    st.player.send(Cmd::SetExclusive(enabled && cfg!(windows)));
}

// MARK: - Artists

use crate::artists::{ArtistDetailDto, ArtistDto, ArtistOverrideDto};

fn library_parts(st: &AppState) -> (Arc<Vec<fl_core::Track>>, Arc<Vec<fl_core::Album>>) {
    let mut l = st.library.write();
    (l.tracks(), l.albums())
}

#[tauri::command]
pub async fn artists(st: St<'_>) -> Result<Vec<ArtistDto>, String> {
    let st = st.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (tracks, albums) = library_parts(&st);
        st.artists.list(&tracks, &albums, &st.artwork)
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn artist_detail(st: St<'_>, key: String) -> Result<Option<ArtistDetailDto>, String> {
    let st = st.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (tracks, albums) = library_parts(&st);
        st.artists.detail(&key, &tracks, &albums, &st.artwork)
    })
    .await
    .map_err(|e| e.to_string())
}

/// `artistImageFetcher.ensureImage`, gated on `autoFetchArtistImages`.
#[tauri::command]
pub fn ensure_artist_image(st: St, key: String, display_name: String) {
    if st.settings.lock().auto_fetch_artist_images {
        st.artists.ensure_image(&key, &display_name);
    }
}

#[tauri::command]
pub fn artist_override(st: St, key: String) -> ArtistOverrideDto {
    st.artists.override_dto(&key, &st.artwork)
}

/// Images arrive as artwork ids (from `load_image_file` / `crop_image`).
#[tauri::command]
pub fn save_artist_override(
    app: AppHandle,
    st: St,
    key: String,
    display_name: Option<String>,
    banner: Option<String>,
    profile: Option<String>,
) {
    let bytes = |id: Option<String>| id.and_then(|id| st.artwork.original(&id)).map(|a| a.to_vec());
    st.artists.save_override(&key, display_name, bytes(banner), bytes(profile));
    let _ = app.emit("artists://changed", ());
}

#[tauri::command]
pub fn reset_artist_override(app: AppHandle, st: St, key: String) {
    st.artists.reset_override(&key);
    let _ = app.emit("artists://changed", ());
}

// MARK: - Images (cropper)

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadedImage {
    id: String,
    width: u32,
    height: u32,
}

/// Reads a picked image file into the artwork store for the cropper.
#[tauri::command]
pub async fn load_image_file(st: St<'_>, path: String) -> Result<LoadedImage, String> {
    let st = st.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let data = std::fs::read(&path).map_err(|e| e.to_string())?;
        let (width, height) = image::ImageReader::new(std::io::Cursor::new(&data))
            .with_guessed_format()
            .map_err(|e| e.to_string())?
            .into_dimensions()
            .map_err(|_| "That file isn't an image FLACtastic can read.".to_string())?;
        Ok(LoadedImage { id: st.artwork.register_bytes("upload", data), width, height })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// `ImageCropperView.commit`: `rect` is `[x, y, w, h]` in source pixels.
#[tauri::command]
pub async fn crop_image(st: St<'_>, id: String, rect: [f64; 4], out_width: u32, out_height: u32) -> Result<String, String> {
    let st = st.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let src = st.artwork.original(&id).ok_or("image expired")?;
        let png = crate::artists::crop_png(&src, rect, out_width, out_height).ok_or("couldn't crop the image")?;
        Ok(st.artwork.register_bytes("crop", png))
    })
    .await
    .map_err(|e| e.to_string())?
}
