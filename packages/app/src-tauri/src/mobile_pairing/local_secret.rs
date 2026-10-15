//! The per-launch local secret (S3, log-only).
//!
//! Today a loopback caller with no forwarder header, no foreign `Host` and no foreign `Origin` is
//! trusted as the desktop app (`requires_pairing_token`). Any local process passes that test. The
//! fix is a secret only the app's own clients know:
//!
//! - At bridge start, 32 random bytes are minted (hex) and written to
//!   `puppet-master.bridge.secret` next to the port file, through `secure_file::write_private`
//!   (atomic; 0600 on Unix; `%APPDATA%` is per-user on Windows). The port file itself is unchanged,
//!   so older readers of it keep working.
//! - The app's webview gets it over Tauri IPC (`bridge_local_secret`), never over HTTP.
//! - The MCP shim reads it from beside the port file it found.
//! - Every client sends it as `X-PM-Local-Secret`.
//!
//! This release only LOGS a local caller without it (`LOCAL_SECRET_MISSING`) and counts it for the
//! health panel; the request is still allowed. Enforcing comes once every client sends it
//! (roadmap 1-18).
//!
//! Only GENUINE local callers are counted: a request that came through the PWA proxy, the Vite
//! proxy or a tunnel (any forwarder header, a foreign `Host` or `Origin`) is a Device or Public
//! caller by design and is never "local without the key" (`requires_pairing_token`). The log line is
//! rate-limited per ROUTE CLASS (`GET /panes/{id}/buffer`, with any `/bridge` prefix removed), once
//! per ten minutes; the counter still counts every request, and the next line says how many were
//! held back.

use parking_lot::Mutex;
use rand::rngs::OsRng;
use rand::RngCore;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const HEADER: &str = "X-PM-Local-Secret";
pub const FILE_NAME: &str = "puppet-master.bridge.secret";
/// The same route class is logged at most this often (the counter still counts every request).
const LOG_EVERY: Duration = Duration::from_secs(10 * 60);

static SECRET: once_cell::sync::OnceCell<String> = once_cell::sync::OnceCell::new();

#[derive(Debug, Default, Clone, Serialize)]
pub struct Status {
    /// Whether this launch minted a secret (false only if the bridge never started).
    pub minted: bool,
    /// Local requests without the secret (or with a wrong one) since launch.
    pub missing_count: u64,
    /// Unix ms of the last one.
    pub last_missing_at: Option<u64>,
    /// `METHOD /route` of the last one (no query, no body).
    pub last_route: Option<String>,
    /// Always true in this release: missing secrets are logged, not refused.
    pub log_only: bool,
}

/// What the tally did with one request.
#[derive(Debug, PartialEq, Eq)]
enum Noted {
    /// Not a local caller without the key: has the key, or came through a proxy or tunnel.
    Ignored,
    /// Counted; its route class was already logged inside the window.
    Counted,
    /// Counted, and this is the line to log. `held_back` requests of the class were counted since the last line.
    Log { class: String, wrong: bool, held_back: u64 },
}

struct Tally {
    status: Status,
    /// Per route class: when it was last logged, and how many were counted since without a line.
    last_logged: std::collections::HashMap<String, (Instant, u64)>,
}

impl Tally {
    fn new() -> Self {
        Tally { status: Status { log_only: true, ..Status::default() }, last_logged: Default::default() }
    }

    /// One request the bridge trusts as the local app. `expected` is this launch's key.
    fn observe(&mut self, headers: &str, expected: Option<&str>, method: &str, path: &str, now: Instant, now_ms: u64) -> Noted {
        // Proxied requests (the PWA server, the Vite proxy, a tunnel) are Device/Public callers, not
        // local ones: they carry a paired phone's bearer token, not the app's key.
        if !super::is_local_only_caller(headers, true) {
            return Noted::Ignored;
        }
        let wrong = match check_against(headers, expected) {
            Check::Valid | Check::NotMinted => return Noted::Ignored,
            Check::Missing => false,
            Check::Wrong => true,
        };
        let class = route_class(method, path);
        self.status.missing_count += 1;
        self.status.last_missing_at = Some(now_ms);
        self.status.last_route = Some(class.clone());
        let entry = self.last_logged.entry(class.clone());
        use std::collections::hash_map::Entry;
        match entry {
            Entry::Vacant(slot) => {
                slot.insert((now, 0));
                Noted::Log { class, wrong, held_back: 0 }
            }
            Entry::Occupied(mut slot) => {
                let (at, held) = slot.get_mut();
                if now.saturating_duration_since(*at) >= LOG_EVERY {
                    let held_back = *held;
                    *at = now;
                    *held = 0;
                    Noted::Log { class, wrong, held_back }
                } else {
                    *held += 1;
                    Noted::Counted
                }
            }
        }
    }
}

static TALLY: once_cell::sync::Lazy<Mutex<Tally>> = once_cell::sync::Lazy::new(|| Mutex::new(Tally::new()));

