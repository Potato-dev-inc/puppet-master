//! The engine with the REAL launcher, the REAL bundled brain (`resources/potato-brain/brain.mjs`) and the REAL
//! bridge gate: the node process starts, says hello and polls the gate. Every other test uses a fake launcher or
//! the fake brain, and the fake brain is started from a plain path; the real app is handed its resource folder
//! by Tauri, and on Windows that path is the canonical `\\?\C:\...` form. Node cannot run a script named like
//! that (it dies at once with `EISDIR ... lstat 'C:'`), so before the fix the brain of a Windows dev build or
//! installed app crashed five times in 18 seconds and stopped with "Press Start again" and no reason.
//!
//! No model is asked: the test only starts the brain and waits for `hello`, so there is no network and no key.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use super::auth::Brains;
use super::service::*;
use super::service_tests::{FakeHost, KEY};
use super::sidecar::{Launcher, NodeState, RealLauncher};
use super::store::{self, MsgKind};

fn real_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// The generated brain folder of this source tree, built on the spot when it is missing. `None` (with a
/// printed reason) when it cannot be built here, e.g. no `esbuild` installed.
fn bundled_brain() -> Option<PathBuf> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = manifest.join("resources").join("potato-brain");
    if !dir.join("brain.mjs").is_file() {
        let script = manifest.join("..").join("..").join("..").join("scripts").join("bundle-potato-brain.mjs");
        let built = std::process::Command::new("node").arg(&script).status().map(|s| s.success()).unwrap_or(false);
        if !built || !dir.join("brain.mjs").is_file() {
            eprintln!("skipped: resources/potato-brain/brain.mjs is not built (node scripts/bundle-potato-brain.mjs)");
            return None;
        }
    }
    Some(dir)
}

/// A service on a temp folder with a bot called `mochi`, the real launcher, and the real gate's listener.
struct Live {
    svc: Arc<Service>,
    host: Arc<FakeHost>,
    root: PathBuf,
}

fn live(name: &str, resources: PathBuf) -> Option<Live> {
    if !matches!(RealLauncher::new().node(), NodeState::Ready { .. }) {
        eprintln!("skipped: no Node 20+ on the path");
        return None;
    }
    let root = std::env::temp_dir().join(format!("pm-potato-real-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let data = root.join("potato");
    let host = Arc::new(FakeHost {
        now: AtomicI64::new(real_ms()),
        secrets: Mutex::new(HashMap::from([(SECRET_DEEPSEEK.to_string(), KEY.to_string())])),
        changes: Mutex::new(Vec::new()),
        resources: Mutex::new(Some(resources)),
        url: Mutex::new(None),
    });
    let brains = Arc::new(Brains::new());
    brains.set_enabled(true);
    let svc = Service::new(data.clone(), host.clone(), Arc::new(RealLauncher::new()), brains.clone());
    store::add_bot(&data, "mochi", "fish", 1).unwrap();
    let addr = crate::bridge::spawn_potato_listener(brains, svc.clone());
    *host.url.lock() = Some(format!("http://{addr}"));
    Some(Live { svc, host, root })
}

impl Live {
    fn state(&self) -> BrainState {
        self.svc.state().unwrap().bots.iter().find(|b| b.id == "mochi").unwrap().brain_state
    }

    /// Runs the supervisor (the app's own thread does this every half second) until `done` or the time is up.
    fn run_until(&self, limit: Duration, done: impl Fn(&Live) -> bool) -> bool {
        let until = Instant::now() + limit;
        while Instant::now() < until {
            self.host.now.store(real_ms(), Ordering::SeqCst);
            self.svc.tick();
            if done(self) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        false
    }

    fn notes(&self) -> Vec<String> {
        self.svc.messages("mochi", 100).unwrap().into_iter().filter(|m| m.kind == MsgKind::Note).map(|m| m.text).collect()
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        self.svc.stop_all(Duration::ZERO);
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn the_real_brain_starts_from_the_path_tauri_gives_the_app() {
    let Some(plain) = bundled_brain() else { return };
    // What `app.path().resolve("resources/potato-brain", Resource)` returns on Windows: canonical, so `\\?\C:\...`.
    let handed = std::fs::canonicalize(&plain).unwrap();
    let Some(rig) = live("verbatim", handed.clone()) else { return };
    rig.svc.start_bot("mochi").unwrap();
    let ready = rig.run_until(Duration::from_secs(25), |r| r.state() == BrainState::Ready || r.state() == BrainState::Stopped);
    let log = rig.svc.log_lines("mochi");
    assert!(ready && rig.state() == BrainState::Ready, "the brain never said hello from {handed:?}; state {:?}; notes {:?}; log {log:?}", rig.state(), rig.notes());
    assert!(log.iter().any(|l| l.contains("ready")), "{log:?}");
}

#[test]
fn the_real_brain_starts_from_a_plain_path_too() {
    let Some(plain) = bundled_brain() else { return };
    let Some(rig) = live("plain", plain) else { return };
    rig.svc.start_bot("mochi").unwrap();
    assert!(rig.run_until(Duration::from_secs(25), |r| r.state() == BrainState::Ready), "state {:?}; log {:?}", rig.state(), rig.svc.log_lines("mochi"));
}
