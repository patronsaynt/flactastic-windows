//! Interop harness for sync protocol v1 against a real Mac or iPhone.
//!
//! ```text
//! fl-conformance browse [secs]             list _flactastic._tcp peers
//! fl-conformance pair <peer> <code>        pair as guest with a device showing a code
//! fl-conformance host-pair [secs]          show a code and wait for a guest (Mac: Pair → type it)
//! fl-conformance hello <peer>              paired dial: hello(proof) → helloAck
//! fl-conformance listen [secs]             accept a paired hello, answer helloAck,
//!                                          save the peer's syncRequest, then cancel
//! fl-conformance peers | forget <id>
//! ```
//! `<peer>` is a device ID (resolved over Bonjour) or `host:port`.
//! Keys go to the OS keyring under `conformance:<id>`; nothing touches a library.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use fl_core::Uid;
use fl_sync::exchange::{self, IncomingRequest};
use fl_sync::gatekeeper::PairingGatekeeper;
use fl_sync::pairing::{GuestPairingSession, PairedPeer, PairingIdentity};
use fl_sync::tls::{self, ListenerKeys, SharedListenerKeys};
use fl_sync::{protocol, txt, DeviceKind, Key, SyncConnection, WireMessage};
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use serde::{Deserialize, Serialize};

const SERVICE: &str = "_flactastic._tcp.local.";

#[derive(Serialize, Deserialize)]
struct State {
    device_id: Uid,
    display_name: String,
    peers: Vec<PairedPeer>,
}

fn state_path() -> PathBuf {
    let dirs = fl_platform::AppDirs::resolve().expect("app dirs");
    dirs.data.join("conformance-state.json")
}

fn load_state() -> State {
    fl_core::apple_json::load(&state_path()).ok().flatten().unwrap_or_else(|| State {
        device_id: Uid::new_v4(),
        display_name: format!("{} (conformance)", hostname()),
        peers: vec![],
    })
}

fn save_state(s: &State) {
    fl_core::apple_json::save(&state_path(), s).expect("save state");
}

fn hostname() -> String {
    std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_else(|_| "PC".into())
}

fn identity(s: &State) -> PairingIdentity {
    PairingIdentity { device_id: s.device_id, display_name: s.display_name.clone(), kind: DeviceKind::LOCAL }
}

fn key_account(id: Uid) -> String {
    format!("conformance:{id}")
}

fn load_key(id: Uid) -> Option<Key> {
    fl_platform::secrets::get(&key_account(id)).ok().flatten().and_then(|b| Key::from_slice(&b))
}

// MARK: - Discovery

struct Found {
    peer: txt::DiscoveredPeer,
    addrs: Vec<SocketAddr>,
}

fn browse(secs: u64) -> Vec<Found> {
    let mdns = ServiceDaemon::new().expect("mdns daemon");
    let rx = mdns.browse(SERVICE).expect("browse");
    let deadline = std::time::Instant::now() + Duration::from_secs(secs);
    let mut out: HashMap<Uid, Found> = HashMap::new();
    while let Some(left) = deadline.checked_duration_since(std::time::Instant::now()) {
        let Ok(ev) = rx.recv_timeout(left) else { break };
        if let ServiceEvent::ServiceResolved(info) = ev {
            let props: HashMap<String, String> =
                info.get_properties().iter().map(|p| (p.key().to_owned(), p.val_str().to_owned())).collect();
            let Some(peer) = txt::decode(&props) else { continue };
            let mut addrs: Vec<SocketAddr> =
                info.get_addresses().iter().map(|ip| SocketAddr::new(*ip, info.get_port())).collect();
            addrs.sort_by_key(|a| !a.is_ipv4());
            out.insert(peer.device_id, Found { peer, addrs });
        }
    }
    let _ = mdns.shutdown();
    out.into_values().collect()
}

fn resolve(target: &str) -> SocketAddr {
    if let Ok(a) = target.parse::<SocketAddr>() {
        return a;
    }
    let id = Uid::parse(target).unwrap_or_else(|| die("peer must be a device ID or host:port"));
    let found = browse(5);
    let f = found.iter().find(|f| f.peer.device_id == id).unwrap_or_else(|| die("peer not found on the network"));
    *f.addrs.first().unwrap_or_else(|| die("peer has no address"))
}

