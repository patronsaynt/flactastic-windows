//! `PeerAdvertiser` + `PeerBrowser` over mDNS/DNS-SD (`_flactastic._tcp`).
//!
//! Everything in a TXT record is unauthenticated: names are shown, never
//! trusted. A device ID seen twice on the network is ignored (both copies).

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};

use fl_core::Uid;
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};

use crate::protocol::{DeviceKind, BONJOUR_SERVICE_TYPE, VERSION};
use crate::txt::{self, DiscoveredPeer};

/// `_flactastic._tcp.local.`
pub fn service_type() -> String {
    format!("{BONJOUR_SERVICE_TYPE}.local.")
}

fn host_name() -> String {
    let raw = std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_else(|_| "flactastic".into());
    let clean: String = raw.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' }).collect();
    format!("{}.local.", clean.to_lowercase())
}

/// Advertises this device while the Sync screen is open.
pub struct Advertiser {
    daemon: ServiceDaemon,
    fullname: String,
    device_id: Uid,
    display_name: String,
    port: u16,
}

impl Advertiser {
    pub fn start(device_id: Uid, display_name: &str, port: u16, pairing_open: bool) -> Result<Advertiser, String> {
        let daemon = ServiceDaemon::new().map_err(|e| format!("Couldn't advertise on the network: {e}"))?;
        let mut a = Advertiser { daemon, fullname: String::new(), device_id, display_name: display_name.to_owned(), port };
        a.register(pairing_open)?;
        Ok(a)
    }

    fn register(&mut self, pairing_open: bool) -> Result<(), String> {
        let props: HashMap<String, String> =
            txt::encode(self.device_id, &self.display_name, DeviceKind::LOCAL, pairing_open).into_iter().collect();
        let ips: &[IpAddr] = &[];
        let info = ServiceInfo::new(&service_type(), &self.display_name, &host_name(), ips, self.port, props)
            .map_err(|e| format!("Couldn't advertise on the network: {e}"))?
            .enable_addr_auto();
        self.fullname = info.get_fullname().to_owned();
        self.daemon.register(info).map_err(|e| format!("Couldn't advertise on the network: {e}"))
    }

    /// Re-publishes the TXT record with the new `p` flag.
    pub fn set_pairing_open(&mut self, open: bool) -> Result<(), String> {
        let _ = self.daemon.unregister(&self.fullname);
        self.register(open)
    }

    pub fn stop(self) {
        if let Ok(rx) = self.daemon.unregister(&self.fullname) {
            // Let the goodbye packet go out.
            let _ = rx.recv_timeout(std::time::Duration::from_millis(500));
        }
        let _ = self.daemon.shutdown();
    }
}

#[derive(Debug, Clone)]
pub struct Found {
    pub peer: DiscoveredPeer,
    /// IPv4 first.
    pub addrs: Vec<SocketAddr>,
}

impl Found {
    pub fn is_compatible(&self) -> bool {
        VERSION.is_compatible(&self.peer.protocol_version)
    }
}

/// Browses for peers; `on_change` runs (on the browse thread) whenever the
/// visible set changes.
pub struct Browser {
    daemon: ServiceDaemon,
    peers: Arc<Mutex<Vec<Found>>>,
}

impl Browser {
    pub fn start(own_id: Uid, on_change: impl Fn(Vec<Found>) + Send + 'static) -> Result<Browser, String> {
        let daemon = ServiceDaemon::new().map_err(|e| format!("Couldn't browse the network: {e}"))?;
        let rx = daemon.browse(&service_type()).map_err(|e| format!("Couldn't browse the network: {e}"))?;
        let peers: Arc<Mutex<Vec<Found>>> = Arc::default();
        let shared = peers.clone();
        std::thread::Builder::new()
            .name("sync-browse".into())
            .spawn(move || {
                // fullname → what it resolved to.
                let mut by_name: HashMap<String, Found> = HashMap::new();
                while let Ok(ev) = rx.recv() {
                    let changed = match ev {
                        ServiceEvent::ServiceResolved(info) => {
                            let props: HashMap<String, String> =
                                info.get_properties().iter().map(|p| (p.key().to_owned(), p.val_str().to_owned())).collect();
                            match txt::decode(&props) {
                                Some(peer) if peer.device_id != own_id => {
                                    let mut addrs: Vec<SocketAddr> =
                                        info.get_addresses().iter().map(|ip| SocketAddr::new(*ip, info.get_port())).collect();
                                    addrs.sort_by_key(|a| (!a.is_ipv4(), a.to_string()));
                                    by_name.insert(info.get_fullname().to_owned(), Found { peer, addrs });
                                    true
                                }
                                _ => false,
                            }
                        }
                        ServiceEvent::ServiceRemoved(_, fullname) => by_name.remove(&fullname).is_some(),
                        ServiceEvent::SearchStopped(_) => break,
                        _ => false,
                    };
                    if changed {
                        let list = visible(&by_name);
                        *shared.lock().unwrap() = list.clone();
                        on_change(list);
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Browser { daemon, peers })
    }

    pub fn peers(&self) -> Vec<Found> {
        self.peers.lock().unwrap().clone()
    }

    pub fn find(&self, id: Uid) -> Option<Found> {
        self.peers().into_iter().find(|f| f.peer.device_id == id)
    }

    pub fn stop(self) {
        let _ = self.daemon.stop_browse(&service_type());
        let _ = self.daemon.shutdown();
    }
}

/// Drops device IDs advertised by more than one service; compatible peers
/// first, then by name.
fn visible(by_name: &HashMap<String, Found>) -> Vec<Found> {
    let mut count: HashMap<Uid, usize> = HashMap::new();
    for f in by_name.values() {
        *count.entry(f.peer.device_id).or_default() += 1;
    }
    let mut out: Vec<Found> = by_name.values().filter(|f| count[&f.peer.device_id] == 1).cloned().collect();
    out.sort_by(|a, b| {
        b.is_compatible()
            .cmp(&a.is_compatible())
            .then_with(|| fl_core::text::standard_compare(&a.peer.display_name, &b.peer.display_name))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(id: Uid, name: &str) -> Found {
        Found {
            peer: DiscoveredPeer {
                device_id: id,
                display_name: name.into(),
                kind: DeviceKind::Mac,
                protocol_version: VERSION,
                is_pairing_open: false,
            },
            addrs: vec![],
        }
    }

    #[test]
    fn duplicates_are_dropped_and_names_sorted() {
        let (a, b, c) = (Uid::new_v4(), Uid::new_v4(), Uid::new_v4());
        let mut m = HashMap::new();
        m.insert("x".into(), found(a, "Zed"));
        m.insert("y".into(), found(b, "alpha"));
        m.insert("z1".into(), found(c, "Dup"));
        m.insert("z2".into(), found(c, "Dup again"));
        let v = visible(&m);
        assert_eq!(v.iter().map(|f| f.peer.display_name.as_str()).collect::<Vec<_>>(), vec!["alpha", "Zed"]);
        assert_eq!(service_type(), "_flactastic._tcp.local.");
    }
}
