//! `SyncModel`: discovery, pairing and sync runs behind the Sync UI.
//!
//! Networking (the TLS listener, mDNS advertising and browsing) runs only
//! while the Sync screen or Settings ▸ Devices is on screen (reference
//! counted, as on the Mac), and the advertiser idles out after 10 minutes
//! without activity. Keys live in the OS keyring; `sync-peers.json` holds only
//! names, dates and filters.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use fl_core::apple_json::{self, AppleDate};
use fl_core::model::relative_path;
use fl_core::{Playlist, Uid};
use fl_sync::builder;
use fl_sync::crypto::{is_well_formed_code, normalize_typed_code};
use fl_sync::discovery::{Advertiser, Browser, Found};
use fl_sync::exchange::{self, IncomingRequest};
use fl_sync::gatekeeper::PairingGatekeeper;
use fl_sync::manifest::{SyncFilter, SyncPlan, SyncSelection};
use fl_sync::pairing::{GuestPairingSession, PairedPeer, PairingError, PairingIdentity};
use fl_sync::picklist::{self, PickList};
use fl_sync::protocol::{DeviceKind, Direction};
use fl_sync::session::{self, LocalContext, Progress, SessionError, Summary};
use fl_sync::tls::{self, ListenerKeys, SharedListenerKeys};
use fl_sync::{Key, SyncConnection};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::oneshot;

use crate::state::AppState;

const ADVERTISER_IDLE: Duration = Duration::from_secs(600);

// MARK: - Peer store

#[derive(Clone, Serialize, Deserialize)]
struct Record {
    peer: PairedPeer,
    #[serde(default)]
    filter: SyncFilter,
}

/// `SyncPeerStore`: which devices this machine paired with (no key material).
struct PeerStore {
    path: PathBuf,
}

impl PeerStore {
    fn load(&self) -> HashMap<Uid, Record> {
        apple_json::load::<HashMap<String, Record>>(&self.path)
            .ok()
            .flatten()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(k, v)| Uid::parse(&k).map(|id| (id, v)))
            .collect()
    }

    fn save(&self, records: &HashMap<Uid, Record>) {
        let keyed: HashMap<String, &Record> = records.iter().map(|(k, v)| (k.to_string(), v)).collect();
        if let Err(e) = apple_json::save(&self.path, &keyed) {
            log::warn!("[SyncPeerStore] Failed to save: {e}");
        }
    }

    fn peers(&self) -> Vec<PairedPeer> {
        let mut v: Vec<PairedPeer> = self.load().into_values().map(|r| r.peer).collect();
        v.sort_by(|a, b| fl_core::text::standard_compare(&a.display_name, &b.display_name));
        v
    }

    /// Keeps a re-paired device's filter and last-sync date.
    fn upsert(&self, peer: PairedPeer) {
        let mut r = self.load();
        let prev = r.get(&peer.device_id).cloned();
        let mut peer = peer;
        peer.last_synced_at = prev.as_ref().and_then(|p| p.peer.last_synced_at);
        r.insert(peer.device_id, Record { peer, filter: prev.map(|p| p.filter).unwrap_or_default() });
        self.save(&r);
    }

    fn remove(&self, id: Uid) {
        let mut r = self.load();
        r.remove(&id);
        self.save(&r);
    }

    fn record_sync(&self, id: Uid) {
        let mut r = self.load();
        if let Some(rec) = r.get_mut(&id) {
            rec.peer.last_synced_at = Some(AppleDate::now());
            self.save(&r);
        }
    }

    fn filter(&self, id: Uid) -> SyncFilter {
        self.load().get(&id).map(|r| r.filter.clone()).unwrap_or_default()
    }
}

fn key_account(id: Uid) -> String {
    format!("sync.peer:{id}")
}

