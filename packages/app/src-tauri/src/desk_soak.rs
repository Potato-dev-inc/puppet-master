// Behaviour from desktoplab (github.com/v7ren/desktoplab), commit 6e5360e, MIT (examples/soak.rs, PLAN.md N0.2).
//! The loopback soak of My computers, run by hand: `cargo run --example desk_soak -- --secs 60`.
//!
//! Two computers in one process, each a full `DeskLink` on its own real network endpoint bound to 127.0.0.1 only,
//! paired with each other in memory (no pairing sheet, no key store, no file). A pretend PC (`SimHost`) stands in for
//! the operating system: nothing here moves the real pointer, presses a key, installs a hook or touches the real
//! clipboard. What is real is the link: iroh sockets on loopback, both computers dialing at once, 1 kHz motion, a copy
//! of a few MB every 5 seconds, and the heartbeat.
//!
//! It prints how many times a computer was not online, the longest gap between heartbeats, how many copies arrived,
//! and exits with 1 when the link was ever offline after it came up or a heartbeat gap reached 150 ms.
//! The two-computer script (`docs/features/my-computers-tests.md`) is the real test; this one proves the link is sound
//! on one machine first, and gives a number to compare with.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::json;

use crate::desk::host::DisplayInfo;
use crate::desk::link::remote::{Capture, CaptureSink, ClipItem, Inject, RemoteHost};
use crate::desk::link::{DeskLink, MemLinkStore};
use crate::p2p::capability::{self, Registry};
use crate::p2p::endpoint::NodeId;
use crate::p2p::identity::DeviceIdentity;
use crate::p2p::iroh_net::IrohFactory;
use crate::p2p::trust::{KeyStoreSigner, MemoryTrustStorage, TrustList};
use crate::p2p::P2p;
use crate::secrets::{MemoryBackend, SecretBackend};
use crate::team::keystore::KeyStore;

/// A PC that is only numbers.
pub struct SimHost {
    cursor: Mutex<(i32, i32)>,
    capture: Mutex<Option<CaptureSink>>,
    clipboard: Mutex<Vec<ClipItem>>,
    pub moves: AtomicU64,
    pub copies: AtomicU64,
    held: Mutex<BTreeSet<u16>>,
}

impl SimHost {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            cursor: Mutex::new((960, 540)),
            capture: Mutex::new(None),
            clipboard: Mutex::new(Vec::new()),
            moves: AtomicU64::new(0),
            copies: AtomicU64::new(0),
            held: Mutex::new(BTreeSet::new()),
        })
    }

    fn person(&self, c: Capture) -> bool {
        let sink = self.capture.lock().unwrap_or_else(|p| p.into_inner()).clone();
        sink.map(|s| s(c)).is_some()
    }
}

