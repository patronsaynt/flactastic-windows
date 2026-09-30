//! Playlist commands (`PlaylistStore`, `PlaylistAddCoordinator`) and the
//! "recently played" context records that go with starting playback.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use fl_core::model::{artwork_content_id, join_relative};
use fl_core::{Playlist, Track, Uid};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::artwork::ArtworkStore;
use crate::state::AppState;

type St<'a> = State<'a, Arc<AppState>>;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistEntryDto {
    pub id: String,
    /// The library track this entry resolves to, if it still exists.
    pub track: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistDto {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    /// Unix seconds.
    pub date_created: f64,
    pub custom_artwork: Option<String>,
    /// `customArtwork ?? resolved.first?.artwork`.
    pub artwork: Option<String>,
    pub entries: Vec<PlaylistEntryDto>,
    /// Resolved tracks in order (`resolvedTracks`).
    pub track_ids: Vec<String>,
    pub total_duration: f64,
}

fn dto(p: &Playlist, by_id: &HashMap<Uid, &Track>, by_path: &HashMap<&Path, &Track>, root: Option<&Path>, art: &ArtworkStore) -> PlaylistDto {
    let mut entries = Vec::with_capacity(p.entries.len());
    let mut resolved: Vec<&Track> = Vec::new();
    for e in &p.entries {
        let t = e.track_id.and_then(|id| by_id.get(&id).copied()).or_else(|| {
            let abs = join_relative(root?, &e.relative_path);
            by_path.get(abs.as_path()).copied()
        });
        if let Some(t) = t {
            resolved.push(t);
        }
        entries.push(PlaylistEntryDto { id: e.id.to_string(), track: t.map(|t| t.id.to_string()) });
    }
    let custom = p.custom_artwork.clone().map(|b| art.register_bytes(&format!("playlist:{}", p.id), b));
    let first = resolved.first().and_then(|t| t.artwork.as_ref()).map(|a| artwork_content_id(a));
    PlaylistDto {
        id: p.id.to_string(),
        name: p.name.clone(),
        description: p.description.clone(),
        date_created: p.date_created.unix_seconds(),
        artwork: custom.clone().or(first),
        custom_artwork: custom,
        entries,
        track_ids: resolved.iter().map(|t| t.id.to_string()).collect(),
        total_duration: resolved.iter().map(|t| t.duration.unwrap_or(0.0)).sum(),
    }
}

fn uid(s: &str) -> Result<Uid, String> {
    Uid::parse(s).ok_or_else(|| format!("bad id {s}"))
}

fn changed(app: &AppHandle) {
    let _ = app.emit("playlists://changed", ());
}

fn save_listening(app: &AppHandle, st: &AppState) {
    if let Some((path, bytes)) = st.listening.lock().snapshot() {
        std::thread::spawn(move || {
            let _ = fl_core::apple_json::write_atomic(&path, &bytes);
        });
    }
    let _ = app.emit("listening://changed", ());
}

#[tauri::command]
pub fn playlists(st: St) -> Vec<PlaylistDto> {
    let tracks = st.library.read().tracks();
    let root = st.root_path();
    let mut by_id = HashMap::new();
    let mut by_path = HashMap::new();
    for t in tracks.iter() {
        by_id.entry(t.id).or_insert(t);
        by_path.entry(t.path.as_path()).or_insert(t);
    }
    let store = st.playlists.lock();
    store.playlists.iter().map(|p| dto(p, &by_id, &by_path, root.as_deref(), &st.artwork)).collect()
}

#[tauri::command]
pub fn create_playlist(app: AppHandle, st: St, name: String) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("empty name".into());
    }
    let p = st.playlists.lock().create(name);
    changed(&app);
    Ok(p.id.to_string())
}

#[tauri::command]
pub fn delete_playlist(app: AppHandle, st: St, id: String) -> Result<(), String> {
    st.playlists.lock().delete(uid(&id)?);
    changed(&app);
    Ok(())
}

#[tauri::command]
pub fn rename_playlist(app: AppHandle, st: St, id: String, name: String) -> Result<(), String> {
    let name = name.trim();
    if !name.is_empty() {
        st.playlists.lock().rename(uid(&id)?, name);
        changed(&app);
    }
    Ok(())
}

/// `PlaylistEditorView.save`. `artwork` is an artwork-store id (the current
/// cover's, or a fresh crop's) or null to remove it.
#[tauri::command]
pub fn update_playlist_metadata(
    app: AppHandle,
    st: St,
    id: String,
    name: String,
    description: Option<String>,
    artwork: Option<String>,
) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("empty name".into());
    }
    let description = description
        .map(|d| d.trim().chars().take(Playlist::DESCRIPTION_MAX_LENGTH).collect::<String>())
        .filter(|d| !d.is_empty());
    let bytes = artwork.and_then(|a| st.artwork.original(&a)).map(|a| a.to_vec());
    st.playlists.lock().update_metadata(uid(&id)?, name, description, bytes);
    changed(&app);
    Ok(())
}

/// How many of these tracks the playlist already holds (the duplicate prompt).
#[tauri::command]
pub fn playlist_duplicate_count(st: St, id: String, track_ids: Vec<String>) -> Result<usize, String> {
    let tracks = st.tracks_by_id(&track_ids);
    Ok(st.playlists.lock().duplicate_count(&tracks, uid(&id)?))
}

#[tauri::command]
pub fn add_to_playlist(app: AppHandle, st: St, id: String, track_ids: Vec<String>, skip_duplicates: bool) -> Result<(), String> {
    let tracks = st.tracks_by_id(&track_ids);
    st.playlists.lock().add_tracks(&tracks, uid(&id)?, skip_duplicates);
    changed(&app);
    Ok(())
}

/// `createPlaylistAndAdd`: the inline "New playlist name…" field.
#[tauri::command]
pub fn create_playlist_and_add(app: AppHandle, st: St, name: String, track_ids: Vec<String>) -> Result<(), String> {
    let name = name.trim();
    let tracks = st.tracks_by_id(&track_ids);
    if name.is_empty() || tracks.is_empty() {
        return Ok(());
    }
    let mut store = st.playlists.lock();
    let p = store.create(name);
    store.add_tracks(&tracks, p.id, false);
    drop(store);
    changed(&app);
    Ok(())
}

#[tauri::command]
pub fn remove_playlist_entries(app: AppHandle, st: St, id: String, entry_ids: Vec<String>) -> Result<(), String> {
    let ids: HashSet<Uid> = entry_ids.iter().filter_map(|s| Uid::parse(s)).collect();
    st.playlists.lock().remove_entries(&ids, uid(&id)?);
    changed(&app);
    Ok(())
}

#[tauri::command]
pub fn move_playlist_entry(app: AppHandle, st: St, id: String, source: String, before: String) -> Result<(), String> {
    st.playlists.lock().move_entry(uid(&source)?, uid(&before)?, uid(&id)?);
    changed(&app);
    Ok(())
}

#[tauri::command]
pub fn record_playlist_play(app: AppHandle, st: St, id: String) -> Result<(), String> {
    let p = st.playlists.lock().get(uid(&id)?).cloned();
    if let Some(p) = p {
        st.listening.lock().record_playlist_play(&p);
        save_listening(&app, &st);
    }
    Ok(())
}

#[tauri::command]
pub fn record_album_play(app: AppHandle, st: St, id: String) {
    let album = st.library.write().albums_by_id().get(&id).cloned();
    if let Some(a) = album {
        st.listening.lock().record_album_play(&a);
        save_listening(&app, &st);
    }
}