// MARK: - State for the UI

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Phase {
    Idle,
    Preparing { fraction: f64 },
    #[serde(rename_all = "camelCase")]
    AwaitingApproval { plan: SyncPlan, plan_hash: String, total_bytes: i64, overwrite_count: usize, pick_list: PickList },
    Transferring { progress: Progress },
    Finished { summary: Summary },
    Failed { message: String },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerRow {
    #[serde(rename = "deviceID")]
    pub device_id: Uid,
    /// The name recorded at pairing (authenticated) over the advertised one.
    pub display_name: String,
    pub kind: DeviceKind,
    pub is_paired: bool,
    pub is_compatible: bool,
    pub is_pairing_open: bool,
    pub last_synced_at: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncState {
    pub display_name: String,
    pub is_advertising: bool,
    pub is_browsing: bool,
    pub listener_error: Option<String>,
    pub pairing_code: Option<String>,
    pub lockout_seconds: u64,
    pub rows: Vec<PeerRow>,
    pub offline_peers: Vec<PeerRow>,
    pub phase: Phase,
    #[serde(rename = "activePeerID")]
    pub active_peer_id: Option<Uid>,
    #[serde(rename = "pairingPeerID")]
    pub pairing_peer_id: Option<Uid>,
    pub error_message: Option<String>,
    pub direction: Direction,
    pub has_library: bool,
}

// MARK: - Model

struct Net {
    advertiser: Option<Advertiser>,
    browser: Option<Browser>,
    listener: Option<tauri::async_runtime::JoinHandle<()>>,
    port: Option<u16>,
}

struct Inner {
    active_users: u32,
    found: Vec<Found>,
    paired: Vec<PairedPeer>,
    listener_error: Option<String>,
    phase: Phase,
    active_peer: Option<Uid>,
    pairing_peer: Option<Uid>,
    error_message: Option<String>,
    direction: Direction,
    last_activity: Instant,
}

pub struct SyncModel {
    app: AppHandle,
    store: PeerStore,
    keys: SharedListenerKeys,
    keys_loaded: AtomicBool,
    gatekeeper: Mutex<PairingGatekeeper>,
    inner: Mutex<Inner>,
    net: Mutex<Net>,
    approval: Mutex<Option<oneshot::Sender<Option<SyncSelection>>>>,
    run_cancel: Mutex<Arc<AtomicBool>>,
    /// Developer switch: `FLACTASTIC_SYNC_OFFLINE=1` renders the UI without
    /// opening any sockets (and so without the firewall prompt).
    offline: bool,
}

impl SyncModel {
    pub fn new(app: AppHandle, data_dir: &Path) -> Arc<Self> {
        let store = PeerStore { path: data_dir.join("sync-peers.json") };
        let paired = store.peers();
        Arc::new(SyncModel {
            app,
            store,
            keys: Arc::new(RwLock::new(ListenerKeys::default())),
            keys_loaded: AtomicBool::new(false),
            gatekeeper: Mutex::new(PairingGatekeeper::default()),
            inner: Mutex::new(Inner {
                active_users: 0,
                found: vec![],
                paired,
                listener_error: None,
                phase: Phase::Idle,
                active_peer: None,
                pairing_peer: None,
                error_message: None,
                direction: Direction::Push,
                last_activity: Instant::now(),
            }),
            net: Mutex::new(Net { advertiser: None, browser: None, listener: None, port: None }),
            approval: Mutex::new(None),
            run_cancel: Mutex::new(Arc::new(AtomicBool::new(false))),
            offline: std::env::var("FLACTASTIC_SYNC_OFFLINE").is_ok_and(|v| v == "1"),
        })
    }

    fn st(&self) -> Arc<AppState> {
        self.app.state::<Arc<AppState>>().inner().clone()
    }

    fn identity(&self) -> PairingIdentity {
        let st = self.st();
        let mut s = st.settings.lock();
        let fresh = s.sync_device_id.is_none();
        let device_id = s.device_id();
        let name = s
            .sync_device_name
            .clone()
            .filter(|n| !fl_core::text::trim_ws(n).is_empty())
            .unwrap_or_else(default_device_name);
        drop(s);
        if fresh {
            st.save_settings();
        }
        PairingIdentity { device_id, display_name: name, kind: DeviceKind::LOCAL }
    }

    pub fn state(&self) -> SyncState {
        let identity = self.identity();
        let i = self.inner.lock();
        let paired: HashMap<Uid, &PairedPeer> = i.paired.iter().map(|p| (p.device_id, p)).collect();
        let mut rows: Vec<PeerRow> = i
            .found
            .iter()
            .map(|f| {
                let p = paired.get(&f.peer.device_id);
                PeerRow {
                    device_id: f.peer.device_id,
                    display_name: p.map_or_else(|| f.peer.display_name.clone(), |p| p.display_name.clone()),
                    kind: f.peer.kind,
                    is_paired: p.is_some(),
                    is_compatible: f.is_compatible(),
                    is_pairing_open: f.peer.is_pairing_open,
                    last_synced_at: p.and_then(|p| p.last_synced_at).map(|d| d.unix_seconds()),
                }
            })
            .collect();
        // Paired devices first.
        rows.sort_by_key(|r| !r.is_paired);
        let visible: std::collections::HashSet<Uid> = i.found.iter().map(|f| f.peer.device_id).collect();
        let offline_peers = i
            .paired
            .iter()
            .filter(|p| !visible.contains(&p.device_id))
            .map(|p| PeerRow {
                device_id: p.device_id,
                display_name: p.display_name.clone(),
                kind: p.kind,
                is_paired: true,
                is_compatible: true,
                is_pairing_open: false,
                last_synced_at: p.last_synced_at.map(|d| d.unix_seconds()),
            })
            .collect();
        let mut g = self.gatekeeper.lock();
        g.refresh_lockout();
        let net = self.net.lock();
        SyncState {
            display_name: identity.display_name,
            is_advertising: net.advertiser.is_some(),
            is_browsing: net.browser.is_some(),
            listener_error: i.listener_error.clone(),
            pairing_code: g.is_pairing_open().then(|| g.current_code()).flatten(),
            lockout_seconds: if g.is_locked_out() { g.lockout_seconds_remaining() } else { 0 },
            rows,
            offline_peers,
            phase: i.phase.clone(),
            active_peer_id: i.active_peer,
            pairing_peer_id: i.pairing_peer,
            error_message: i.error_message.clone(),
            direction: i.direction,
            has_library: self.st().root_path().is_some(),
        }
    }

    fn emit(&self) {
        let _ = self.app.emit("sync://changed", self.state());
    }

    fn set_phase(&self, p: Phase) {
        self.inner.lock().phase = p;
        self.emit();
    }

    fn set_error(&self, m: impl Into<String>) {
        self.inner.lock().error_message = Some(m.into());
        self.emit();
    }

    /// Paired keys, read from the keyring once per launch.
    fn load_keys(&self) {
        if self.keys_loaded.swap(true, Ordering::SeqCst) {
            return;
        }
        let paired = self.inner.lock().paired.clone();
        let mut k = self.keys.write().unwrap();
        for p in paired {
            if let Ok(Some(b)) = fl_platform::secrets::get(&key_account(p.device_id)) {
                if let Some(key) = Key::from_slice(&b) {
                    k.paired.insert(p.device_id, key);
                }
            }
        }
    }

    fn key_for(&self, id: Uid) -> Option<Key> {
        self.keys.read().unwrap().paired.get(&id).cloned()
    }

    // MARK: Lifecycle

    /// The Sync screen or Devices tab appeared.
    pub fn begin(self: &Arc<Self>) {
        let first = {
            let mut i = self.inner.lock();
            i.active_users += 1;
            i.last_activity = Instant::now();
            i.active_users == 1
        };
        if !first {
            return;
        }
        self.load_keys();
        if self.offline {
            self.inner.lock().listener_error = Some("Sync networking is off (FLACTASTIC_SYNC_OFFLINE).".into());
            self.emit();
            return;
        }
        self.start_browser();
        self.start_listener();
        self.spawn_ticker();
    }

    /// The last user left: everything stops.
    pub fn end(&self) {
        let last = {
            let mut i = self.inner.lock();
            i.active_users = i.active_users.saturating_sub(1);
            i.active_users == 0
        };
        if !last {
            return;
        }
        self.cancel_run();
        self.stop_listener();
        self.gatekeeper.lock().close_pairing();
        self.keys.write().unwrap().allow_pairing = false;
        if let Some(b) = self.net.lock().browser.take() {
            b.stop();
        }
        {
            let mut i = self.inner.lock();
            i.found.clear();
            i.phase = Phase::Idle;
            i.active_peer = None;
        }
        self.emit();
    }

    fn start_browser(self: &Arc<Self>) {
        let own = self.identity().device_id;
        let me = Arc::downgrade(self);
        match Browser::start(own, move |found| {
            if let Some(me) = me.upgrade() {
                me.inner.lock().found = found;
                me.emit();
            }
        }) {
            Ok(b) => self.net.lock().browser = Some(b),
            Err(e) => self.inner.lock().listener_error = Some(e),
        }
    }

    fn start_listener(self: &Arc<Self>) {
        if self.net.lock().listener.is_some() {
            return;
        }
        let ctx = match tls::server_context(self.keys.clone()) {
            Ok(c) => c,
            Err(e) => {
                self.set_error(format!("Couldn't start the sync listener: {e}"));
                return;
            }
        };
        let std_listener = match std::net::TcpListener::bind(("0.0.0.0", 0)) {
            Ok(l) => l,
            Err(e) => {
                self.inner.lock().listener_error = Some(format!("Couldn't start the sync listener: {e}"));
                self.emit();
                return;
            }
        };
        let _ = std_listener.set_nonblocking(true);
        let port = std_listener.local_addr().map(|a| a.port()).unwrap_or(0);
        let me = Arc::downgrade(self);
        let handle = tauri::async_runtime::spawn(async move {
            let Ok(l) = tokio::net::TcpListener::from_std(std_listener) else { return };
            loop {
                let Ok((tcp, _)) = l.accept().await else { continue };
                let Some(m) = me.upgrade() else { return };
                let ctx = ctx.clone();
                tauri::async_runtime::spawn(async move { m.handle_incoming(tcp, ctx).await });
            }
        });
        let identity = self.identity();
        let pairing_open = self.gatekeeper.lock().is_pairing_open();
        let adv = Advertiser::start(identity.device_id, &identity.display_name, port, pairing_open);
        let mut net = self.net.lock();
        net.listener = Some(handle);
        net.port = Some(port);
        match adv {
            Ok(a) => net.advertiser = Some(a),
            Err(e) => self.inner.lock().listener_error = Some(e),
        }
        drop(net);
        self.emit();
    }

    fn stop_listener(&self) {
        let mut net = self.net.lock();
        if let Some(h) = net.listener.take() {
            h.abort();
        }
        if let Some(a) = net.advertiser.take() {
            a.stop();
        }
        net.port = None;
    }

    /// Code expiry, lockout countdown and the advertiser's idle timeout.
    fn spawn_ticker(self: &Arc<Self>) {
        let me = Arc::downgrade(self);
        tauri::async_runtime::spawn(async move {
            let mut was_open = false;
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                let Some(m) = me.upgrade() else { return };
                if m.inner.lock().active_users == 0 {
                    return;
                }
                let open = m.gatekeeper.lock().is_pairing_open();
                if was_open && !open {
                    m.pairing_closed();
                }
                let idle = m.inner.lock().last_activity.elapsed() > ADVERTISER_IDLE;
                let running = matches!(m.inner.lock().phase, Phase::Preparing { .. } | Phase::Transferring { .. } | Phase::AwaitingApproval { .. });
                if idle && !running && m.net.lock().listener.is_some() {
                    m.stop_listener();
                }
                if open || was_open || m.gatekeeper.lock().is_locked_out() {
                    m.emit();
                }
                was_open = open;
            }
        });
    }

    /// Restarts (or keeps) the listener on user activity.
    fn keep_listening(self: &Arc<Self>) {
        let active = {
            let mut i = self.inner.lock();
            i.last_activity = Instant::now();
            i.active_users > 0
        };
        if active && !self.offline && self.net.lock().listener.is_none() {
            self.start_listener();
        }
    }

    fn pairing_closed(&self) {
        self.keys.write().unwrap().allow_pairing = false;
        if let Some(a) = self.net.lock().advertiser.as_mut() {
            let _ = a.set_pairing_open(false);
        }
        self.emit();
    }

    // MARK: Pairing

    pub fn open_pairing_code(self: &Arc<Self>) {
        self.keep_listening();
        let code = self.gatekeeper.lock().open_pairing();
        if code.is_none() {
            let secs = self.gatekeeper.lock().lockout_seconds_remaining();
            self.set_error(format!("Too many failed attempts. Try again in {secs} seconds."));
            return;
        }
        self.keys.write().unwrap().allow_pairing = true;
        if let Some(a) = self.net.lock().advertiser.as_mut() {
            let _ = a.set_pairing_open(true);
        }
        self.emit();
    }

    pub fn close_pairing_code(&self) {
        self.gatekeeper.lock().close_pairing();
        self.pairing_closed();
    }

    /// Types a code shown on another device.
    pub fn pair(self: &Arc<Self>, device_id: Uid, typed: &str) {
        self.keep_listening();
        let Some(found) = self.net.lock().browser.as_ref().and_then(|b| b.find(device_id)) else {
            self.set_error("That device is no longer on the network.");
            return;
        };
        let code = normalize_typed_code(typed);
        let session = match GuestPairingSession::new(&code, self.identity()) {
            Ok(s) => s,
            Err(e) => {
                self.set_error(e.user_facing_message());
                return;
            }
        };
        self.inner.lock().pairing_peer = Some(device_id);
        self.emit();
        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            let result = async {
                let ctx = tls::pairing_client().map_err(|e| e.to_string())?;
                let conn = connect_any(&found.addrs, &ctx).await.map_err(|_| {
                    "Couldn't reach that device. Is its pairing code still showing?".to_owned()
                })?;
                let identity = me.identity();
                let m1 = me.clone();
                let m2 = me.clone();
                let r = exchange::run_guest(
                    &conn,
                    session,
                    &identity,
                    |peer, key| async move { m1.save_pairing(peer, key) },
                    |id| async move { m2.forget(id) },
                )
                .await;
                conn.close().await;
                r.map(|_| ()).map_err(|e| match e {
                    exchange::ExchangeError::Pairing(p) => p.user_facing_message().to_owned(),
                    other => format!("Pairing failed. {other}"),
                })
            }
            .await;
            me.inner.lock().pairing_peer = None;
            match result {
                Ok(()) => me.emit(),
                Err(e) => me.set_error(e),
            }
        });
    }

    /// Keyring first; never a plaintext fallback.
    fn save_pairing(&self, peer: PairedPeer, key: Key) -> Result<(), String> {
        fl_platform::secrets::set(&key_account(peer.device_id), key.as_bytes()).map_err(|e| {
            log::warn!("[SyncModel] Couldn't save pairing key: {e}");
            PairingError::STORAGE_FAILED_REASON.to_owned()
        })?;
        self.keys.write().unwrap().paired.insert(peer.device_id, key);
        self.store.upsert(peer);
        self.inner.lock().paired = self.store.peers();
        self.emit();
        Ok(())
    }

    /// Revokes a device: its key leaves the listener's PSK table at once.
    pub fn forget(&self, id: Uid) {
        let _ = fl_platform::secrets::delete(&key_account(id));
        self.keys.write().unwrap().paired.remove(&id);
        self.store.remove(id);
        self.inner.lock().paired = self.store.peers();
        self.emit();
    }

    // MARK: Runs

    pub fn set_direction(&self, d: Direction) {
        self.inner.lock().direction = d;
        self.emit();
    }

    pub fn sync_with(self: &Arc<Self>, device_id: Uid) {
        self.keep_listening();
        let found = self.net.lock().browser.as_ref().and_then(|b| b.find(device_id));
        let name = self.state().rows.iter().find(|r| r.device_id == device_id).map(|r| r.display_name.clone()).unwrap_or_default();
        let Some(found) = found else {
            self.set_error("That device is no longer on the network.");
            return;
        };
        if !found.is_compatible() {
            self.set_error(format!("{name} is running a different version of FLACtastic's sync. Update both devices."));
            return;
        }
        let Some(key) = self.key_for(device_id) else {
            self.set_error(format!("{name} isn't paired with this computer yet."));
            return;
        };
        let st = self.st();
        let Some(root) = st.root_path() else {
            self.set_error("Open a music folder before syncing.");
            return;
        };
        let direction = self.inner.lock().direction;
        let filter = self.store.filter(device_id);
        let tracks: Vec<fl_core::Track> = st.library.read().tracks().iter().cloned().collect();
        let playlists: Vec<Playlist> = st.playlists.lock().playlists.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        *self.run_cancel.lock() = cancel.clone();
        {
            let mut i = self.inner.lock();
            i.active_peer = Some(device_id);
            i.phase = Phase::Preparing { fraction: 0.0 };
        }
        self.emit();

        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            let identity = me.identity();
            let result: Result<Summary, SessionError> = async {
                let (m2, c2, root2, filter2) = (me.clone(), cancel.clone(), root.clone(), filter.clone());
                let pl2 = playlists.clone();
                let manifest = tauri::async_runtime::spawn_blocking(move || {
                    let mut last = Instant::now();
                    builder::build(
                        &tracks,
                        &pl2,
                        &root2,
                        identity_id(&m2),
                        &filter2,
                        |f| {
                            if last.elapsed() > Duration::from_millis(100) {
                                last = Instant::now();
                                m2.set_phase(Phase::Preparing { fraction: f });
                            }
                        },
                        &|| c2.load(Ordering::Relaxed),
                    )
                })
                .await
                .map_err(|e| SessionError::Prepare(e.to_string()))?
                .map_err(|e| match e {
                    builder::BuildError::Cancelled => SessionError::Cancelled,
                    other => SessionError::Prepare(other.to_string()),
                })?;
                let ctx = tls::paired_client(key.clone(), identity.device_id).map_err(|e| SessionError::Prepare(e.to_string()))?;
                let conn = match connect_any(&found.addrs, &ctx).await {
                    Ok(c) => c,
                    Err(fl_sync::ConnectionError::HandshakeFailed(_)) => return Err(SessionError::NotAuthenticated),
                    Err(_) => return Err(SessionError::Unreachable(name.clone())),
                };
                let local = LocalContext { root: root.clone(), manifest, filter, playlists };
                let m3 = me.clone();
                let progress = move |p: &Progress| m3.set_phase(Phase::Transferring { progress: p.clone() });
                let r = session::run_initiator(
                    &conn,
                    direction,
                    &identity,
                    &key,
                    local,
                    |plan| me.request_approval(plan),
                    &progress,
                    &cancel,
                )
                .await;
                conn.close().await;
                let (summary, landed) = r?;
                if direction == Direction::Pull {
                    me.absorb(landed, &root);
                }
                Ok(summary)
            }
            .await;
            match result {
                Ok(summary) => {
                    me.store.record_sync(device_id);
                    me.inner.lock().paired = me.store.peers();
                    me.set_phase(Phase::Finished { summary });
                }
                Err(SessionError::Declined | SessionError::Cancelled) => me.set_phase(Phase::Idle),
                Err(e) => me.set_phase(Phase::Failed { message: e.to_string() }),
            }
            me.inner.lock().active_peer = None;
            me.emit();
        });
    }

    /// Suspends the run until the user answers the confirmation sheet. An
    /// empty plan needs no sheet.
    async fn request_approval(self: &Arc<Self>, plan: SyncPlan) -> Option<SyncSelection> {
        if plan.is_empty() {
            return Some(SyncSelection::everything());
        }
        let (tx, rx) = oneshot::channel();
        *self.approval.lock() = Some(tx);
        self.set_phase(Phase::AwaitingApproval {
            plan_hash: plan.plan_hash(),
            total_bytes: plan.total_transfer_bytes(),
            overwrite_count: plan.overwrite_count(),
            pick_list: picklist::build(&plan),
            plan,
        });
        rx.await.ok().flatten()
    }

    pub fn approve(&self, selection: SyncSelection) {
        // Close the sheet on the click, not at the first progress report.
        self.set_phase(Phase::Transferring { progress: Progress::default() });
        if let Some(tx) = self.approval.lock().take() {
            let _ = tx.send(Some(selection));
        }
    }

    pub fn decline(&self) {
        if let Some(tx) = self.approval.lock().take() {
            let _ = tx.send(None);
        }
    }

    pub fn cancel_run(&self) {
        self.run_cancel.lock().store(true, Ordering::SeqCst);
        self.decline();
    }

    /// Registers what a receiving run landed: adopt the sender's IDs (so the
    /// next run matches on identity), upsert playlists, rescan.
    fn absorb(&self, landed: session::Landed, root: &Path) {
        let st = self.st();
        let mut ids = fl_core::track_ids::TrackIdStore::new();
        ids.load(root);
        let ident = fl_platform::NativeFileIdentity;
        for f in &landed.files {
            if let Some(rel) = relative_path(&f.destination, root) {
                ids.adopt(&ident, f.track_id, &f.destination, &rel);
            }
        }
        ids.save();
        if !landed.playlists.is_empty() {
            let mut store = st.playlists.lock();
            for p in landed.playlists {
                store.upsert_from_sync(p);
            }
            drop(store);
            let _ = self.app.emit("playlists://changed", ());
        }
        st.refresh_library(&self.app);
    }

    // MARK: Incoming

    async fn handle_incoming(self: Arc<Self>, tcp: tokio::net::TcpStream, ctx: openssl::ssl::SslContext) {
        // Snapshotted on arrival: the code on screen when the guest connected.
        let code = self.gatekeeper.lock().current_code();
        let Ok(conn) = SyncConnection::accept(tcp, &ctx).await else { return };
        let result: Result<(), String> = async {
            match IncomingRequest::read(&conn).await.map_err(|e| e.to_string())? {
                IncomingRequest::Pairing(hello) => {
                    let identity = self.identity();
                    let me = self.clone();
                    let r = exchange::run_host(&conn, &hello, code, &identity, |peer, key| async move { me.save_pairing(peer, key) }).await;
                    match r {
                        Ok(_) => {
                            self.gatekeeper.lock().record_success();
                            self.pairing_closed();
                        }
                        Err(exchange::ExchangeError::Pairing(PairingError::NotAcceptingPairing)) => {}
                        Err(e) => {
                            if matches!(e, exchange::ExchangeError::Pairing(PairingError::StorageFailed)) {
                                self.set_error(PairingError::StorageFailed.user_facing_message());
                            }
                            // Every other failure burns the code.
                            self.gatekeeper.lock().record_failure();
                            self.pairing_closed();
                        }
                    }
                    Ok(())
                }
                IncomingRequest::Sync(hello) => {
                    let identity = self.identity();
                    let keys = self.keys.clone();
                    let st = self.st();
                    let root = st.root_path();
                    let tracks: Vec<fl_core::Track> = st.library.read().tracks().iter().cloned().collect();
                    let playlists: Vec<Playlist> = st.playlists.lock().playlists.clone();
                    let cancel = AtomicBool::new(false);
                    let device_id = identity.device_id;
                    let root2 = root.clone();
                    let r = session::run_responder(
                        &conn,
                        &hello,
                        &identity,
                        |id| keys.read().unwrap().paired.get(&id).cloned(),
                        || async move {
                            // Only reached once the peer proved it's paired.
                            let Some(root) = root2 else { return Err("No music folder is open.".to_owned()) };
                            tauri::async_runtime::spawn_blocking(move || {
                                let manifest = builder::build(&tracks, &playlists, &root, device_id, &SyncFilter::default(), |_| {}, &|| false)
                                    .map_err(|e| e.to_string())?;
                                Ok(LocalContext { root, manifest, filter: SyncFilter::default(), playlists })
                            })
                            .await
                            .map_err(|e| e.to_string())?
                        },
                        &|_| {},
                        &cancel,
                    )
                    .await;
                    conn.close().await;
                    let (_, landed) = r.map_err(|e| e.to_string())?;
                    if let Some(root) = root {
                        self.absorb(landed, &root);
                    }
                    Ok(())
                }
            }
        }
        .await;
        if let Err(e) = result {
            log::info!("[SyncModel] Incoming connection ended: {e}");
        }
        self.emit();
    }
}

