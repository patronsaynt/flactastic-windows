//! Metadata editors (`MetadataWriter` call sites): track, album and merge
//! writes, embedded lyrics and chapter markers.

use std::sync::Arc;

use fl_core::cue::TrackMarker;
use fl_core::lyrics::Lyrics;
use fl_core::stores::{lyrics_cache_key, LyricsCacheEntry};
use fl_core::{AppleDate, Track, Uid};
use fl_tags::{ArtworkChange, TagWrite};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use crate::player_actor::Cmd;
use crate::state::AppState;

type St<'a> = State<'a, Arc<AppState>>;

/// `MetadataWriter.ArtworkChange` as sent by the UI: an artwork-store id for
/// a new image.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum ArtworkEdit {
    Unchanged,
    Removed,
    Updated { id: String },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackEdit {
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub year: Option<i64>,
    pub genre: Option<String>,
    pub secondary_genres: Vec<String>,
    pub track_number: Option<i64>,
    pub artwork: ArtworkEdit,
    /// Absent = unchanged; `null` inside `Some` clears ALBUMARTIST.
    #[serde(default, with = "double_option")]
    pub album_artist: Option<Option<String>>,
    #[serde(default)]
    pub compilation: Option<bool>,
    #[serde(default)]
    pub mix_compilation: Option<bool>,
}

mod double_option {
    use serde::{Deserialize, Deserializer};
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Option<String>>, D::Error> {
        Ok(Some(Option::deserialize(d)?))
    }
}

pub(crate) fn artwork_change(st: &AppState, a: &ArtworkEdit) -> Result<ArtworkChange, String> {
    Ok(match a {
        ArtworkEdit::Unchanged => ArtworkChange::Unchanged,
        ArtworkEdit::Removed => ArtworkChange::Removed,
        ArtworkEdit::Updated { id } => {
            ArtworkChange::Updated(st.artwork.original(id).ok_or("The chosen image is no longer available.")?.to_vec())
        }
    })
}

pub(crate) fn tag_write(e: &TrackEdit, artwork: ArtworkChange) -> TagWrite {
    TagWrite {
        title: e.title.clone(),
        artist: e.artist.clone(),
        album: e.album.clone(),
        year: e.year,
        genre: e.genre.clone(),
        secondary_genres: e.secondary_genres.clone(),
        track_number: e.track_number,
        artwork,
        album_artist: e.album_artist.clone(),
        compilation: e.compilation,
        mix_compilation: e.mix_compilation,
    }
}

fn track(st: &AppState, id: &str) -> Result<Track, String> {
    st.tracks_by_id(&[id.to_owned()]).into_iter().next().ok_or_else(|| "Track not found".to_owned())
}

/// `library.updateTrack` / `replaceTracks`, then refresh artwork, the
/// player's queue copies, and the UI.
fn apply(app: &AppHandle, st: &AppState, updated: Vec<Track>, album_id: Option<&str>) {
    if updated.is_empty() {
        return;
    }
    {
        let mut l = st.library.write();
        l.replace_tracks(&updated);
        if let Some(a) = album_id {
            l.invalidate_album_artwork(a);
        }
        l.persist_metadata_cache();
        let (tracks, albums) = (l.tracks(), l.albums());
        st.artwork.register_library(&tracks, &albums);
    }
    st.player.send(Cmd::UpdateTracks(updated));
    st.emit_library_changed(app);
}

#[tauri::command]
pub async fn write_track_metadata(app: AppHandle, st: St<'_>, id: String, edit: TrackEdit) -> Result<(), String> {
    let st = st.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let t = track(&st, &id)?;
        let art = artwork_change(&st, &edit.artwork)?;
        let updated = fl_tags::write(&t, &tag_write(&edit, art)).map_err(|e| e.to_string())?;
        apply(&app, &st, vec![updated], None);
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumTrackEdit {
    pub id: String,
    pub edit: TrackEdit,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Progress {
    saved: usize,
    total: usize,
}

/// `AlbumMetadataEditorView.save` and `MergeTracksIntoAlbumView.save`: each
/// track written in turn, with `metadata://progress` after each. Every
/// success is applied even when some fail; the first error is returned.
#[tauri::command]
pub async fn write_tracks_metadata(
    app: AppHandle,
    st: St<'_>,
    album_id: Option<String>,
    edits: Vec<AlbumTrackEdit>,
) -> Result<(), String> {
    let st = st.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let total = edits.len();
        let mut collected = Vec::new();
        let mut first_error: Option<String> = None;
        for e in &edits {
            let result = track(&st, &e.id).and_then(|t| {
                let art = artwork_change(&st, &e.edit.artwork)?;
                fl_tags::write(&t, &tag_write(&e.edit, art)).map_err(|err| err.to_string())
            });
            match result {
                Ok(u) => {
                    collected.push(u);
                    let _ = app.emit("metadata://progress", Progress { saved: collected.len(), total });
                }
                Err(err) => {
                    first_error.get_or_insert(err);
                }
            }
        }
        apply(&app, &st, collected, album_id.as_deref());
        first_error.map_or(Ok(()), Err)
    })
    .await
    .map_err(|e| e.to_string())?
}