fn advertise(s: &State, port: u16, pairing_open: bool) -> ServiceDaemon {
    let mdns = ServiceDaemon::new().expect("mdns daemon");
    let props: HashMap<String, String> = txt::encode(s.device_id, &s.display_name, DeviceKind::LOCAL, pairing_open).into_iter().collect();
    let host = format!("{}.local.", hostname().to_lowercase());
    let ips: Vec<IpAddr> = local_ips();
    let info = ServiceInfo::new(SERVICE, &s.display_name, &host, &ips[..], port, props).expect("service info");
    mdns.register(info).expect("register");
    mdns
}

fn local_ips() -> Vec<IpAddr> {
    // A UDP "connect" picks the outbound interface without sending anything.
    let s = std::net::UdpSocket::bind("0.0.0.0:0").expect("udp");
    let _ = s.connect("192.0.2.1:9");
    vec![s.local_addr().map(|a| a.ip()).unwrap_or(IpAddr::from([127, 0, 0, 1]))]
}

// MARK: - Commands

fn die(msg: &str) -> ! {
    eprintln!("error: {msg}");
    std::process::exit(2)
}

async fn cmd_pair(target: &str, code: &str) {
    let mut s = load_state();
    let addr = resolve(target);
    let id = identity(&s);
    let session = GuestPairingSession::new(code, id.clone()).unwrap_or_else(|e| die(&e.to_string()));
    let conn = SyncConnection::connect(addr, &tls::pairing_client().unwrap())
        .await
        .unwrap_or_else(|e| die(&format!("{e} (is a pairing code showing on the other device?)")));
    println!("TLS: {:?}", conn.negotiated);
    let saved: Arc<std::sync::Mutex<Option<(PairedPeer, Key)>>> = Default::default();
    let s2 = saved.clone();
    let r = exchange::run_guest(
        &conn,
        session,
        &id,
        |peer, key| async move {
            fl_platform::secrets::set(&key_account(peer.device_id), key.as_bytes())?;
            *s2.lock().unwrap() = Some((peer, key));
            Ok(())
        },
        |peer_id| async move {
            let _ = fl_platform::secrets::delete(&key_account(peer_id));
        },
    )
    .await;
    conn.close().await;
    match r {
        Ok(peer) => {
            println!("paired with {} ({}, {})", peer.display_name, peer.device_id, peer.kind.raw_value());
            s.peers.retain(|p| p.device_id != peer.device_id);
            s.peers.push(peer);
            save_state(&s);
        }
        Err(e) => die(&format!("pairing failed: {e}")),
    }
}

async fn serve(pairing: bool, secs: u64) {
    let mut s = load_state();
    let id = identity(&s);
    let keys: SharedListenerKeys = Arc::new(RwLock::new(ListenerKeys::default()));
    for p in &s.peers {
        if let Some(k) = load_key(p.device_id) {
            keys.write().unwrap().paired.insert(p.device_id, k);
        }
    }
    let mut gate = PairingGatekeeper::default();
    let code = if pairing { gate.open_pairing() } else { None };
    keys.write().unwrap().allow_pairing = code.is_some();

    let listener = tokio::net::TcpListener::bind("0.0.0.0:0").await.expect("bind");
    let port = listener.local_addr().unwrap().port();
    let _mdns = advertise(&s, port, code.is_some());
    println!("advertising \"{}\" ({}) on port {port}", s.display_name, s.device_id);
    if let Some(c) = &code {
        println!("PAIRING CODE: {} {}", &c[..4], &c[4..]);
    }
    let ctx = tls::server_context(keys.clone()).unwrap();

    let deadline = tokio::time::Instant::now() + Duration::from_secs(secs);
    loop {
        let accepted = tokio::select! {
            a = listener.accept() => a,
            _ = tokio::time::sleep_until(deadline) => { println!("timed out"); return; }
        };
        let (tcp, from) = accepted.expect("accept");
        println!("connection from {from}");
        let conn = match SyncConnection::accept(tcp, &ctx).await {
            Ok(c) => c,
            Err(e) => {
                println!("  handshake failed: {e}");
                continue;
            }
        };
        println!("  TLS: {:?}", conn.negotiated);
        match IncomingRequest::read(&conn).await {
            Ok(IncomingRequest::Pairing(h)) => {
                println!("  pairing request from {} ({})", h.display_name, h.device_id);
                let saved: Arc<std::sync::Mutex<Option<PairedPeer>>> = Default::default();
                let s2 = saved.clone();
                let r = exchange::run_host(&conn, &h, gate.current_code(), &id, |peer, key| async move {
                    fl_platform::secrets::set(&key_account(peer.device_id), key.as_bytes())?;
                    *s2.lock().unwrap() = Some(peer);
                    Ok(())
                })
                .await;
                match r {
                    Ok(peer) => {
                        gate.record_success();
                        keys.write().unwrap().allow_pairing = false;
                        println!("  paired with {} ({})", peer.display_name, peer.device_id);
                        s.peers.retain(|p| p.device_id != peer.device_id);
                        s.peers.push(peer);
                        save_state(&s);
                        return;
                    }
                    Err(e) => {
                        gate.record_failure();
                        keys.write().unwrap().allow_pairing = false;
                        println!("  pairing failed: {e}");
                        return;
                    }
                }
            }
            Ok(IncomingRequest::Sync(h)) => {
                println!("  paired hello from {} ({}), proof present: {}", h.display_name, h.device_id, h.proof.is_some());
                let r = exchange::responder_hello(&conn, &h, &id, load_key).await;
                println!("  responder hello: {r:?}");
                if r.is_ok() {
                    match conn.receive_message(protocol::MANIFEST_WAIT_TIMEOUT).await {
                        Ok(m) => {
                            let out = state_path().with_file_name(format!("captured-{}.json", m.tag()));
                            std::fs::write(&out, m.encoded()).unwrap();
                            println!("  received {} — saved to {}", m.tag(), out.display());
                        }
                        Err(e) => println!("  after helloAck: {e}"),
                    }
                    let _ = conn.send(&WireMessage::Cancel(fl_sync::wire::Cancellation { reason: "Conformance test finished.".into() })).await;
                }
                conn.close().await;
            }
            Err(e) => println!("  bad opening: {e}"),
        }
    }
}

