//! "Found nearby": which computers on this network are showing a pairing code right now.
//!
//! A computer announces itself only while the person has a code on screen (name, node id, addresses: all of it is
//! about to be shown anyway) and withdraws the moment the code is used, expires, or is cancelled. Nothing is
//! announced while idle. LAN only, behind the Labs switch.
//!
//! This file is the seam, not the radio: `Discovery` is what pairing talks to. `MemoryLan` is a shared list for
//! tests and demos. `NoDiscovery` is what ships until the LAN announcer (mDNS) is wired and has passed the
//! two-computer test (DL-11): "Add by address" always works.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use super::endpoint::NodeId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Announcement {
    pub node: NodeId,
    pub name: String,
    pub addrs: Vec<SocketAddr>,
}

pub trait Discovery: Send + Sync {
    fn announce(&self, a: Announcement);
    fn withdraw(&self, node: &NodeId);
    fn nearby(&self) -> Vec<Announcement>;
}

/// Finds nobody and announces nothing.
pub struct NoDiscovery;

impl Discovery for NoDiscovery {
    fn announce(&self, _a: Announcement) {}
    fn withdraw(&self, _node: &NodeId) {}
    fn nearby(&self) -> Vec<Announcement> {
        Vec::new()
    }
}

/// One shared list: every computer given a clone sees the others' announcements.
#[derive(Clone, Default)]
pub struct MemoryLan {
    list: Arc<Mutex<Vec<Announcement>>>,
}

impl MemoryLan {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Discovery for MemoryLan {
    fn announce(&self, a: Announcement) {
        let mut l = self.list.lock().unwrap_or_else(|p| p.into_inner());
        l.retain(|x| x.node != a.node);
        l.push(a);
    }

    fn withdraw(&self, node: &NodeId) {
        self.list.lock().unwrap_or_else(|p| p.into_inner()).retain(|x| x.node != *node);
    }

