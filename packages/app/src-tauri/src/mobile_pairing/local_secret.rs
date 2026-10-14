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
//! This release only LOGS a local caller without it (`LOCAL_SECRET_MISSING`, rate-limited) and
//! counts it for the health panel; the request is still allowed. Enforcing comes once every client
//! sends it (roadmap 1-18).

use parking_lot::Mutex;
use rand::rngs::OsRng;
use rand::RngCore;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const HEADER: &str = "X-PM-Local-Secret";
pub const FILE_NAME: &str = "puppet-master.bridge.secret";
/// The same route is logged at most this often (the counter still counts every request).
const LOG_EVERY: Duration = Duration::from_secs(60);

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

struct Tally {
    status: Status,
    last_logged: std::collections::HashMap<String, Instant>,
}

static TALLY: once_cell::sync::Lazy<Mutex<Tally>> = once_cell::sync::Lazy::new(|| {
    Mutex::new(Tally { status: Status { log_only: true, ..Status::default() }, last_logged: Default::default() })
});

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
    let wrong = match check(headers) {
        Check::Valid | Check::NotMinted => return,
        Check::Missing => false,
        Check::Wrong => true,
    };
    note_missing(method, path, wrong);
}

fn note_missing(method: &str, path: &str, wrong: bool) {
    let route = format!("{method} {path}");
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let should_log = {
        let mut tally = TALLY.lock();
        tally.status.missing_count += 1;
        tally.status.last_missing_at = Some(now_ms);
        tally.status.last_route = Some(route.clone());
        let due = tally
            .last_logged
            .get(&route)
            .is_none_or(|at| at.elapsed() >= LOG_EVERY);
        if due {
            tally.last_logged.insert(route.clone(), Instant::now());
        }
        due
    };
    if should_log {
        tracing::warn!(
            code = "LOCAL_SECRET_MISSING",
            %route,
            wrong,
            "a local caller reached the bridge without the app's key (allowed for now)"
        );
        super::events::emit(super::events::SecurityEvent::LocalSecretMissing {
            method: method.to_string(),
            route: path.to_string(),
            wrong,
        });
    }
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

    #[test]
    fn a_missing_secret_is_counted_and_never_refused() {
        let before = status().missing_count;
        note_missing("POST", "/shell/exec", false);
        note_missing("POST", "/shell/exec", false); // same route inside a minute: counted, not re-logged
        let after = status();
        assert!(after.missing_count >= before + 2);
        assert_eq!(after.last_route.as_deref().map(|r| r.starts_with("POST ")), Some(true));
        assert!(after.log_only);
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