impl RemoteHost for SimHost {
    fn displays(&self) -> Result<Vec<DisplayInfo>, String> {
        Ok(vec![DisplayInfo { id: "sim".into(), name: "sim".into(), x: 0, y: 0, width: 1920, height: 1080, scale: 1.0, primary: true }])
    }
    fn cursor_pos(&self) -> Result<(i32, i32), String> {
        Ok(*self.cursor.lock().unwrap_or_else(|p| p.into_inner()))
    }
    fn move_cursor(&self, x: i32, y: i32) -> Result<(), String> {
        *self.cursor.lock().unwrap_or_else(|p| p.into_inner()) = (x, y);
        Ok(())
    }
    fn inject(&self, event: Inject) -> Result<(), String> {
        match event {
            Inject::Move { dx, dy } => {
                let mut c = self.cursor.lock().unwrap_or_else(|p| p.into_inner());
                *c = ((c.0 + dx).clamp(0, 1919), (c.1 + dy).clamp(0, 1079));
                self.moves.fetch_add(1, Ordering::Relaxed);
            }
            Inject::Key { hid, down } => {
                let mut h = self.held.lock().unwrap_or_else(|p| p.into_inner());
                if down {
                    h.insert(hid);
                } else {
                    h.remove(&hid);
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn release_injected(&self) {
        self.held.lock().unwrap_or_else(|p| p.into_inner()).clear();
    }
    fn start_capture(&self, sink: CaptureSink, _exit: (i32, i32)) -> Result<(), String> {
        *self.capture.lock().unwrap_or_else(|p| p.into_inner()) = Some(sink);
        Ok(())
    }
    fn stop_capture(&self) {
        *self.capture.lock().unwrap_or_else(|p| p.into_inner()) = None;
    }
    fn clipboard_read(&self) -> Result<Vec<ClipItem>, String> {
        Ok(self.clipboard.lock().unwrap_or_else(|p| p.into_inner()).clone())
    }
    fn clipboard_write(&self, items: &[ClipItem]) -> Result<(), String> {
        *self.clipboard.lock().unwrap_or_else(|p| p.into_inner()) = items.to_vec();
        self.copies.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

struct Pc {
    id: NodeId,
    device_pub: [u8; 32],
    name: String,
    trust: Arc<TrustList>,
    p2p: Arc<P2p>,
    link: Arc<DeskLink>,
    host: Arc<SimHost>,
}

fn pc(name: &str) -> Result<Pc, String> {
    let backend: Arc<dyn SecretBackend> = Arc::new(MemoryBackend::new());
    let keys = KeyStore::with_backend(backend, name);
    let ident = DeviceIdentity::load(&keys).map_err(|e| e.to_string())?;
    let trust = TrustList::open(MemoryTrustStorage::new(), Arc::new(KeyStoreSigner::new(keys.clone())));
    let p2p = Arc::new(P2p::new(keys, Arc::new(IrohFactory), Arc::new(Registry::new()), trust.clone()).loopback_only(true));
    p2p.apply_settings(&json!({ "desk_enabled": true }));
    let host = SimHost::new();
    let link = DeskLink::new(p2p.clone(), trust.clone(), host.clone(), Arc::new(MemLinkStore::default()), name).map_err(|e| e.to_string())?;
    link.apply_settings(true);
    Ok(Pc { id: ident.id(), device_pub: ident.binding().device_pub, name: name.to_string(), trust, p2p, link, host })
}

fn png(n: usize, seed: u8) -> Vec<u8> {
    let mut v = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    v.extend((0..n.saturating_sub(8)).map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed)));
    v
}

#[derive(Debug)]
pub struct Report {
    pub secs: u64,
    pub came_up_in_ms: u128,
    pub offline_samples: u32,
    pub max_heartbeat_gap_ms: u64,
    pub heartbeats: u64,
    pub copies_sent: u32,
    pub copies_arrived: u64,
    pub motion_sent: u64,
    pub motion_arrived: u64,
}

impl Report {
    pub fn ok(&self) -> bool {
        self.offline_samples == 0 && self.max_heartbeat_gap_ms < 150 && self.copies_arrived >= u64::from(self.copies_sent)
    }
}

/// Runs the soak for `secs` seconds with a `copy_mb` MB copy every 5 seconds.
pub async fn run(secs: u64, copy_mb: usize) -> Result<Report, String> {
    let (a, b) = (pc("Soak-A")?, pc("Soak-B")?);
    a.trust.add(b.id, b.device_pub, &b.name, 1).map_err(|e| e.to_string())?;
    b.trust.add(a.id, a.device_pub, &a.name, 1).map_err(|e| e.to_string())?;
    for (x, y) in [(&a, &b), (&b, &a)] {
        for cap in [capability::DESK_INPUT, capability::DESK_CLIPBOARD] {
            x.trust.set_permission(&y.id, cap, true).map_err(|e| e.to_string())?;
        }
    }
    // Both listen first, so each can be told where the other is (this tool has no discovery and no pairing ticket).
    a.p2p.want(capability::DESK_INPUT).await.map_err(|e| e.to_string())?;
    b.p2p.want(capability::DESK_INPUT).await.map_err(|e| e.to_string())?;
    a.p2p.addrs().remember(b.id, &b.p2p.local_addrs());
    b.p2p.addrs().remember(a.id, &a.p2p.local_addrs());

    let started = Instant::now();
    a.link.start();
    b.link.start();
    let online = |p: &Pc, q: &Pc| p.link.view().devices.iter().any(|d| d.node == q.id.to_hex() && d.state == "online");
    while !(online(&a, &b) && online(&b, &a)) {
        if started.elapsed() > Duration::from_secs(30) {
            return Err("the two computers did not link up within 30 s".into());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let came_up_in_ms = started.elapsed().as_millis();

    // Cross from A to B.
    a.host.move_cursor(1919, 540)?;
    let deadline = Instant::now() + Duration::from_secs(10);
    while a.host.capture.lock().unwrap_or_else(|p| p.into_inner()).is_none() {
        if Instant::now() > deadline {
            return Err("the pointer did not cross".into());
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let mut offline_samples = 0u32;
    let mut copies_sent = 0u32;
    let mut motion_sent = 0u64;
    let begin = Instant::now();
    let mut next_copy = begin;
    let mut next_sample = begin;
    while begin.elapsed() < Duration::from_secs(secs) {
        if Instant::now() >= next_copy {
            next_copy += Duration::from_secs(5);
            copies_sent += 1;
            *a.host.clipboard.lock().unwrap_or_else(|p| p.into_inner()) = vec![ClipItem::Png(png(copy_mb * 1024 * 1024, copies_sent as u8))];
            a.link.push_clipboard(&b.id);
        }
        if Instant::now() >= next_sample {
            next_sample += Duration::from_millis(100);
            if !(online(&a, &b) && online(&b, &a)) {
                offline_samples += 1;
            }
        }
        let d = if (motion_sent / 50) % 2 == 0 { 1 } else { -1 };
        if a.host.person(Capture::Motion { dx: d, dy: 0 }) {
            motion_sent += 1;
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    // Let the last copy land.
    tokio::time::sleep(Duration::from_secs(3)).await;

    let (beats_b, gap_b) = b.link.beat_stats(&a.id).unwrap_or((0, 0));
    let (beats_a, gap_a) = a.link.beat_stats(&b.id).unwrap_or((0, 0));
    let report = Report {
        secs,
        came_up_in_ms,
        offline_samples,
        max_heartbeat_gap_ms: gap_a.max(gap_b),
        heartbeats: beats_a + beats_b,
        copies_sent,
        copies_arrived: b.host.copies.load(Ordering::Relaxed),
        motion_sent,
        motion_arrived: b.host.moves.load(Ordering::Relaxed),
    };
    a.link.shutdown();
    b.link.shutdown();
    Ok(report)
}

/// The example's entry point. Returns the process exit code.
pub async fn run_cli(args: Vec<String>) -> i32 {
    let mut secs = 30u64;
    let mut copy_mb = 4usize;
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        match (arg.as_str(), it.next()) {
            ("--secs", Some(v)) => secs = v.parse().unwrap_or(30),
            ("--copy-mb", Some(v)) => copy_mb = v.parse().unwrap_or(4).clamp(1, 30),
            _ => {
                eprintln!("usage: desk_soak [--secs N] [--copy-mb N]");
                return 2;
            }
        }
    }
    println!("desk soak: two computers on 127.0.0.1, {secs} s, a {copy_mb} MB copy every 5 s");
    match run(secs, copy_mb).await {
        Ok(r) => {
            println!("{r:#?}");
            if r.ok() {
                println!("PASS");
                0
            } else {
                println!("FAIL: offline samples {}, longest heartbeat gap {} ms (limit 150), copies {}/{}", r.offline_samples, r.max_heartbeat_gap_ms, r.copies_arrived, r.copies_sent);
                1
            }
        }
        Err(e) => {
            println!("FAIL: {e}");
            1
        }
    }
}
