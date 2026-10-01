//! App-wide state (`FlactasticApp`'s stores) and the events the UI listens to.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use fl_audio::output::Backend;
use fl_audio::output_manager::OutputSelection;
use fl_core::listening::ListeningStore;
use fl_core::playlist_store::PlaylistStore;
use fl_core::settings::Settings;
use fl_core::{Track, Uid};
use fl_library::{Library, LibraryEvent, LibraryHandle};
use fl_platform::AppDirs;
use parking_lot::{Mutex, RwLock};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::artists::Artists;
use crate::artwork::ArtworkStore;
use crate::player_actor::{self, Callbacks, PlayerHandle};

pub struct AppState {
    pub dirs: AppDirs,
    pub settings: Mutex<Settings>,
    pub library: LibraryHandle,
    /// The current library root, shared with the player (queue DTOs).
    pub root: Arc<RwLock<Option<PathBuf>>>,
    pub player: PlayerHandle,
    pub artwork: Arc<ArtworkStore>,
    pub listening: Arc<Mutex<ListeningStore>>,
    pub playlists: Mutex<PlaylistStore>,
    pub artists: Arc<Artists>,
    pub lyrics_cache: Mutex<fl_core::stores::LyricsRemoteCache>,
    pub highlight: Mutex<crate::home::Highlight>,
    pub staged: crate::import::Staged,
    pub lyrics: Arc<crate::lyrics::LyricsFetcher>,
    pub organizer_plan: crate::organizer::Plan,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryChanged {
    pub revision: u64,
    pub scan_state: fl_library::ScanState,
    pub has_completed_initial_load: bool,
}

fn backend() -> Arc<dyn Backend> {
    #[cfg(windows)]
    {
        fl_audio::output::wasapi::WasapiBackend::new()
    }
    #[cfg(target_os = "linux")]
    {
        fl_audio::output::alsa::AlsaBackend::new()
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        fl_audio::output::null::NullBackend::paced(48_000, 1.0)
    }
}

impl AppState {
    pub fn new(app: &AppHandle) -> Arc<AppState> {
        let dirs = AppDirs::resolve().expect("app directories");
        let settings = Settings::load(&dirs.config);
        let root: Arc<RwLock<Option<PathBuf>>> = Arc::default();
        let listening: Arc<Mutex<ListeningStore>> = Arc::default();

        let (a1, a2, a3, a4, a5) = (app.clone(), app.clone(), app.clone(), app.clone(), app.clone());
        let listening2 = listening.clone();
        let callbacks = Callbacks {
            state: Box::new(move |snap| {
                crate::media_controls::publish(&snap);
                crate::tray::on_player_state(&a1, snap.is_playing);
                let _ = a1.emit("player://state", snap);
            }),
            plays: Box::new(move |plays| {
                let mut l = listening2.lock();
                let mut changed = false;
                for p in plays {
                    changed |= l.record(&p.track, p.started_at, p.seconds_listened, p.counted);
                }
                if changed {
                    if let Some((path, bytes)) = l.snapshot() {
                        std::thread::spawn(move || {
                            let _ = fl_core::apple_json::write_atomic(&path, &bytes);
                        });
                    }
                    let _ = a2.emit("listening://changed", ());
                }
            }),
            output_selection: Box::new(move |sel: OutputSelection, status| {
                use tauri::Manager;
                if let Some(st) = a3.try_state::<Arc<AppState>>() {
                    let mut s = st.settings.lock();
                    s.output_device_uid = sel.device_id;
                    s.output_sample_rate = sel.sample_rate;
                    s.output_bit_depth = sel.bit_depth;
                    s.output_exclusive_mode = sel.exclusive;
                    let _ = s.save(&st.dirs.config);
                }
                let _ = a3.emit("output://status", status);
            }),
            spectrum: Arc::new(move |frame| {
                let _ = a4.emit("spectrum://frame", frame.to_vec());
            }),
            volume: Box::new(move |v| {
                use tauri::Manager;
                if let Some(st) = a5.try_state::<Arc<AppState>>() {
                    let mut s = st.settings.lock();
                    s.volume = v;
                    let _ = s.save(&st.dirs.config);
                }
            }),
        };
        let player = player_actor::spawn(
            player_actor::Init {
                backend: backend(),
                selection: OutputSelection {
                    device_id: settings.output_device_uid.clone(),
                    sample_rate: settings.output_sample_rate,
                    bit_depth: settings.output_bit_depth,
                    exclusive: settings.output_exclusive_mode && cfg!(windows),
                },
                volume: settings.volume,
                counted_play_fraction: settings.counted_play_fraction,
                root: root.clone(),
            },
            callbacks,
        );

        let a6 = app.clone();
        let artists = Artists::new(&dirs.data, Arc::new(move || {
            let _ = a6.emit("artists://changed", ());
        }));

        let mut lyrics_cache = fl_core::stores::LyricsRemoteCache::new(&dirs.data);
        lyrics_cache.load();

        Arc::new(AppState {
            artists,
            lyrics_cache: Mutex::new(lyrics_cache),
            highlight: Mutex::default(),
            staged: Default::default(),
            lyrics: crate::lyrics::LyricsFetcher::new(app.clone()),
            organizer_plan: Default::default(),
            artwork: Arc::new(ArtworkStore::new(dirs.cache.clone())),
            dirs,
            settings: Mutex::new(settings),
            library: Library::handle(Arc::new(fl_platform::NativeFileIdentity)),
            root,
            player,
            listening,
            playlists: Mutex::new(PlaylistStore::new()),
        })
    }

