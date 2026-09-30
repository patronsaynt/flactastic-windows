//! Organizer (`OrganizerModel`): plan a profile against the library for the
//! live preview, then apply the last plan and patch the library in place.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use fl_core::format::AudioFileFormat;
use fl_core::organizer::{self, HierarchyLevel, OpStatus, Operation, OrganizerProfile, Phase, PreviewRow};
use fl_core::{AppleDate, Track};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use crate::state::AppState;

type St<'a> = State<'a, Arc<AppState>>;

/// Rows the preview builds before summarising the rest.
const PREVIEW_LIMIT: usize = 300;

/// The plan the preview is showing, which Apply executes.
#[derive(Default)]
pub struct Plan(Mutex<Option<(OrganizerProfile, Vec<Operation>)>>);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanDto {
    move_count: usize,
    unchanged_count: usize,
    conflict_count: usize,
    rows: Vec<PreviewRow>,
    hidden_track_count: usize,
    track_count: usize,
}

#[tauri::command]
pub async fn organizer_plan(st: St<'_>, profile: OrganizerProfile) -> Result<PlanDto, String> {
    let st = st.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let root = st.root_path().ok_or("Choose a source folder in Settings before organizing.")?;
        let tracks = st.library.read().tracks();
        let ops = organizer::plan(&tracks, &profile, &root);
        let (rows, hidden) = organizer::preview_rows(&ops, &root, PREVIEW_LIMIT);
        let count = |f: fn(&OpStatus) -> bool| ops.iter().filter(|o| f(&o.status)).count();
        let dto = PlanDto {
            move_count: count(|s| matches!(s, OpStatus::Move)),
            unchanged_count: count(|s| matches!(s, OpStatus::Unchanged)),
            conflict_count: count(|s| matches!(s, OpStatus::Conflict(_))),
            rows,
            hidden_track_count: hidden,
            track_count: tracks.len(),
        };
        *st.organizer_plan.0.lock() = Some((profile, ops));
        Ok(dto)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Progress {
    phase: Phase,
    completed: usize,
    total: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyResult {
    error: Option<String>,
    message: Option<String>,
}

/// `OrganizerModel.apply`: moves, prunes, validates; then repoints the
/// library's tracks and the track-ID sidecar at the new paths.
#[tauri::command]
pub async fn organizer_apply(app: AppHandle, st: St<'_>) -> Result<ApplyResult, String> {
    let st = st.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let root = st.root_path().ok_or("Choose a source folder in Settings before organizing.")?;
        let (profile, ops) = st.organizer_plan.0.lock().take().ok_or("Nothing to apply.")?;
        let result = organizer::execute(&ops, &root, profile.delete_empty_originals, |phase, completed, total| {
            let _ = app.emit("organizer://progress", Progress { phase, completed, total });
        });

        if !result.moved.is_empty() {
            let moved: HashMap<_, PathBuf> = result.moved.iter().cloned().collect();
            let path_map = organizer::relative_path_map(&ops, &result, &root);
            let mut l = st.library.write();
            let updated: Vec<Track> = l
                .tracks()
                .iter()
                .filter_map(|t| {
                    moved.get(&t.id).map(|p| Track {
                        path: p.clone(),
                        file_format: AudioFileFormat::classify(p).unwrap_or(t.file_format),
                        ..t.clone()
                    })
                })
                .collect();
            l.replace_tracks(&updated);
            l.track_ids.rename_paths(&path_map);
            l.track_ids.save();
            l.persist_metadata_cache();
            let (tracks, albums) = (l.tracks(), l.albums());
            st.artwork.register_library(&tracks, &albums);
            drop(l);
            st.player.send(crate::player_actor::Cmd::UpdateTracks(updated));
            st.emit_library_changed(&app);
        }

        let (moved, failed, lost) = (result.moved.len(), result.failed.len(), result.lost.len());
        let plural = |n: usize| if n == 1 { "" } else { "s" };
        Ok(if lost > 0 {
            ApplyResult {
                error: Some(format!(
                    "Validation failed: {lost} file{} missing from their destination after move. Check the source folder before re-running.",
                    if lost == 1 { " is" } else { "s are" }
                )),
                message: None,
            }
        } else if failed > 0 {
            ApplyResult { error: Some(format!("{failed} file{} could not be moved. Organized {moved}.", plural(failed))), message: None }
        } else {
            ApplyResult { error: None, message: Some(format!("Organized {moved} file{}.", plural(moved))) }
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExampleRequest {
    template: String,
    fallback: String,
    /// The file-name example also shows the extension.
    #[serde(default)]
    is_filename: bool,
}

/// The builder's "Example:" lines, rendered against the first library track
/// (or a stand-in before anything is scanned).
#[tauri::command]
pub fn organizer_examples(st: St, requests: Vec<ExampleRequest>, primary_artist_only: bool) -> Vec<String> {
    let tracks = st.library.read().tracks();
    let sample;
    let track = match tracks.first() {
        Some(t) => t,
        None => {
            sample = Track {
                artist: Some("SZA".into()),
                album_artist: Some("SZA".into()),
                album: Some("Album Title".into()),
                track_number: Some(1),
                genre: Some("Electronic".into()),
                year: Some(2024),
                date_added: Some(AppleDate::now()),
                ..Track::new(PathBuf::from("/Music/Sample Song.flac"), "Sample Song".into(), AudioFileFormat::Flac)
            };
            &sample
        }
    };
    let ext = track.path.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
    requests
        .iter()
        .map(|r| {
            let s = organizer::render(&r.template, track, &r.fallback, primary_artist_only);
            if r.is_filename { format!("{s}.{ext}") } else { s }
        })
        .collect()
}

#[derive(Serialize)]
pub struct TokenDto {
    placeholder: String,
    description: &'static str,
}

#[tauri::command]
pub fn organizer_tokens() -> Vec<TokenDto> {
    organizer::ALL_TOKENS.iter().map(|t| TokenDto { placeholder: format!("{{{}}}", t.key), description: t.description }).collect()
}

/// "New from preset" entries, each with fresh ids.
#[tauri::command]
pub fn organizer_presets() -> Vec<OrganizerProfile> {
    OrganizerProfile::presets()
}

#[tauri::command]
pub fn organizer_new_level() -> HierarchyLevel {
    OrganizerProfile::new_level()
}

#[tauri::command]
pub fn organizer_duplicate(profile: OrganizerProfile) -> OrganizerProfile {
    profile.duplicate()
}
