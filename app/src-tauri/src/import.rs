//! Import (`ImportTrackView`, `ImportAlbumView`, `ImportPlaylistView`):
//! read picked files, then copy them into the library root and write tags.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use fl_core::import_copy::{album_folder_name, copy_into, track_file_name};
use fl_core::Track;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use crate::dto::TrackDto;
use crate::metadata::TrackEdit;
use crate::state::AppState;

type St<'a> = State<'a, Arc<AppState>>;

/// Files read by `import_load`, held until committed (keyed by stub id).
#[derive(Default)]
pub struct Staged(Mutex<HashMap<String, Track>>);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LoadProgress {
    loaded: usize,
    total: usize,
}

const NO_ROOT: &str = "Choose a library folder in Settings before importing.";

/// Reads each supported file's tags (`scanner.loadMetadata`), reporting
/// `import://progress`. Unsupported files are skipped.
#[tauri::command]
pub async fn import_load(app: AppHandle, st: St<'_>, paths: Vec<String>) -> Result<Vec<TrackDto>, String> {
    let st = st.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let stubs: Vec<Track> = paths.iter().filter_map(|p| Track::make_from_path(Path::new(p))).collect();
        if stubs.is_empty() {
            return Err("No supported audio files in selection.".to_owned());
        }
        let total = stubs.len();
        let mut out = Vec::with_capacity(total);
        let mut staged = st.staged.0.lock();
        staged.clear();
        for (i, stub) in stubs.iter().enumerate() {
            let t = fl_library::scanner::load_metadata(stub, None);
            if let Some(a) = &t.artwork {
                st.artwork.register(a);
            }
            out.push(TrackDto::from_track(&t, None));
            staged.insert(t.id.to_string(), t);
            let _ = app.emit("import://progress", LoadProgress { loaded: i + 1, total });
        }
        Ok(out)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportItem {
    /// `TrackDto.id` from `import_load`.
    pub token: String,
    /// Tags to write after copying; `None` copies the file untouched
    /// (playlist import).
    pub edit: Option<TrackEdit>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumFolder {
    pub artist: Option<String>,
    pub album: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SaveProgress {
    saved: usize,
    total: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    /// Library ids of the imported tracks, in item order.
    track_ids: Vec<String>,
    /// First failure, if any (the rest still imported).
    error: Option<String>,
}

/// Copies each file into the root (or `<root>/<artist> - <album>`), named
/// `<artist> - <title>.<ext>`, writes its tags, and adds the copies to the
/// library.
#[tauri::command]
pub async fn import_commit(
    app: AppHandle,
    st: St<'_>,
    items: Vec<ImportItem>,
    album_folder: Option<AlbumFolder>,
) -> Result<ImportResult, String> {
    let st = st.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let root = st.root_path().ok_or(NO_ROOT)?;
        let dir: PathBuf = match &album_folder {
            Some(f) => root.join(album_folder_name(f.artist.as_deref(), &f.album)),
            None => root.clone(),
        };
        let staged: HashMap<String, Track> = st.staged.0.lock().clone();
        let total = items.len();
        let mut collected: Vec<Track> = Vec::new();
        let mut first_error: Option<String> = None;
        for item in &items {
            let result = (|| -> Result<Track, String> {
                let source = staged.get(&item.token).ok_or("The file is no longer staged.")?;
                let (artist, title) = match &item.edit {
                    Some(e) => (e.artist.clone(), if e.title.trim().is_empty() { source.title.clone() } else { e.title.clone() }),
                    None => (source.artist.clone(), source.title.clone()),
                };
                let ext = source.path.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
                let name = track_file_name(artist.as_deref(), &title, &ext);
                let dest = copy_into(&source.path, &dir, &name)
                    .map_err(|e| format!("Failed to copy \"{}\": {e}", file_name(&source.path)))?;
                let relocated = source.relocated(dest);
                match &item.edit {
                    None => Ok(relocated),
                    Some(e) => {
                        let art = crate::metadata::artwork_change(&st, &e.artwork)?;
                        fl_tags::write(&relocated, &crate::metadata::tag_write(e, art)).map_err(|err| err.to_string())
                    }
                }
            })();
            match result {
                Ok(t) => {
                    collected.push(t);
                    let _ = app.emit("import://saved", SaveProgress { saved: collected.len(), total });
                }
                Err(e) => {
                    first_error.get_or_insert(e);
                }
            }
        }

        // Stable ids are assigned on the way in; resolve the library's copies by path.
        let paths: Vec<PathBuf> = collected.iter().map(|t| t.path.clone()).collect();
        let ids = {
            let mut l = st.library.write();
            l.add_imported_tracks(collected);
            l.persist_metadata_cache();
            let (tracks, albums) = (l.tracks(), l.albums());
            st.artwork.register_library(&tracks, &albums);
            let by_path: HashMap<&Path, String> = tracks.iter().map(|t| (t.path.as_path(), t.id.to_string())).collect();
            paths.iter().filter_map(|p| by_path.get(p.as_path()).cloned()).collect()
        };
        st.staged.0.lock().clear();
        st.emit_library_changed(&app);
        Ok(ImportResult { track_ids: ids, error: first_error })
    })
    .await
    .map_err(|e| e.to_string())?
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}
