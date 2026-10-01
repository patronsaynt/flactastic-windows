//! A whole sync run over real TLS-PSK on localhost: hello, manifests, plan,
//! selection, file transfer, playlists (port of `SyncSessionTests` /
//! `FileTransferTests` core cases).

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, RwLock};

use fl_core::{AudioFileFormat, Playlist, PlaylistEntry, Track, Uid};
use fl_sync::builder;
use fl_sync::exchange::IncomingRequest;
use fl_sync::manifest::{SyncFilter, SyncSelection};
use fl_sync::pairing::PairingIdentity;
use fl_sync::protocol::Direction;
use fl_sync::session::{self, LocalContext, SessionError};
use fl_sync::tls::{self, ListenerKeys, SharedListenerKeys};
use fl_sync::{DeviceKind, Key, SyncConnection};
use tokio::net::TcpListener;

fn ident(name: &str) -> PairingIdentity {
    PairingIdentity { device_id: Uid::new_v4(), display_name: name.into(), kind: DeviceKind::Other }
}

fn track(root: &Path, rel: &str, body: &[u8], title: &str) -> Track {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, body).unwrap();
    let mut t = Track::new(p, title.into(), AudioFileFormat::classify(Path::new(rel)).unwrap());
    t.artist = Some("Artist".into());
    t
}

fn context(root: &Path, device: Uid, tracks: &[Track], playlists: Vec<Playlist>) -> LocalContext {
    let manifest = builder::build(tracks, &playlists, root, device, &SyncFilter::default(), |_| {}, &|| false).unwrap();
    LocalContext { root: root.to_path_buf(), manifest, filter: SyncFilter::default(), playlists }
}

async fn serve(keys: SharedListenerKeys) -> (std::net::SocketAddr, tokio::sync::mpsc::Receiver<SyncConnection>) {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    let ctx = tls::server_context(keys).unwrap();
    let (tx, rx) = tokio::sync::mpsc::channel(4);
    tokio::spawn(async move {
        while let Ok((tcp, _)) = l.accept().await {
            if let Ok(c) = SyncConnection::accept(tcp, &ctx).await {
                let _ = tx.send(c).await;
            }
        }
    });
    (addr, rx)
}

