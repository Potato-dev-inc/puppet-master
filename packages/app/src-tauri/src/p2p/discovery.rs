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
