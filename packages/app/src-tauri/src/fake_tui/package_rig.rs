//! The Package check end-to-end rig (`tests/fake_tui_package_check.rs`): a fake package registry on
//! `127.0.0.1` (a port the OS picks) and the card's real lookup (`package_check::live::lookup`) running
//! on this world's pane registry.
//!
//! What is real: the pane and its screen, the card (`pane_prompt::detect`, risk and packages), the private
//! server check (reads the project's own `.npmrc`), the settings file, the cache, the thread pool and the
//! deadline, the HTTP client (`UreqFetcher`: allow-list, caps, headers, timeouts), the registry parsers and the
//! sentences. What is made up: the registry (this file) and the environment the private-server check reads
//! (no variables, no home folder, so a developer's own `~/.npmrc` or `PIP_INDEX_URL` cannot change a test).
//!
//! No request leaves the machine: `set_base_for_tests` accepts only `http://127.0.0.1:<port>`. Dev only.

use super::world::World;
use crate::package_check::cache::Cache;
use crate::package_check::live::{self, LiveIo, RegistryIo};
use crate::package_check::model::{set_base_for_tests, Fetcher, Registry, Settings};
use crate::package_check::settings as settings_file;
use crate::pty::PaneRegistry;
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// One request the fake registry saw: its request line and every header, names in lower case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    pub request_line: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
}

impl Seen {
    /// The header names, sorted.
    pub fn header_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.headers.iter().map(|(name, _)| name.clone()).collect();
        names.sort();
        names
    }
}

struct Reply {
    status: u16,
    body: Vec<u8>,
}

struct Shared {
    routes: Mutex<HashMap<String, Reply>>,
    seen: Mutex<Vec<Seen>>,
    /// Milliseconds every answer waits before it is sent.
    delay_ms: AtomicU64,
    /// Every connection is closed as soon as it is accepted, with no answer.
    hang_up: AtomicBool,
    stop: AtomicBool,
}

/// A fake registry plus the card's lookup, for one test.
pub struct PackageRig {
    shared: Arc<Shared>,
    port: u16,
    registry: Arc<Mutex<PaneRegistry>>,
}

impl PackageRig {
    /// Starts the fake registry and points every registry base at it. Lookups are on.
    pub fn start() -> PackageRig {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind a loopback port");
        let port = listener.local_addr().expect("local address").port();
        let shared = Arc::new(Shared {
            routes: Mutex::new(HashMap::new()),
            seen: Mutex::new(Vec::new()),
            delay_ms: AtomicU64::new(0),
            hang_up: AtomicBool::new(false),
            stop: AtomicBool::new(false),
        });
        let accepting = shared.clone();
        std::thread::Builder::new()
            .name("package-rig-registry".into())
            .spawn(move || {
                for stream in listener.incoming() {
                    if accepting.stop.load(Ordering::SeqCst) {
                        break;
                    }
                    let Ok(stream) = stream else { continue };
                    let shared = accepting.clone();
                    // One thread per connection, so a slow answer does not hold up the others.
                    let _ = std::thread::Builder::new().name("package-rig-conn".into()).spawn(move || serve(stream, &shared));
                }
            })
            .expect("start the fake registry");
        let base = format!("http://127.0.0.1:{port}");
        for registry in [Registry::NpmDoc, Registry::NpmDownloads, Registry::Pypi, Registry::Crates, Registry::Gems, Registry::Go] {
            set_base_for_tests(registry, &base);
        }
        let (_, registry) = World::get().handles();
        let rig = PackageRig { shared, port, registry };
        rig.set_lookups(true);
        rig
    }

    /// `http://127.0.0.1:<port>`
    pub fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// Answers `GET <path>` with `status` and `body` (JSON). A path nobody serves is a 404.
    pub fn serve(&self, path: &str, status: u16, body: &str) {
        self.shared.routes.lock().insert(path.to_string(), Reply { status, body: body.as_bytes().to_vec() });
    }

