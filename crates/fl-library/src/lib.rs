//! `LibraryStore`: the library state and the scan / refresh / metadata-load
//! pipeline around it.
//!
//! State lives in [`Library`] behind a [`LibraryHandle`] (an `RwLock`); the
//! long-running jobs ([`open_folder`], [`refresh`]) run on their own threads,
//! take the lock only to publish, and report through a [`LibraryEvents`]
//! callback. Starting a job cancels the previous one (generation counter), as
//! the Swift store cancels `scanTask`/`metadataTask`.

pub mod scanner;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use fl_core::library::{self, deduplicate_artwork, normalise_artist_tags};
use fl_core::metadata_cache::{self, MetadataCacheEntry};
use fl_core::model::{relative_path, sort_for_library};
use fl_core::track_ids::{FileIdentity, TrackIdStore};
use fl_core::{Album, Artwork, Track, Uid};
use parking_lot::RwLock;
use serde::Serialize;

pub use scanner::ScanError;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum ScanState {
    Idle,
    Scanning,
    Refreshing,
    Done { count: usize },
    Failed { message: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum LibraryEvent {
    /// `tracks` changed (revision bumped).
    TracksChanged,
    ScanStateChanged,
    /// The first load resolved; the UI lifts its loading cover.
    InitialLoadCompleted,
}

pub type LibraryEvents = Arc<dyn Fn(LibraryEvent) + Send + Sync>;

pub struct Library {
    pub root: Option<PathBuf>,
    tracks: Arc<Vec<Track>>,
    revision: u64,
    pub scan_state: ScanState,
    pub has_completed_initial_load: bool,
    /// Per-album artwork, seeded after scans; a present `None` means "no art".
    album_artwork: HashMap<String, Option<Artwork>>,
    albums_cache: Option<Arc<Vec<Album>>>,
    albums_by_id: Option<Arc<HashMap<String, Album>>>,
    pub track_ids: TrackIdStore,
    identity: Arc<dyn FileIdentity>,
    /// Bumped to cancel the running scan / metadata job.
    generation: Arc<AtomicU64>,
}

pub type LibraryHandle = Arc<RwLock<Library>>;

impl Library {
    pub fn new(identity: Arc<dyn FileIdentity>) -> Library {
        Library {
            root: None,
            tracks: Arc::new(Vec::new()),
            revision: 0,
            scan_state: ScanState::Idle,
            has_completed_initial_load: false,
            album_artwork: HashMap::new(),
            albums_cache: None,
            albums_by_id: None,
            track_ids: TrackIdStore::new(),
            identity,
            generation: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn handle(identity: Arc<dyn FileIdentity>) -> LibraryHandle {
        Arc::new(RwLock::new(Library::new(identity)))
    }

    pub fn tracks(&self) -> Arc<Vec<Track>> {
        self.tracks.clone()
    }

    /// Bumped on every `tracks` assignment (`tracksRevision`).
    pub fn revision(&self) -> u64 {
        self.revision
    }

    fn set_tracks(&mut self, tracks: Vec<Track>) {
        self.tracks = Arc::new(tracks);
        self.revision = self.revision.wrapping_add(1);
        self.albums_cache = None;
        self.albums_by_id = None;
    }

    pub fn albums(&mut self) -> Arc<Vec<Album>> {
        if let Some(a) = &self.albums_cache {
            return a.clone();
        }
        let a = Arc::new(library::build_albums(&self.tracks, &self.album_artwork));
        self.albums_cache = Some(a.clone());
        a
    }

    pub fn albums_by_id(&mut self) -> Arc<HashMap<String, Album>> {
        if let Some(m) = &self.albums_by_id {
            return m.clone();
        }
        let mut m = HashMap::new();
        for a in self.albums().iter() {
            m.entry(a.id.clone()).or_insert_with(|| a.clone());
        }
        let m = Arc::new(m);
        self.albums_by_id = Some(m.clone());
        m
    }

    pub fn track(&self, id: Uid) -> Option<&Track> {
        self.tracks.iter().find(|t| t.id == id)
    }

    pub fn album_for(&mut self, track: Uid) -> Option<Album> {
        self.albums().iter().find(|a| a.tracks.iter().any(|t| t.id == track)).cloned()
    }

    /// Seeds album artwork for albums not yet present; existing entries (from a
    /// prior seed or the album editor) are kept.
    pub fn seed_album_artwork_cache(&mut self) {
        let albums = self.albums();
        let mut changed = false;
        for a in albums.iter() {
            if !self.album_artwork.contains_key(&a.id) {
                self.album_artwork.insert(a.id.clone(), a.artwork.clone());
                changed = true;
            }
        }
        if changed {
            self.albums_cache = None;
            self.albums_by_id = None;
        }
    }

    /// Album editor saved: recompute this album's cover from its tracks.
    pub fn invalidate_album_artwork(&mut self, album_id: &str) {
        self.album_artwork.remove(album_id);
        self.albums_cache = None;
        self.albums_by_id = None;
        self.seed_album_artwork_cache();
    }

    pub fn update_track(&mut self, id: Uid, updated: Track) {
        let mut v = (*self.tracks).clone();
        if let Some(i) = v.iter().position(|t| t.id == id) {
            v[i] = updated;
            self.set_tracks(v);
        }
    }

    /// Applies many updates in one assignment, so an album rename never shows
    /// the album split across two grouping keys.
    pub fn replace_tracks(&mut self, updated: &[Track]) {
        if updated.is_empty() {
            return;
        }
        let by_id: HashMap<Uid, &Track> = updated.iter().map(|t| (t.id, t)).collect();
        let v = self.tracks.iter().map(|t| by_id.get(&t.id).map_or_else(|| t.clone(), |u| (*u).clone())).collect();
        self.set_tracks(v);
    }

    /// Imported files (not from a scan). An already-known path keeps its entry.
    pub fn add_imported_tracks(&mut self, imported: Vec<Track>) {
        let existing: HashSet<&Path> = self.tracks.iter().map(|t| t.path.as_path()).collect();
        let mut fresh: Vec<Track> = imported.into_iter().filter(|t| !existing.contains(t.path.as_path())).collect();
        if fresh.is_empty() {
            return;
        }
        if let Some(root) = self.root.clone() {
            fresh = self.apply_stable_ids(fresh, &root);
            self.track_ids.save();
        }
        let mut v = (*self.tracks).clone();
        v.extend(fresh);
        sort_for_library(&mut v);
        normalise_artist_tags(&mut v);
        self.set_tracks(v);
        self.seed_album_artwork_cache();
    }

    /// Trashes the files and drops their tracks. Tracks whose file couldn't be
    /// trashed stay; returns how many failed.
    pub fn remove_tracks(&mut self, to_remove: &[Track]) -> usize {
        let mut removed = HashSet::new();
        let mut failures = 0;
        for t in to_remove {
            if !t.path.exists() || fl_platform::move_to_trash(&t.path).is_ok() {
                removed.insert(t.id);
            } else {
                failures += 1;
            }
        }
        if removed.is_empty() {
            return failures;
        }
        let v = self.tracks.iter().filter(|t| !removed.contains(&t.id)).cloned().collect();
        self.set_tracks(v);
        self.seed_album_artwork_cache();
        self.persist_metadata_cache();
        failures
    }

    /// Rewrites each track's id to its stable UUID (file identity, then the
    /// sidecar, else new). Doesn't save; callers save once per batch.
    fn apply_stable_ids(&mut self, tracks: Vec<Track>, root: &Path) -> Vec<Track> {
        let ident = self.identity.clone();
        tracks
            .into_iter()
            .map(|mut t| {
                if let Some(rel) = relative_path(&t.path, root) {
                    t.id = self.track_ids.assign(ident.as_ref(), &t.path, &rel);
                }
                t
            })
            .collect()
    }

    /// Rewrites `metadata-cache.json` from the current tracks, on a
    /// background thread.
    pub fn persist_metadata_cache(&self) {
        let Some(root) = self.root.clone() else { return };
        let snapshot = self.tracks.clone();
        std::thread::spawn(move || metadata_cache::rebuild(&snapshot, &root));
    }

    fn finish_load(&mut self) {
        let mut v = (*self.tracks).clone();
        normalise_artist_tags(&mut v);
        deduplicate_artwork(&mut v);
        self.set_tracks(v);
        self.seed_album_artwork_cache();
        self.persist_metadata_cache();
    }
}

// MARK: - Jobs

/// A job's cancellation token: live until a newer job starts on the same
/// library.
#[derive(Clone)]
struct Gen(Arc<AtomicU64>, u64);

impl Gen {
    fn start(lib: &LibraryHandle) -> Gen {
        let c = lib.read().generation.clone();
        let g = c.fetch_add(1, Ordering::AcqRel) + 1;
        Gen(c, g)
    }
    fn live(&self) -> bool {
        self.0.load(Ordering::Acquire) == self.1
    }
}

/// Opens `root` as the library: loads the ID sidecar, cheap-scans, publishes
/// the stubs, then loads metadata in batches. Returns immediately.
pub fn open_folder(lib: &LibraryHandle, root: PathBuf, events: LibraryEvents) {
    let gen = Gen::start(lib);
    {
        let mut l = lib.write();
        l.root = Some(root.clone());
        l.scan_state = ScanState::Scanning;
        l.album_artwork.clear();
        l.set_tracks(Vec::new());
        l.track_ids.load(&root);
    }
    events(LibraryEvent::ScanStateChanged);
    events(LibraryEvent::TracksChanged);

    let lib = lib.clone();
    std::thread::Builder::new()
        .name("fl-library-scan".into())
        .spawn(move || {
            let result = scanner::scan(&root, &|| !gen.live());
            if !gen.live() {
                return;
            }
            match result {
                Ok(cheap) => {
                    let count = cheap.len();
                    {
                        let mut l = lib.write();
                        let stable = l.apply_stable_ids(cheap, &root);
                        l.track_ids.save();
                        l.set_tracks(stable);
                        l.scan_state = ScanState::Done { count };
                    }
                    events(LibraryEvent::TracksChanged);
                    events(LibraryEvent::ScanStateChanged);
                    load_metadata_job(&lib, &root, &gen, None, &events);
                }
                Err(e) => {
                    let mut l = lib.write();
                    l.scan_state = ScanState::Failed { message: e.to_string() };
                    l.has_completed_initial_load = true;
                    drop(l);
                    events(LibraryEvent::ScanStateChanged);
                    events(LibraryEvent::InitialLoadCompleted);
                }
            }
        })
        .expect("spawn scan thread");
}

/// Rescans: existing tracks keep their in-memory state; only new files get
/// IDs and a metadata load. Returns immediately.
pub fn refresh(lib: &LibraryHandle, events: LibraryEvents) {
    let root = {
        let mut l = lib.write();
        let Some(root) = l.root.clone() else { return };
        if l.scan_state == ScanState::Scanning {
            return;
        }
        l.scan_state = ScanState::Refreshing;
        root
    };
    let gen = Gen::start(lib);
    events(LibraryEvent::ScanStateChanged);
    let lib = lib.clone();
    std::thread::Builder::new()
        .name("fl-library-refresh".into())
        .spawn(move || {
            let result = scanner::scan(&root, &|| !gen.live());
            if !gen.live() {
                return;
            }
            let scanned = match result {
                Ok(s) => s,
                Err(e) => {
                    lib.write().scan_state = ScanState::Failed { message: e.to_string() };
                    events(LibraryEvent::ScanStateChanged);
                    return;
                }
            };
            let new_ids = {
                let mut l = lib.write();
                let existing: HashMap<PathBuf, Track> = l.tracks.iter().map(|t| (t.path.clone(), t.clone())).collect();
                let raw_new: Vec<Track> = scanned.iter().filter(|t| !existing.contains_key(&t.path)).cloned().collect();
                let stable_new = l.apply_stable_ids(raw_new, &root);
                if !stable_new.is_empty() {
                    l.track_ids.save();
                }
                let stable_by_path: HashMap<&Path, &Track> = stable_new.iter().map(|t| (t.path.as_path(), t)).collect();
                let merged: Vec<Track> = scanned
                    .iter()
                    .map(|s| existing.get(&s.path).or_else(|| stable_by_path.get(s.path.as_path()).copied()).unwrap_or(s).clone())
                    .collect();
                let count = merged.len();
                l.set_tracks(merged);
                l.scan_state = ScanState::Done { count };
                stable_new.iter().map(|t| t.id).collect::<HashSet<Uid>>()
            };
            events(LibraryEvent::TracksChanged);
            events(LibraryEvent::ScanStateChanged);
            if !new_ids.is_empty() {
                load_metadata_job(&lib, &root, &gen, Some(new_ids), &events);
            }
        })
        .expect("spawn refresh thread");
}

const MAX_CONCURRENT: usize = 8;
const BATCH: usize = 24;
const FLUSH_EVERY: Duration = Duration::from_millis(250);

/// Loads metadata with 8 workers, publishing in batches of 24 or every
/// 250 ms. `only`: restrict to these track IDs (refresh of new files).
fn load_metadata_job(lib: &LibraryHandle, root: &Path, gen: &Gen, only: Option<HashSet<Uid>>, events: &LibraryEvents) {
    let cached: HashMap<String, MetadataCacheEntry> = metadata_cache::load(root);
    let snapshot: Vec<Track> = {
        let l = lib.read();
        l.tracks.iter().filter(|t| only.as_ref().is_none_or(|o| o.contains(&t.id))).cloned().collect()
    };
    let next = AtomicUsize::new(0);
    let (tx, rx) = std::sync::mpsc::channel::<Track>();

    std::thread::scope(|s| {
        for _ in 0..MAX_CONCURRENT.min(snapshot.len().max(1)) {
            let tx = tx.clone();
            let (next, snapshot, cached) = (&next, &snapshot, &cached);
            s.spawn(move || loop {
                if !gen.live() {
                    return;
                }
                let i = next.fetch_add(1, Ordering::AcqRel);
                let Some(t) = snapshot.get(i) else { return };
                let entry = relative_path(&t.path, root).and_then(|r| cached.get(&r));
                if tx.send(scanner::load_metadata(t, entry)).is_err() {
                    return;
                }
            });
        }
        drop(tx);

        let mut pending: Vec<Track> = Vec::new();
        let mut last = Instant::now();
        let flush = |pending: &mut Vec<Track>| {
            if pending.is_empty() || !gen.live() {
                return;
            }
            lib.write().replace_tracks(pending);
            pending.clear();
            events(LibraryEvent::TracksChanged);
        };
        loop {
            match rx.recv_timeout(FLUSH_EVERY) {
                Ok(t) => pending.push(t),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
            if !gen.live() {
                return;
            }
            if pending.len() >= BATCH || last.elapsed() >= FLUSH_EVERY {
                flush(&mut pending);
                last = Instant::now();
            }
        }
        flush(&mut pending);
    });
    if !gen.live() {
        return;
    }

    let first = {
        let mut l = lib.write();
        let mut v = (*l.tracks).clone();
        sort_for_library(&mut v);
        l.set_tracks(v);
        !std::mem::replace(&mut l.has_completed_initial_load, true)
    };
    events(LibraryEvent::TracksChanged);
    if first {
        events(LibraryEvent::InitialLoadCompleted);
    }
    // Whole-library refinements after first paint.
    lib.write().finish_load();
    events(LibraryEvent::TracksChanged);
}

/// Cancels whatever scan / metadata job is running.
pub fn cancel_jobs(lib: &LibraryHandle) {
    lib.read().generation.fetch_add(1, Ordering::AcqRel);
}