// MARK: - Lyrics

fn cache_key(t: &Track) -> String {
    lyrics_cache_key(t.artist.as_deref().or(t.album_artist.as_deref()), Some(&t.title), t.duration)
}

/// The file's LYRICS tag, else whatever lrclib content is cached.
#[tauri::command]
pub async fn read_track_lyrics(st: St<'_>, id: String) -> Result<String, String> {
    let st = st.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let t = track(&st, &id)?;
        if let Some(s) = fl_tags::read_lyrics(&t.path).filter(|s| !s.trim().is_empty()) {
            return Ok(s);
        }
        let cache = st.lyrics_cache.lock();
        Ok(cache
            .entries
            .get(&cache_key(&t))
            .filter(|e| !e.not_found)
            .and_then(|e| e.synced_lyrics.clone().or_else(|| e.plain_lyrics.clone()))
            .unwrap_or_default())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Writes LYRICS and refreshes the cache so the visualizer sees the edit.
#[tauri::command]
pub async fn write_track_lyrics(app: AppHandle, st: St<'_>, id: String, lyrics: String) -> Result<(), String> {
    let st = st.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let t = track(&st, &id)?;
        let payload = (!lyrics.is_empty()).then_some(lyrics.as_str());
        fl_tags::write_lyrics(&t.path, payload).map_err(|e| e.to_string())?;
        let trimmed = lyrics.trim();
        let synced = Lyrics::parse_lrc(trimmed).is_synced;
        let entry = LyricsCacheEntry {
            key: cache_key(&t),
            plain_lyrics: (!trimmed.is_empty() && !synced).then(|| trimmed.to_owned()),
            synced_lyrics: (!trimmed.is_empty() && synced).then(|| trimmed.to_owned()),
            fetched_at: AppleDate::now(),
            not_found: false,
        };
        st.lyrics_cache.lock().set(entry);
        let _ = app.emit("lyrics://changed", id);
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Serialize, Deserialize)]
pub struct LineDto {
    timestamp: Option<f64>,
    text: String,
}

/// Seeds the sync sheet: LRC is parsed, anything else split on newlines.
#[tauri::command]
pub fn parse_lyrics_for_sync(raw: String) -> Vec<LineDto> {
    let parsed = Lyrics::parse_lrc(&raw);
    if parsed.is_synced {
        parsed.lines.into_iter().map(|l| LineDto { timestamp: l.timestamp, text: l.text }).collect()
    } else {
        raw.split('\n').map(|s| LineDto { timestamp: None, text: s.to_owned() }).collect()
    }
}

#[tauri::command]
pub fn serialize_lrc(lines: Vec<LineDto>) -> String {
    let pairs: Vec<(Option<f64>, String)> = lines.into_iter().map(|l| (l.timestamp, l.text)).collect();
    Lyrics::serialize_lrc(&pairs)
}

// MARK: - Markers

#[derive(Serialize, Deserialize)]
pub struct MarkerDto {
    id: String,
    timestamp: f64,
    title: String,
}

#[tauri::command]
pub async fn read_track_markers(st: St<'_>, id: String) -> Result<Vec<MarkerDto>, String> {
    let st = st.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let t = track(&st, &id)?;
        Ok(fl_tags::read_markers(&t.path)
            .into_iter()
            .map(|m| MarkerDto { id: m.id.to_string(), timestamp: m.timestamp, title: m.title })
            .collect())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn write_track_markers(st: St<'_>, id: String, markers: Vec<MarkerDto>) -> Result<(), String> {
    let st = st.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let t = track(&st, &id)?;
        let m: Vec<TrackMarker> = markers
            .into_iter()
            .map(|m| TrackMarker { id: Uid::parse(&m.id).unwrap_or_else(Uid::new_v4), timestamp: m.timestamp, title: m.title })
            .collect();
        fl_tags::write_markers(&t.path, &m).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// `TrackMarker.parseUserTimestamp`: "mm:ss", "m:ss" or "h:mm:ss".
#[tauri::command]
pub fn parse_marker_timestamp(text: String) -> Option<f64> {
    TrackMarker::parse_user_timestamp(&text)
}