/// `METHOD /route` with the `/bridge` prefix removed and every id-like segment (uuid, hex, number,
/// long token) written `{id}`, so `GET /bridge/panes/47c0…/buffer` and `GET /panes/9f2a…/buffer`
/// are one class.
fn route_class(method: &str, path: &str) -> String {
    let id_like = |seg: &str| {
        let hexish = seg.len() >= 8 && seg.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
        let number = !seg.is_empty() && seg.chars().all(|c| c.is_ascii_digit());
        let token = seg.len() >= 16 && seg.chars().filter(|c| c.is_ascii_digit()).count() >= 3;
        hexish || number || token
    };
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let segments = super::normalize_bridge_segments(&segments);
    let route: Vec<&str> = segments.iter().map(|seg| if id_like(seg) { "{id}" } else { *seg }).collect();
    format!("{method} /{}", route.join("/"))
}

/// `puppet-master.bridge.secret` beside the given port file.
pub fn file_beside(port_file: &Path) -> PathBuf {
    port_file.with_file_name(FILE_NAME)
}

fn mint() -> String {
    let mut bytes = [0_u8; 32];
    OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Mint this launch's secret (once per process) and write it beside the port file.
pub fn init(port_file: &Path) -> Result<&'static str, String> {
    let secret = SECRET.get_or_init(mint);
    crate::secure_file::write_private(&file_beside(port_file), secret)?;
    TALLY.lock().status.minted = true;
    Ok(secret.as_str())
}

/// This launch's secret, once the bridge has started.
pub fn current() -> Option<&'static str> {
    SECRET.get().map(String::as_str)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    Valid,
    Missing,
    Wrong,
    /// No secret minted in this process (unit tests that never start the bridge).
    NotMinted,
}

/// Compare the request's `X-PM-Local-Secret` with `expected` in constant time.
pub fn check_against(headers: &str, expected: Option<&str>) -> Check {
    let Some(expected) = expected else { return Check::NotMinted };
    match super::header_value(headers, HEADER) {
        None => Check::Missing,
        Some(given) if constant_time_eq(given.as_bytes(), expected.as_bytes()) => Check::Valid,
        Some(_) => Check::Wrong,
    }
}