async fn cmd_hello(target: &str) {
    let s = load_state();
    let addr = resolve(target);
    let (peer_id, key) = s
        .peers
        .iter()
        .find_map(|p| load_key(p.device_id).map(|k| (p.device_id, k)))
        .filter(|(id, _)| Uid::parse(target).is_none_or(|t| t == *id))
        .unwrap_or_else(|| die("not paired with that peer"));
    let conn = SyncConnection::connect(addr, &tls::paired_client(key.clone(), s.device_id).unwrap())
        .await
        .unwrap_or_else(|e| die(&e.to_string()));
    println!("TLS: {:?}", conn.negotiated);
    match exchange::initiator_hello(&conn, &identity(&s), &key).await {
        Ok(ack) => println!("helloAck from {} ({}, v{}) — peer {peer_id}", ack.display_name, ack.device_id, ack.version),
        Err(e) => die(&format!("hello failed: {e}")),
    }
    let _ = conn.send(&WireMessage::Cancel(fl_sync::wire::Cancellation { reason: "Conformance test finished.".into() })).await;
    conn.close().await;
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |i: usize| args.get(i).map(String::as_str);
    let secs = |i: usize, d: u64| arg(i).and_then(|s| s.parse().ok()).unwrap_or(d);
    match arg(0) {
        Some("browse") => {
            for f in browse(secs(1, 5)) {
                let p = &f.peer;
                println!(
                    "{}  {:<28} kind={:<6} v{} pairing={} {:?}",
                    p.device_id, p.display_name, p.kind.raw_value(), p.protocol_version, p.is_pairing_open, f.addrs
                );
            }
        }
        Some("pair") => cmd_pair(arg(1).unwrap_or_else(|| die("peer?")), arg(2).unwrap_or_else(|| die("code?"))).await,
        Some("host-pair") => serve(true, secs(1, 120)).await,
        Some("listen") => serve(false, secs(1, 600)).await,
        Some("hello") => cmd_hello(arg(1).unwrap_or_else(|| die("peer?"))).await,
        Some("peers") => {
            let s = load_state();
            println!("this device: {} ({})", s.display_name, s.device_id);
            for p in s.peers {
                let has = load_key(p.device_id).is_some();
                println!("{}  {}  paired {:?}  key:{}", p.device_id, p.display_name, p.paired_at.to_chrono_utc(), has);
            }
        }
        Some("forget") => {
            let mut s = load_state();
            let id = Uid::parse(arg(1).unwrap_or_default()).unwrap_or_else(|| die("device id?"));
            let _ = fl_platform::secrets::delete(&key_account(id));
            s.peers.retain(|p| p.device_id != id);
            save_state(&s);
        }
        _ => {
            eprintln!("usage: fl-conformance browse|pair|host-pair|hello|listen|peers|forget (see source header)");
            std::process::exit(64);
        }
    }
}