    pub fn save_settings(&self) {
        if let Err(e) = self.settings.lock().save(&self.dirs.config) {
            log::warn!("[Settings] save failed: {e}");
        }
    }

    /// `bootstrap()` / `openFolder`: point every per-library store at `root`.
    pub fn open_library(self: &Arc<Self>, app: &AppHandle, root: PathBuf) {
        // Commit the in-flight listen to the outgoing library first.
        self.player.send(player_actor::Cmd::FlushPending);
        *self.root.write() = Some(root.clone());
        self.playlists.lock().load(&root);
        self.listening.lock().load(&root);
        let _ = app.emit("playlists://changed", ());
        let _ = app.emit("listening://changed", ());
        let me = self.clone();
        let app2 = app.clone();
        fl_library::open_folder(&self.library, root, Arc::new(move |e| me.on_library_event(&app2, e)));
    }

    pub fn refresh_library(self: &Arc<Self>, app: &AppHandle) {
        let me = self.clone();
        let app2 = app.clone();
        fl_library::refresh(&self.library, Arc::new(move |e| me.on_library_event(&app2, e)));
    }

    fn on_library_event(&self, app: &AppHandle, e: LibraryEvent) {
        let (payload, done) = {
            let mut l = self.library.write();
            if e == LibraryEvent::TracksChanged {
                let tracks = l.tracks();
                let albums = l.albums();
                self.artwork.register_library(&tracks, &albums);
            }
            (
                LibraryChanged {
                    revision: l.revision(),
                    scan_state: l.scan_state.clone(),
                    has_completed_initial_load: l.has_completed_initial_load,
                },
                matches!(l.scan_state, fl_library::ScanState::Done { .. }),
            )
        };
        if e == LibraryEvent::ScanStateChanged && done {
            // `reconcile` after every completed scan.
            let tracks = self.library.read().tracks();
            if let Some(root) = self.root.read().clone() {
                let mut p = self.playlists.lock();
                p.reconcile(&tracks, &root);
                let _ = app.emit("playlists://changed", ());
            }
        }
        if e == LibraryEvent::InitialLoadCompleted {
            let albums = self.library.write().albums();
            self.artwork.prewarm((*albums).clone(), 2.0);
            self.prefetch_artist_images();
        }
        let _ = app.emit("library://changed", payload);
    }

    /// Library tracks for these IDs, in the given order (missing ones skipped).
    pub fn tracks_by_id(&self, ids: &[String]) -> Vec<Track> {
        let l = self.library.read();
        let tracks = l.tracks();
        let by_id: std::collections::HashMap<Uid, &Track> = tracks.iter().map(|t| (t.id, t)).collect();
        ids.iter().filter_map(|s| Uid::parse(s)).filter_map(|id| by_id.get(&id).map(|t| (*t).clone())).collect()
    }

    /// `prefetchArtistImages`: warm every artist's picture after the first load.
    pub fn prefetch_artist_images(&self) {
        if !self.settings.lock().auto_fetch_artist_images {
            return;
        }
        let (tracks, albums) = {
            let mut l = self.library.write();
            (l.tracks(), l.albums())
        };
        for s in self.artists.summaries(&tracks, &albums) {
            self.artists.ensure_image(&s.id, &s.display_name);
        }
    }

    /// `library://changed` after an in-place edit (not a scan).
    pub fn emit_library_changed(&self, app: &AppHandle) {
        let l = self.library.read();
        let _ = app.emit(
            "library://changed",
            LibraryChanged {
                revision: l.revision(),
                scan_state: l.scan_state.clone(),
                has_completed_initial_load: l.has_completed_initial_load,
            },
        );
    }

    pub fn root_path(&self) -> Option<PathBuf> {
        self.root.read().clone()
    }
}

pub fn path_exists(p: &Path) -> bool {
    p.is_dir()
}