fn identity_id(m: &SyncModel) -> Uid {
    m.identity().device_id
}

/// Tries each advertised address in turn (IPv4 first).
async fn connect_any(addrs: &[std::net::SocketAddr], ctx: &openssl::ssl::SslContext) -> Result<SyncConnection, fl_sync::ConnectionError> {
    let mut last = fl_sync::ConnectionError::TimedOut;
    for a in addrs {
        match SyncConnection::connect(*a, ctx).await {
            Ok(c) => return Ok(c),
            Err(e @ fl_sync::ConnectionError::HandshakeFailed(_)) => return Err(e),
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// The computer's name, as other devices should see it.
fn default_device_name() -> String {
    let raw = std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_default();
    let name = raw.trim();
    if name.is_empty() {
        return if cfg!(windows) { "Windows PC".into() } else { "Linux PC".into() };
    }
    // COMPUTERNAME is upper-case; "DESKTOP-ABC" reads better as typed.
    name.to_owned()
}

// MARK: - Commands

type M<'a> = State<'a, Arc<SyncModel>>;

fn uid(s: &str) -> Result<Uid, String> {
    Uid::parse(s).ok_or_else(|| format!("bad id {s}"))
}

#[tauri::command]
pub fn sync_state(m: M) -> SyncState {
    m.state()
}

#[tauri::command]
pub fn sync_begin(m: M) {
    m.begin();
}

#[tauri::command]
pub fn sync_end(m: M) {
    m.end();
}

#[tauri::command]
pub fn sync_open_pairing_code(m: M) {
    m.open_pairing_code();
}

#[tauri::command]
pub fn sync_close_pairing_code(m: M) {
    m.close_pairing_code();
}

#[tauri::command]
pub fn sync_pair(m: M, device_id: String, code: String) -> Result<(), String> {
    if !is_well_formed_code(&normalize_typed_code(&code)) {
        return Err("Enter the eight digits shown on the other device.".into());
    }
    m.pair(uid(&device_id)?, &code);
    Ok(())
}

#[tauri::command]
pub fn sync_forget(m: M, device_id: String) -> Result<(), String> {
    m.forget(uid(&device_id)?);
    Ok(())
}

#[tauri::command]
pub fn sync_set_direction(m: M, direction: Direction) {
    m.set_direction(direction);
}

#[tauri::command]
pub fn sync_start(m: M, device_id: String) -> Result<(), String> {
    m.sync_with(uid(&device_id)?);
    Ok(())
}

#[tauri::command]
pub fn sync_approve(m: M, selection: SyncSelection) {
    m.approve(selection);
}

#[tauri::command]
pub fn sync_decline(m: M) {
    m.decline();
}

#[tauri::command]
pub fn sync_cancel(m: M) {
    m.cancel_run();
}

#[tauri::command]
pub fn sync_dismiss_error(m: M) {
    m.inner.lock().error_message = None;
    m.emit();
}

/// First run: briefly open the kinds of sockets sync uses (a TCP listener on
/// all interfaces and the mDNS responder) so Windows asks for firewall access
/// now rather than in the middle of pairing. Runs once per installation.
#[tauri::command]
pub fn sync_prime_network(st: State<Arc<AppState>>) {
    let marker = st.dirs.config.join("network-primed");
    if marker.exists() || std::env::var("FLACTASTIC_SYNC_OFFLINE").is_ok_and(|v| v == "1") {
        return;
    }
    std::thread::spawn(move || {
        let listener = std::net::TcpListener::bind(("0.0.0.0", 0));
        let mdns = mdns_sd::ServiceDaemon::new();
        std::thread::sleep(Duration::from_secs(5));
        if let Ok(d) = mdns {
            let _ = d.shutdown();
        }
        drop(listener);
        let _ = std::fs::write(&marker, b"1");
    });
}