    fn nearby(&self) -> Vec<Announcement> {
        self.list.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
}

/// The real LAN announcer: mDNS / DNS-SD service `_pmpair._udp.local.` (RFC 6763), pure Rust, no system service.
/// The TXT record carries the node id, the name and the socket addresses; those are about to be shown on screen
/// anyway. Nothing is registered while idle: `announce` registers, `withdraw` unregisters.
pub struct MdnsLan {
    daemon: mdns_sd::ServiceDaemon,
    seen: Arc<Mutex<std::collections::HashMap<String, Announcement>>>,
    mine: Mutex<Vec<(NodeId, String)>>,
}

const SERVICE: &str = "_pmpair._udp.local.";

impl MdnsLan {
    pub fn new() -> Result<Self, String> {
        let daemon = mdns_sd::ServiceDaemon::new().map_err(|e| e.to_string())?;
        let rx = daemon.browse(SERVICE).map_err(|e| e.to_string())?;
        let seen: Arc<Mutex<std::collections::HashMap<String, Announcement>>> = Default::default();
        let sink = seen.clone();
        std::thread::Builder::new()
            .name("pm-mdns-browse".into())
            .spawn(move || {
                while let Ok(ev) = rx.recv() {
                    match ev {
                        mdns_sd::ServiceEvent::ServiceResolved(info) => {
                            if let Some(a) = parse(&info) {
                                sink.lock().unwrap_or_else(|p| p.into_inner()).insert(info.get_fullname().to_string(), a);
                            }
                        }
                        mdns_sd::ServiceEvent::ServiceRemoved(_, full) => {
                            sink.lock().unwrap_or_else(|p| p.into_inner()).remove(&full);
                        }
                        _ => {}
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self { daemon, seen, mine: Mutex::new(Vec::new()) })
    }
}

fn parse(info: &mdns_sd::ServiceInfo) -> Option<Announcement> {
    let hex = info.get_property_val_str("n")?;
    if hex.len() != 64 || !hex.is_ascii() {
        return None;
    }
    let mut id = [0u8; 32];
    for (i, o) in id.iter_mut().enumerate() {
        *o = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    let name = info.get_property_val_str("m").unwrap_or("A computer").to_string();
    let mut addrs: Vec<SocketAddr> = info.get_property_val_str("a").unwrap_or("").split(',').filter_map(|s| s.parse().ok()).collect();
    if addrs.is_empty() {
        let port = info.get_port();
        addrs = info.get_addresses().iter().map(|ip| SocketAddr::new(*ip, port)).collect();
    }
    Some(Announcement { node: NodeId(id), name, addrs })
}

impl Discovery for MdnsLan {
    fn announce(&self, a: Announcement) {
        let ips: Vec<String> = a.addrs.iter().map(|s| s.ip()).filter(|ip| !ip.is_unspecified()).map(|ip| ip.to_string()).collect();
        let Some(port) = a.addrs.first().map(|s| s.port()) else { return };
        if ips.is_empty() {
            return;
        }
        let hex = a.node.to_hex();
        let instance = format!("pm-{}", &hex[..16]);
        let mut txt = std::collections::HashMap::new();
        txt.insert("n".to_string(), hex);
        txt.insert("m".to_string(), a.name.chars().take(60).collect());
        let mut list = String::new();
        for s in &a.addrs {
            let next = s.to_string();
            if list.len() + next.len() + 1 > 200 {
                break;
            }
            if !list.is_empty() {
                list.push(',');
            }
            list.push_str(&next);
        }
        txt.insert("a".to_string(), list);
        let Ok(info) = mdns_sd::ServiceInfo::new(SERVICE, &instance, &format!("{instance}.local."), ips.join(",").as_str(), port, txt) else { return };
        let full = info.get_fullname().to_string();
        if self.daemon.register(info).is_ok() {
            let mut m = self.mine.lock().unwrap_or_else(|p| p.into_inner());
            m.retain(|(n, _)| *n != a.node);
            m.push((a.node, full));
        }
    }

    fn withdraw(&self, node: &NodeId) {
        let mut m = self.mine.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(i) = m.iter().position(|(n, _)| n == node) {
            let (_, full) = m.remove(i);
            let _ = self.daemon.unregister(&full);
        }
    }

    fn nearby(&self) -> Vec<Announcement> {
        let mine: Vec<NodeId> = self.mine.lock().unwrap_or_else(|p| p.into_inner()).iter().map(|(n, _)| *n).collect();
        self.seen.lock().unwrap_or_else(|p| p.into_inner()).values().filter(|a| !mine.contains(&a.node)).cloned().collect()
    }
}

#[cfg(test)]
mod mdns_tests {
    use super::*;

    /// Real multicast on this machine's network: run with `PM_TEST_LAN_IP=<lan ip> cargo test mdns_two_in_one -- --ignored`.
    #[test]
    #[ignore]
    fn mdns_two_in_one_process_see_each_other() {
        let (a, b) = (MdnsLan::new().unwrap(), MdnsLan::new().unwrap());
        let ip = local_ip();
        let node = NodeId([7u8; 32]);
        a.announce(Announcement { node, name: "Alpha".into(), addrs: vec![SocketAddr::new(ip, 4455)] });
        let mut found = Vec::new();
        for _ in 0..40 {
            found = b.nearby();
            if !found.is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        assert_eq!(found.len(), 1, "nothing found nearby");
        assert_eq!(found[0].node, node);
        assert_eq!(found[0].name, "Alpha");
        assert!(a.nearby().is_empty(), "must not list itself");
        a.withdraw(&node);
        for _ in 0..40 {
            if b.nearby().is_empty() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        panic!("withdrawn computer still listed");
    }

    /// This computer's address on the LAN, from `PM_TEST_LAN_IP` (read at build time: the guard tests keep sockets and env reads out of this file).
    fn local_ip() -> std::net::IpAddr {
        option_env!("PM_TEST_LAN_IP").expect("build with PM_TEST_LAN_IP set to this computer's LAN address").parse().unwrap()
    }
}