    /// An npm package: its document and last week's downloads. `age_days` is how long ago it was first
    /// published (a little more, so "3 days" is still "3 days" when the lookup runs a minute later).
    pub fn serve_npm(&self, name: &str, age_days: u64, weekly: u64, install_script: bool) {
        let published = chrono::Utc::now() - chrono::Duration::days(age_days as i64) - chrono::Duration::hours(1);
        let at = published.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let scripts = if install_script { json!({ "postinstall": "node setup.js" }) } else { json!({}) };
        let doc = json!({
            "name": name,
            "dist-tags": { "latest": "1.0.0" },
            "time": { "created": at, "modified": at, "1.0.0": at },
            "maintainers": [{ "name": "sample-one" }],
            "repository": { "type": "git", "url": "git+https://example.test/sample-one/repo.git" },
            "versions": { "1.0.0": { "name": name, "version": "1.0.0", "scripts": scripts } },
        });
        self.serve(&format!("/{name}"), 200, &doc.to_string());
        self.serve(&format!("/downloads/point/last-week/{name}"), 200, &json!({ "downloads": weekly, "package": name }).to_string());
    }

    /// Every answer waits this long before it is sent (a slow registry).
    pub fn slow(&self, delay: Duration) {
        self.shared.delay_ms.store(delay.as_millis() as u64, Ordering::SeqCst);
    }

    /// The registry is gone: every connection is closed as soon as it is made, with no answer.
    pub fn close(&self) {
        self.shared.hang_up.store(true, Ordering::SeqCst);
    }

    /// Every request so far, oldest first.
    pub fn requests(&self) -> Vec<Seen> {
        self.shared.seen.lock().clone()
    }

    /// The paths asked for so far, oldest first.
    pub fn paths(&self) -> Vec<String> {
        self.requests().into_iter().map(|seen| seen.path).collect()
    }

    /// Settings > General > Safety > "Check packages before installs".
    pub fn set_lookups(&self, on: bool) {
        settings_file::save(&settings_file::live_root(), &Settings { lookups: on }).expect("save the package check setting");
    }

    /// `package_check_lookup` as the desktop window calls it. The view as JSON, or the error text.
    pub fn lookup(&self, pane_id: &str, prompt_id: &str, again: bool) -> Result<Value, String> {
        let io = RigIo { inner: RegistryIo::new(self.registry.clone()) };
        live::lookup(&io, Cache::global(), pane_id, prompt_id, again)
            .map(|view| serde_json::to_value(view).unwrap_or(Value::Null))
    }
}

impl Drop for PackageRig {
    fn drop(&mut self) {
        // The next test finds the setting on, and the accept loop ends.
        self.set_lookups(true);
        self.shared.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
    }
}

/// The real lookup I/O, except that the environment and the home folder are empty.
struct RigIo {
    inner: RegistryIo,
}

impl LiveIo for RigIo {
    fn prompt(&self, pane_id: &str) -> Option<(Value, String)> {
        self.inner.prompt(pane_id)
    }
    fn fetcher(&self) -> Arc<dyn Fetcher> {
        self.inner.fetcher()
    }
    fn now_ms(&self) -> u64 {
        self.inner.now_ms()
    }
    fn settings(&self) -> Settings {
        self.inner.settings()
    }
    fn read_text(&self, p: &Path) -> Option<String> {
        self.inner.read_text(p)
    }
    fn env(&self, _k: &str) -> Option<String> {
        None
    }
    fn home(&self) -> Option<PathBuf> {
        None
    }
}

/// One connection: read the request head, record it, answer.
fn serve(mut stream: TcpStream, shared: &Shared) {
    if shared.hang_up.load(Ordering::SeqCst) {
        let _ = stream.shutdown(Shutdown::Both);
        return;
    }
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if head.len() > 16 * 1024 {
            return;
        }
        match stream.read(&mut byte) {
            Ok(1) => head.push(byte[0]),
            _ => return,
        }
    }
    let text = String::from_utf8_lossy(&head).into_owned();
    let mut lines = text.split("\r\n");
    let request_line = lines.next().unwrap_or("").to_string();
    let headers: Vec<(String, String)> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_string()))
        .collect();
    let path = request_line.split(' ').nth(1).unwrap_or("").to_string();
    shared.seen.lock().push(Seen { request_line, path: path.clone(), headers });

    let delay = shared.delay_ms.load(Ordering::SeqCst);
    if delay > 0 {
        std::thread::sleep(Duration::from_millis(delay));
    }
    let (status, body) = match shared.routes.lock().get(&path) {
        Some(reply) => (reply.status, reply.body.clone()),
        None => (404, b"{}".to_vec()),
    };
    let reason = match status {
        200 => "OK",
        404 => "Not Found",
        _ => "Other",
    };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&body);
    let _ = stream.flush();
}
