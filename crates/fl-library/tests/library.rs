//! Scan → stable IDs → metadata load → sidecars, reopen and refresh.

use std::path::Path;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use fl_core::Uid;
use fl_library::{open_folder, refresh, Library, LibraryEvent, LibraryHandle, ScanState};
use fl_platform::identity::NativeFileIdentity;

fn write_flac(path: &Path, seconds: f64, rate: usize) {
    use flacenc::component::BitRepr;
    use flacenc::error::Verify;
    let frames = (seconds * rate as f64) as usize;
    let samples: Vec<i32> = (0..frames * 2).map(|i| ((i as f64 * 0.01).sin() * 8000.0) as i32).collect();
    let cfg = flacenc::config::Encoder::default().into_verified().unwrap();
    let src = flacenc::source::MemSource::from_samples(&samples, 2, 16, rate);
    let stream = flacenc::encode_with_fixed_block_size(&cfg, src, cfg.block_size).unwrap();
    let mut sink = flacenc::bitsink::ByteSink::new();
    stream.write(&mut sink).unwrap();
    let mut bytes = sink.as_slice().to_vec();
    bytes.copy_within(10..12, 8);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

fn tag(path: &Path, title: &str, artist: &str, album: &str, n: u32) {
    let mut f = fl_tags::TagFile::open(path).unwrap();
    f.set_tag(0, title);
    f.set_tag(1, artist);
    f.set_tag(2, album);
    f.set_track(n);
    assert!(f.save());
}

fn wait_initial(lib: &LibraryHandle, rx: &mpsc::Receiver<LibraryEvent>) {
    loop {
        match rx.recv_timeout(Duration::from_secs(30)).expect("library load timed out") {
            LibraryEvent::InitialLoadCompleted => break,
            _ => {}
        }
    }
    // finish_load runs right after; wait for its TracksChanged.
    while rx.recv_timeout(Duration::from_millis(300)).is_ok() {}
    assert!(lib.read().has_completed_initial_load);
}

fn events() -> (fl_library::LibraryEvents, mpsc::Receiver<LibraryEvent>) {
    let (tx, rx) = mpsc::channel();
    let tx = parking_lot::Mutex::new(tx);
    (Arc::new(move |e| {
        let _ = tx.lock().send(e);
    }), rx)
}

#[test]
fn scan_load_reopen_refresh() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let a = root.join("Artist/Album/01 One.flac");
    let b = root.join("Artist/Album/02 Two.flac");
    write_flac(&a, 1.5, 44_100);
    write_flac(&b, 2.0, 96_000);
    tag(&a, "One", "Artist", "Album", 1);
    tag(&b, "Two", "Artist", "Album", 2);
    // Skipped: AppleDouble, the sidecar folder, dot-folders.
    std::fs::write(root.join("Artist/Album/._01 One.flac"), b"junk").unwrap();
    std::fs::create_dir_all(root.join(".flactastic")).unwrap();
    write_flac(&root.join(".hidden/x.flac"), 0.5, 44_100);

    let lib = Library::handle(Arc::new(NativeFileIdentity));
    let (ev, rx) = events();
    open_folder(&lib, root.clone(), ev.clone());
    wait_initial(&lib, &rx);

    let ids: Vec<Uid> = {
        let mut l = lib.write();
        assert_eq!(l.scan_state, ScanState::Done { count: 2 });
        let tracks = l.tracks();
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0].title, "One");
        assert_eq!(tracks[1].title, "Two");
        assert!((tracks[0].duration.unwrap() - 1.5).abs() < 1e-6);
        assert_eq!(tracks[1].sample_rate, Some(96_000.0));
        assert_eq!(tracks[1].bit_depth, Some(16));
        let albums = l.albums();
        assert_eq!(albums.len(), 1);
        assert_eq!(albums[0].id, "Artist|Album");
        tracks.iter().map(|t| t.id).collect()
    };
    assert!(root.join(".flactastic/track-ids.json").exists());
    // The metadata sidecar is written on a background thread.
    let cache = root.join(".flactastic/metadata-cache.json");
    for _ in 0..100 {
        if cache.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(cache.exists());

    // Reopen with a fresh store: same IDs, and metadata from the cache.
    let lib2 = Library::handle(Arc::new(NativeFileIdentity));
    let (ev2, rx2) = events();
    open_folder(&lib2, root.clone(), ev2.clone());
    wait_initial(&lib2, &rx2);
    let ids2: Vec<Uid> = lib2.read().tracks().iter().map(|t| t.id).collect();
    assert_eq!(ids, ids2);

    // Refresh picks up a new file and keeps the old tracks' state.
    let c = root.join("Artist/Album/03 Three.flac");
    write_flac(&c, 1.0, 48_000);
    tag(&c, "Three", "Artist", "Album", 3);
    refresh(&lib2, ev2);
    for _ in 0..200 {
        let done = lib2.read().tracks().iter().any(|t| t.title == "Three");
        if done {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let l = lib2.read();
    let tracks = l.tracks();
    assert_eq!(tracks.len(), 3);
    assert!(tracks.iter().any(|t| t.title == "Three" && t.sample_rate == Some(48_000.0)));
    assert!(ids.iter().all(|id| tracks.iter().any(|t| t.id == *id)));
}

#[test]
fn missing_root_fails_and_resolves_initial_load() {
    let lib = Library::handle(Arc::new(NativeFileIdentity));
    let (ev, rx) = events();
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("gone");
    open_folder(&lib, missing.clone(), ev);
    wait_initial(&lib, &rx);
    let state = lib.read().scan_state.clone();
    assert!(matches!(state, ScanState::Failed { .. }), "{state:?}");
    assert!(!missing.exists(), "opening must not create the root");
}