pub fn check(headers: &str) -> Check {
    check_against(headers, current())
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0_u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Called for every request the bridge trusts as the local app. Log-only: never refuses.
pub fn observe_local_request(headers: &str, method: &str, path: &str) {
    let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    let noted = TALLY.lock().observe(headers, current(), method, path, Instant::now(), now_ms);
    if let Noted::Log { class, wrong, held_back } = noted {
        tracing::warn!(
            code = "LOCAL_SECRET_MISSING",
            route = %class,
            wrong,
            held_back,
            "a local caller reached the bridge without the app's key (allowed for now; logged once per route per 10 minutes)"
        );
        super::events::emit(super::events::SecurityEvent::LocalSecretMissing {
            method: method.to_string(),
            route: class,
            wrong,
        });
    }
}

/// The route classes that have been logged since launch (tests).
#[cfg(test)]
pub(crate) fn logged_classes() -> Vec<String> {
    TALLY.lock().last_logged.keys().cloned().collect()
}

pub fn status() -> Status {
    TALLY.lock().status.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_secret_is_compared_exactly() {
        let headers = "Host: 127.0.0.1\r\nX-PM-Local-Secret: abc123";
        assert_eq!(check_against(headers, Some("abc123")), Check::Valid);
        assert_eq!(check_against(headers, Some("abc124")), Check::Wrong);
        assert_eq!(check_against("Host: 127.0.0.1", Some("abc123")), Check::Missing);
        assert_eq!(check_against(headers, None), Check::NotMinted);
        // header names are case-insensitive
        assert_eq!(check_against("x-pm-local-secret: abc123", Some("abc123")), Check::Valid);
    }

    const KEY: Option<&str> = Some("abc123");
    const LOCAL: &str = "Host: 127.0.0.1:17321\r\nUser-Agent: puppet-master-mcp";

    fn note(t: &mut Tally, headers: &str, method: &str, path: &str, at: Instant) -> Noted {
        t.observe(headers, KEY, method, path, at, 1_000)
    }

    #[test]
    fn a_missing_secret_is_counted_and_never_refused() {
        let mut t = Tally::new();
        let now = Instant::now();
        assert!(matches!(note(&mut t, LOCAL, "POST", "/shell/exec", now), Noted::Log { .. }));
        // same route inside the window: counted, not re-logged
        assert_eq!(note(&mut t, LOCAL, "POST", "/shell/exec", now), Noted::Counted);
        assert_eq!(t.status.missing_count, 2);
        assert_eq!(t.status.last_route.as_deref(), Some("POST /shell/exec"));
        assert!(t.status.log_only);
        // a request that has the key is not counted
        let valid = format!("{LOCAL}\r\nX-PM-Local-Secret: abc123");
        assert_eq!(note(&mut t, &valid, "GET", "/panes", now), Noted::Ignored);
        assert_eq!(t.status.missing_count, 2);
        // a wrong key is counted and says so
        let wrong = format!("{LOCAL}\r\nX-PM-Local-Secret: nope");
        assert!(matches!(note(&mut t, &wrong, "GET", "/settings", now), Noted::Log { wrong: true, .. }));
    }

    /// Round 13 #1: every phone request proxied by `pwa_server` (`/bridge/*`) logged
    /// `LOCAL_SECRET_MISSING`. The proxy always adds `X-PM-Proxied`, so those are Device or Public
    /// callers, never "local without the key".
    #[test]
    fn proxied_and_tunnelled_requests_are_not_local_callers_without_the_key() {
        let mut t = Tally::new();
        let now = Instant::now();
        for forwarded in [
            "X-PM-Proxied: 1\r\nAuthorization: Bearer phone-token",
            "X-PM-Proxied: 1",
            "Cf-Connecting-Ip: 203.0.113.9",
            "X-Forwarded-For: 203.0.113.9",
            "Origin: https://evil.example",
        ] {
            let headers = format!("{LOCAL}\r\n{forwarded}");
            assert_eq!(note(&mut t, &headers, "GET", "/bridge/panes", now), Noted::Ignored, "{forwarded}");
        }
        assert_eq!(t.status.missing_count, 0);
        assert!(t.status.last_route.is_none());
        // a foreign Host (rebinding, a reverse proxy pointed at the port) is not local either
        assert_eq!(note(&mut t, "Host: pm.example.com", "GET", "/bridge/panes", now), Noted::Ignored);
        // ...while the same request from a genuine local caller is
        assert!(matches!(note(&mut t, LOCAL, "GET", "/bridge/panes", now), Noted::Log { .. }));
        assert_eq!(t.status.missing_count, 1);
    }

    #[test]
    fn route_classes_ignore_the_bridge_prefix_and_ids() {
        let id = "47c0b972-dfba-4f82-beff-45fecf29bd71";
        assert_eq!(route_class("GET", &format!("/bridge/panes/{id}/buffer")), "GET /panes/{id}/buffer");
        assert_eq!(route_class("GET", "/panes/21dbe44a-9957-4d29-bf14-e11acde56f87/buffer"), "GET /panes/{id}/buffer");
        assert_eq!(route_class("POST", "/mcp/mode"), "POST /mcp/mode");
        assert_eq!(route_class("GET", "/operations/657635b4/wait"), "GET /operations/{id}/wait");
        assert_eq!(route_class("GET", "/events/ws"), "GET /events/ws");
        assert_eq!(route_class("GET", "/bridge/panes"), "GET /panes");
        assert_eq!(route_class("GET", "/"), "GET /");
    }

    /// The flood: a PWA page opened from this machine (or an old MCP shim) calling the bridge
    /// without the key, over and over, on routes that carry a pane id.
    #[test]
    fn one_warning_per_route_class_per_ten_minutes_and_the_counter_keeps_counting() {
        let mut t = Tally::new();
        let t0 = Instant::now();
        let mut logged = 0;
        for i in 0..200 {
            // 4 panes x many polls, with and without the /bridge prefix
            let path = format!("/bridge/panes/47c0b972-dfba-4f82-beff-45fecf29bd{:02}/buffer", i % 4);
            if matches!(note(&mut t, LOCAL, "GET", &path, t0 + Duration::from_secs(i)), Noted::Log { .. }) {
                logged += 1;
            }
        }
        assert_eq!(logged, 1, "200 requests in 200 s to one route class: one line");
        assert_eq!(t.status.missing_count, 200, "the counter still counts every request");
        // another route class is its own line
        assert!(matches!(note(&mut t, LOCAL, "POST", "/mcp/mode", t0 + Duration::from_secs(201)), Noted::Log { held_back: 0, .. }));
        // after ten minutes the class logs again and says how many it held back
        let later = t0 + Duration::from_secs(10 * 60 + 1);
        assert_eq!(
            note(&mut t, LOCAL, "GET", "/bridge/panes/47c0b972-dfba-4f82-beff-45fecf29bd00/buffer", later),
            Noted::Log { class: "GET /panes/{id}/buffer".into(), wrong: false, held_back: 199 }
        );
        // and not again straight after
        assert_eq!(note(&mut t, LOCAL, "GET", "/bridge/panes/47c0b972-dfba-4f82-beff-45fecf29bd00/buffer", later), Noted::Counted);
    }

    #[test]
    fn init_writes_a_private_file_beside_the_port_file() {
        let dir = std::env::temp_dir().join(format!("pm-local-secret-{}", std::process::id()));
        let port_file = dir.join("puppet-master.bridge.port");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&port_file, "127.0.0.1:17321\n").unwrap();
        let secret = init(&port_file).unwrap();
        assert_eq!(secret.len(), 64);
        assert_eq!(std::fs::read_to_string(file_beside(&port_file)).unwrap(), secret);
        // the port file is untouched: older readers parse it as before
        assert_eq!(std::fs::read_to_string(&port_file).unwrap(), "127.0.0.1:17321\n");
        let _ = std::fs::remove_dir_all(dir);
    }
}