#[tokio::test(flavor = "multi_thread")]
async fn push_with_selection_conflict_and_playlist() {
    let (a_dir, b_dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (a_id, b_id) = (ident("A"), ident("B"));
    let key = Key([5; 32]);

    // A (initiator, pushing) has three tracks; B already has one with other tags.
    let mut a_tracks = vec![
        track(a_dir.path(), "Artist/LP/01 One.flac", b"one-bytes", "One"),
        track(a_dir.path(), "Artist/LP/02 Two.flac", &vec![7u8; 3 * 1024 * 1024 + 17], "Two"),
        track(a_dir.path(), "Artist/LP/03 Skip.flac", b"skip-me", "Skip"),
    ];
    let mut shared = track(b_dir.path(), "Artist/LP/01 One.flac", b"one-bytes", "One (old tags)");
    shared.id = a_tracks[0].id;
    a_tracks[0].genre = Some("Rock".into());
    let mut pl = Playlist::new("Road".into());
    pl.entries.push(PlaylistEntry::new(Some(a_tracks[1].id), "Artist/LP/02 Two.flac".into()));
    let a_ctx = context(a_dir.path(), a_id.device_id, &a_tracks, vec![pl.clone()]);
    let skip_id = a_tracks[2].id;

    let keys: SharedListenerKeys = Arc::new(RwLock::new(ListenerKeys::default()));
    keys.write().unwrap().paired.insert(a_id.device_id, key.clone());
    let (addr, mut rx) = serve(keys).await;

    let b_root = b_dir.path().to_path_buf();
    let b_tracks = vec![shared];
    let responder = {
        let (b_id, key, a_dev) = (b_id.clone(), key.clone(), a_id.device_id);
        async move {
            let conn = rx.recv().await.unwrap();
            let IncomingRequest::Sync(hello) = IncomingRequest::read(&conn).await.unwrap() else { panic!() };
            let cancelled = AtomicBool::new(false);
            session::run_responder(
                &conn,
                &hello,
                &b_id,
                |id| (id == a_dev).then(|| key.clone()),
                || async { Ok(context(&b_root, b_id.device_id, &b_tracks, vec![])) },
                &|_| {},
                &cancelled,
            )
            .await
        }
    };
    let initiator = async {
        let conn = SyncConnection::connect(addr, &tls::paired_client(key.clone(), a_id.device_id).unwrap()).await.unwrap();
        let cancelled = AtomicBool::new(false);
        let r = session::run_initiator(
            &conn,
            Direction::Push,
            &a_id,
            &key,
            a_ctx,
            |plan| async move {
                assert_eq!(plan.new_tracks.len(), 2);
                assert_eq!(plan.track_conflicts.len(), 1);
                assert_eq!(plan.track_conflicts[0].differing_fields, vec!["Title"]);
                assert_eq!(plan.new_playlists.len(), 1);
                // Untick "Skip".
                let keep: BTreeSet<Uid> =
                    plan.all_incoming_tracks().iter().map(|t| t.track_id).filter(|id| *id != skip_id).collect();
                Some(SyncSelection { track_ids: Some(keep), playlist_ids: None })
            },
            &|_| {},
            &cancelled,
        )
        .await;
        conn.close().await;
        r
    };
    let (sent, received) = tokio::join!(initiator, responder);
    let (sent, _) = sent.unwrap();
    let (got, landed) = received.unwrap();
    assert_eq!(sent.tracks_transferred, 2, "{sent:?}");
    assert_eq!(got.tracks_transferred, 2, "{got:?}");
    assert!(got.failures.is_empty() && sent.failures.is_empty());
    assert_eq!(landed.playlists.len(), 1);
    assert_eq!(landed.playlists[0].name, "Road");
    let two = b_dir.path().join("Artist/LP/02 Two.flac");
    assert_eq!(std::fs::read(&two).unwrap(), vec![7u8; 3 * 1024 * 1024 + 17]);
    assert!(!b_dir.path().join("Artist/LP/03 Skip.flac").exists(), "unticked track stays behind");
    assert_eq!(landed.files.iter().find(|f| f.destination.ends_with("02 Two.flac")).unwrap().track_id, a_tracks[1].id);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_changed_library_refuses_the_stale_plan() {
    // The responder's manifest changes between building the plan and the
    // decision: here the initiator lies about the hash instead.
    let (a_dir, b_dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (a_id, b_id) = (ident("A"), ident("B"));
    let key = Key([6; 32]);
    let a_tracks = vec![track(a_dir.path(), "x.flac", b"x", "X")];
    let a_ctx = context(a_dir.path(), a_id.device_id, &a_tracks, vec![]);
    let keys: SharedListenerKeys = Arc::new(RwLock::new(ListenerKeys::default()));
    keys.write().unwrap().paired.insert(a_id.device_id, key.clone());
    let (addr, mut rx) = serve(keys).await;
    let b_root = b_dir.path().to_path_buf();
    let responder = {
        let (b_id, key, a_dev) = (b_id.clone(), key.clone(), a_id.device_id);
        async move {
            let conn = rx.recv().await.unwrap();
            let IncomingRequest::Sync(hello) = IncomingRequest::read(&conn).await.unwrap() else { panic!() };
            session::run_responder(
                &conn,
                &hello,
                &b_id,
                |id| (id == a_dev).then(|| key.clone()),
                || async { Ok(context(&b_root, b_id.device_id, &[], vec![])) },
                &|_| {},
                &AtomicBool::new(false),
            )
            .await
        }
    };
    let initiator = async {
        let conn = SyncConnection::connect(addr, &tls::paired_client(key.clone(), a_id.device_id).unwrap()).await.unwrap();
        // Hand-rolled: a decision whose hash doesn't match.
        fl_sync::exchange::initiator_hello(&conn, &a_id, &key).await.unwrap();
        use fl_sync::wire::*;
        conn.send(&WireMessage::SyncRequest(SyncRequest { direction: Direction::Push, filter: SyncFilter::default(), manifest: a_ctx.manifest.clone() }))
            .await
            .unwrap();
        let _peer = conn.receive_message(std::time::Duration::from_secs(10)).await.unwrap();
        conn.send(&WireMessage::PlanDecision(PlanDecision { plan_hash: "stale".into(), approved: true, selection: None })).await.unwrap();
        conn.receive_message(std::time::Duration::from_secs(10)).await.unwrap()
    };
    let (reply, r) = tokio::join!(initiator, responder);
    assert!(matches!(r, Err(SessionError::PlanChanged)));
    assert!(matches!(reply, fl_sync::wire::WireMessage::ProtocolError(f) if f.code == fl_sync::wire::FailureCode::PlanStale));
}

async fn connected_pair() -> (SyncConnection, SyncConnection) {
    let id = Uid::new_v4();
    let key = Key([8; 32]);
    let keys: SharedListenerKeys = Arc::new(RwLock::new(ListenerKeys::default()));
    keys.write().unwrap().paired.insert(id, key.clone());
    let (addr, mut rx) = serve(keys).await;
    let client = SyncConnection::connect(addr, &tls::paired_client(key, id).unwrap()).await.unwrap();
    let server = rx.recv().await.unwrap();
    (client, server)
}

fn entry(root: &Path, rel: &str, body: &[u8]) -> fl_sync::manifest::TrackManifestEntry {
    let t = track(root, rel, body, rel);
    builder::build(&[t], &[], root, Uid::new_v4(), &SyncFilter::default(), |_| {}, &|| false).unwrap().tracks.remove(0)
}

/// MAC-ISSUES #6: a Mac receiver reports a bad checksum after fileEnd, where
/// the sender reads it as the reply to its next fileStart.
#[tokio::test(flavor = "multi_thread")]
async fn sender_charges_a_late_hash_mismatch_to_the_previous_file() {
    use fl_sync::wire::*;
    let src = tempfile::tempdir().unwrap();
    let (e1, e2) = (entry(src.path(), "a.flac", b"first"), entry(src.path(), "b.flac", b"second"));
    let (client, server) = connected_pair().await;
    let mac_receiver = async {
        // File 1: accept, drain, then complain after fileEnd.
        let WireMessage::FileStart(s1) = server.receive_message(std::time::Duration::from_secs(5)).await.unwrap() else { panic!() };
        server.send(&WireMessage::FileAccept(FileAccept { track_id: s1.track_id, resume_offset: 0, skip: false })).await.unwrap();
        loop {
            let f = server.receive_frame(std::time::Duration::from_secs(5)).await.unwrap();
            if f.ty == fl_sync::frame::FrameType::Control {
                break;
            }
        }
        server.send(&WireMessage::ProtocolError(ProtocolFailure::new(FailureCode::HashMismatch, "Checksum mismatch."))).await.unwrap();
        // File 2: skip it.
        let WireMessage::FileStart(s2) = server.receive_message(std::time::Duration::from_secs(5)).await.unwrap() else { panic!() };
        server.send(&WireMessage::FileAccept(FileAccept { track_id: s2.track_id, resume_offset: 0, skip: true })).await.unwrap();
    };
    let sender = async {
        let c = AtomicBool::new(false);
        let o1 = fl_sync::transfer::send(&e1, &src.path().join("a.flac"), &client, &c, |_| {}).await.unwrap();
        let o2 = fl_sync::transfer::send(&e2, &src.path().join("b.flac"), &client, &c, |_| {}).await.unwrap();
        (o1, o2)
    };
    let ((o1, o2), ()) = tokio::join!(sender, mac_receiver);
    assert_eq!((o1.sent, o1.previous_failed), (5, false));
    assert_eq!((o2.sent, o2.previous_failed), (0, true));
}

#[tokio::test(flavor = "multi_thread")]
async fn an_interrupted_transfer_resumes_from_the_part_file() {
    let (src, dst) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let body: Vec<u8> = (0..2_500_000u32).map(|i| (i % 251) as u8).collect();
    let e = entry(src.path(), "Artist/song.flac", &body);
    // The receiver already holds the first 1 MB.
    let incoming = dst.path().join(".flactastic").join("incoming");
    std::fs::create_dir_all(&incoming).unwrap();
    std::fs::write(incoming.join(format!("{}.part", e.track_id)), &body[..1_048_576]).unwrap();
    let (client, server) = connected_pair().await;
    let root = dst.path().to_path_buf();
    let receiver = async {
        let fl_sync::wire::WireMessage::FileStart(start) = server.receive_message(std::time::Duration::from_secs(5)).await.unwrap() else { panic!() };
        fl_sync::transfer::receive(
            &start,
            &server,
            &root,
            |rel| fl_sync::path_sanitizer::destination_for_incoming_track(rel, &root),
            |_, _| false,
            &AtomicBool::new(false),
            |_| {},
        )
        .await
        .unwrap()
        .unwrap()
    };
    let sender = async { fl_sync::transfer::send(&e, &src.path().join("Artist/song.flac"), &client, &AtomicBool::new(false), |_| {}).await.unwrap() };
    let (sent, got) = tokio::join!(sender, receiver);
    assert_eq!(sent.sent, body.len() as i64 - 1_048_576);
    assert_eq!(got.bytes_written, body.len() as i64);
    assert_eq!(std::fs::read(&got.destination).unwrap(), body);
    assert!(!incoming.join(format!("{}.part", e.track_id)).exists());
}
