//! Pairing and the paired handshake over real TLS-PSK on localhost
//! (port of `SyncPairingLoopbackTests.swift`'s core cases).

use std::sync::{Arc, Mutex, RwLock};

use fl_core::Uid;
use fl_sync::exchange::{self, HelloError, IncomingRequest};
use fl_sync::pairing::{GuestPairingSession, PairingError, PairingIdentity};
use fl_sync::tls::{self, ListenerKeys, SharedListenerKeys};
use fl_sync::{DeviceKind, Key, SyncConnection};
use tokio::net::TcpListener;

fn ident(name: &str) -> PairingIdentity {
    PairingIdentity { device_id: Uid::new_v4(), display_name: name.into(), kind: DeviceKind::Other }
}

async fn listener(keys: SharedListenerKeys) -> (std::net::SocketAddr, tokio::sync::mpsc::Receiver<SyncConnection>) {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    let ctx = tls::server_context(keys).unwrap();
    let (tx, rx) = tokio::sync::mpsc::channel(4);
    tokio::spawn(async move {
        loop {
            let (tcp, _) = l.accept().await.unwrap();
            let ctx = ctx.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                if let Ok(c) = SyncConnection::accept(tcp, &ctx).await {
                    let _ = tx.send(c).await;
                }
            });
        }
    });
    (addr, rx)
}

async fn pair(code_on_host: &str, typed: &str) -> (Result<Key, String>, Result<Key, String>, PairingIdentity, PairingIdentity, SharedListenerKeys, std::net::SocketAddr, tokio::sync::mpsc::Receiver<SyncConnection>) {
    let keys: SharedListenerKeys = Arc::new(RwLock::new(ListenerKeys { allow_pairing: true, ..Default::default() }));
    let (addr, mut rx) = listener(keys.clone()).await;
    let (host_id, guest_id) = (ident("Host PC"), ident("Guest PC"));

    let host = {
        let host_id = host_id.clone();
        let code = code_on_host.to_owned();
        let saved = Arc::new(Mutex::new(None));
        async move {
            let conn = rx.recv().await.unwrap();
            let IncomingRequest::Pairing(h) = IncomingRequest::read(&conn).await.unwrap() else { panic!("expected pairing") };
            let s2 = saved.clone();
            let r = exchange::run_host(&conn, &h, Some(code), &host_id, |_peer, key| async move {
                *s2.lock().unwrap() = Some(key);
                Ok(())
            })
            .await;
            let out = r.map(|_| saved.lock().unwrap().clone().unwrap()).map_err(|e| e.to_string());
            (out, rx)
        }
    };
    let guest = {
        let guest_id = guest_id.clone();
        let typed = typed.to_owned();
        async move {
            let conn = SyncConnection::connect(addr, &tls::pairing_client().unwrap()).await.unwrap();
            assert_eq!(conn.negotiated.version, "TLSv1.3");
            let session = GuestPairingSession::new(&typed, guest_id.clone()).unwrap();
            let saved = Arc::new(Mutex::new(None));
            let s2 = saved.clone();
            let r = exchange::run_guest(&conn, session, &guest_id, |_p, k| async move {
                *s2.lock().unwrap() = Some(k);
                Ok(())
            }, |_| async {})
            .await;
            conn.close().await;
            r.map(|_| saved.lock().unwrap().clone().unwrap()).map_err(|e| e.to_string())
        }
    };
    let ((h, rx), g) = tokio::join!(host, guest);
    (h, g, host_id, guest_id, keys, addr, rx)
}

#[tokio::test(flavor = "multi_thread")]
async fn pair_then_paired_hello_both_ways() {
    let (h, g, host_id, guest_id, keys, addr, mut rx) = pair("31415926", "3141-5926").await;
    let (hk, gk) = (h.unwrap(), g.unwrap());
    assert_eq!(hk, gk, "both sides hold the same long-term key");

    // Host registers the guest's key; pairing closes.
    {
        let mut k = keys.write().unwrap();
        k.paired.insert(guest_id.device_id, hk.clone());
        k.allow_pairing = false;
    }

    let dial = async {
        let conn = SyncConnection::connect(addr, &tls::paired_client(gk.clone(), guest_id.device_id).unwrap()).await.unwrap();
        let ack = exchange::initiator_hello(&conn, &guest_id, &gk).await.unwrap();
        (conn.negotiated.clone(), ack)
    };
    let answer = async {
        let conn = rx.recv().await.unwrap();
        let IncomingRequest::Sync(h) = IncomingRequest::read(&conn).await.unwrap() else { panic!() };
        exchange::responder_hello(&conn, &h, &host_id, |id| (id == guest_id.device_id).then(|| hk.clone())).await.unwrap();
        conn.negotiated.clone()
    };
    let ((client_neg, ack), server_neg) = tokio::join!(dial, answer);
    assert_eq!(ack.device_id, host_id.device_id);
    assert_eq!(client_neg.version, "TLSv1.3");
    assert_eq!(client_neg.cipher, "TLS_AES_128_GCM_SHA256");
    assert_eq!(client_neg, server_neg);
    eprintln!("negotiated: {client_neg:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn wrong_code_fails_both_sides() {
    let (h, g, ..) = pair("11111111", "22222222").await;
    assert!(h.is_err());
    let g = g.unwrap_err();
    assert!(g.contains("rejected") || g.contains("closed") || g.contains("didn't match"), "{g}");
}

#[tokio::test(flavor = "multi_thread")]
async fn revoked_peer_cannot_handshake() {
    let keys: SharedListenerKeys = Arc::new(RwLock::new(ListenerKeys::default()));
    let (addr, _rx) = listener(keys).await;
    let id = Uid::new_v4();
    let err = SyncConnection::connect(addr, &tls::paired_client(Key([7; 32]), id).unwrap()).await;
    assert!(err.is_err(), "unknown PSK identity must fail the TLS handshake");
}

#[tokio::test(flavor = "multi_thread")]
async fn pairing_psk_cannot_pose_as_paired_peer() {
    let victim = ident("Victim");
    let real_key = Key([9; 32]);
    let keys: SharedListenerKeys = Arc::new(RwLock::new(ListenerKeys { allow_pairing: true, ..Default::default() }));
    keys.write().unwrap().paired.insert(victim.device_id, real_key.clone());
    let (addr, mut rx) = listener(keys).await;
    let host = ident("Host");

    let attacker = async {
        let conn = SyncConnection::connect(addr, &tls::pairing_client().unwrap()).await.unwrap();
        // Claims the victim's ID but can only MAC with a key it doesn't hold.
        exchange::initiator_hello(&conn, &victim, &Key([1; 32])).await
    };
    let answer = async {
        let conn = rx.recv().await.unwrap();
        let IncomingRequest::Sync(h) = IncomingRequest::read(&conn).await.unwrap() else { panic!() };
        exchange::responder_hello(&conn, &h, &host, |_| Some(real_key.clone())).await
    };
    let (a, r) = tokio::join!(attacker, answer);
    assert!(matches!(r, Err(HelloError::NotAuthenticated)));
    assert!(matches!(a, Err(HelloError::RefusedByPeer(_))));
}

#[test]
fn malformed_code_never_dials() {
    assert_eq!(GuestPairingSession::new("12-34", ident("g")).err(), Some(PairingError::MalformedCode));
}
