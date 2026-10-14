//! `puppet-master-mcp hook <agent-id>`: the Hook Lane emitter (DESIGN-HOOK-LANE §3).
//!
//! An agent CLI runs this at its lifecycle points with the event JSON on stdin. It forwards the
//! JSON to `POST /hooks/event` on the loopback bridge named in its env and exits. The contract:
//!
//! - **always exit 0, print nothing** (stdout and stderr empty): a non-zero exit or any output is
//!   a "hook error" in the agent, and exit 2 would BLOCK it;
//! - **at most 300 ms** from start, whatever is in flight (a watchdog exits the process);
//! - **no env, no work**: without `PM_HOOK_TOKEN` / `PM_HOOK_PANE` / `PM_HOOK_BRIDGE` (the user runs
//!   the agent in their own terminal) it only drains stdin and exits;
//! - **no port probing, no port file**: the bridge address comes from env, set at spawn;
//! - **dumb**: it never interprets the payload, never runs or opens anything named in it;
//! - kill switch: `PM_HOOKS_OFF=1` or a file `<hooks dir>/disabled`.
//!
//! No lib state is touched (no registry, no MCP session, no catalog): this runs before the MCP
//! server would start, from `main`.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// The emitter contract version. Bump it only when §3.3 changes.
pub const HOOK_PROTOCOL: u32 = 1;
pub const BUDGET: Duration = Duration::from_millis(300);
const CONNECT_TIMEOUT: Duration = Duration::from_millis(100);
const IO_TIMEOUT: Duration = Duration::from_millis(150);
pub const MAX_STDIN: usize = 1024 * 1024;
pub const MAX_STRING: usize = 64 * 1024;
pub const MAX_BODY: usize = 512 * 1024;
const MAX_DEPTH: usize = 64;
const LOG_CAP: u64 = 1024 * 1024;

/// Entry from `main` when `args[1] == "hook"`. Never returns.
pub fn run(args: &[String]) -> ! {
    // Nothing may reach stderr, not even a panic message.
    std::panic::set_hook(Box::new(|_| std::process::exit(0)));
    if args.get(2).map(String::as_str) == Some("--protocol") {
        let _ = writeln!(std::io::stdout(), "{HOOK_PROTOCOL}");
        std::process::exit(0);
    }
    let started = Instant::now();
    std::thread::spawn(move || {
        std::thread::sleep(BUDGET.saturating_sub(started.elapsed()));
        std::process::exit(0);
    });
    let agent = args.get(2).cloned().unwrap_or_default();
    let env = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
    let target = match (env("PM_HOOK_TOKEN"), env("PM_HOOK_PANE"), env("PM_HOOK_BRIDGE")) {
        _ if killed() => None,
        (Some(token), Some(pane), Some(bridge)) if valid_agent(&agent) => Some((token, pane, bridge)),
        _ => None,
    };
    let Some((token, pane, bridge)) = target else {
        drain(&mut std::io::stdin().lock(), started);
        std::process::exit(0);
    };
    let (raw, total) = read_capped(&mut std::io::stdin().lock(), started);
    let body = body_for(&raw, total);
    if let Err(error) = post(&bridge, &token, &pane, &agent, &body) {
        log_error(&error);
    }
    std::process::exit(0);
}

fn killed() -> bool {
    std::env::var("PM_HOOKS_OFF").is_ok_and(|v| v == "1") || hooks_dir().join("disabled").exists()
}

/// `%APPDATA%\com.puppetmaster.app` (the folder the bridge port file lives in by default).
fn app_data_dir() -> PathBuf {
    let port_file = super::default_app_data_bridge_port_file();
    port_file.parent().map(PathBuf::from).unwrap_or_default()
}

fn hooks_dir() -> PathBuf {
    std::env::var_os("PUPPET_MASTER_HOOKS_DIR").map(PathBuf::from).unwrap_or_else(|| app_data_dir().join("hooks"))
}

fn valid_agent(agent: &str) -> bool {
    !agent.is_empty() && agent.len() <= 64 && agent.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// Read and throw away stdin until EOF or the budget runs out, so the agent's writer never sees a
/// broken pipe.
fn drain(input: &mut impl Read, started: Instant) {
    let mut buf = [0u8; 16 * 1024];
    while started.elapsed() < BUDGET {
        match input.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
    }
}

/// Keep the first [`MAX_STDIN`] bytes, drain the rest. Returns (kept, total bytes seen).
pub fn read_capped(input: &mut impl Read, started: Instant) -> (Vec<u8>, usize) {
    let mut kept = Vec::new();
    let mut total = 0usize;
    let mut buf = [0u8; 16 * 1024];
    while started.elapsed() < BUDGET {
        match input.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                total += n;
                let room = MAX_STDIN.saturating_sub(kept.len());
                kept.extend_from_slice(&buf[..n.min(room)]);
            }
        }
    }
    (kept, total)
}

/// The JSON body sent to the bridge: the vendor's JSON, shrunk; or only a marker when it is not
/// JSON (raw text is never forwarded).
pub fn body_for(raw: &[u8], total: usize) -> Vec<u8> {
    let parsed = if total > raw.len() { None } else { serde_json::from_slice::<serde_json::Value>(raw).ok() };
    let Some(mut value) = parsed else {
        return serde_json::to_vec(&serde_json::json!({ "unparsed": true, "bytes": total })).unwrap_or_default();
    };
    shrink(&mut value);
    serde_json::to_vec(&value).unwrap_or_default()
}

/// Generic shrink (§3.3.4): strings over 64 KiB are cut and marked; then, while the body is over
/// 512 KiB, the largest string is cut; nesting deeper than 64 becomes null.
pub fn shrink(value: &mut serde_json::Value) {
    cut_deep(value, 0);
    for _ in 0..64 {
        let size = serde_json::to_vec(value).map(|v| v.len()).unwrap_or(0);
        if size <= MAX_BODY {
            break;
        }
        let Some(largest) = largest_string(value) else { break };
        let excess = size - MAX_BODY;
        let keep = largest.len().saturating_sub(excess + 64);
        *largest = cut(largest, keep);
    }
}

fn cut_deep(value: &mut serde_json::Value, depth: usize) {
    if depth >= MAX_DEPTH && matches!(value, serde_json::Value::Array(_) | serde_json::Value::Object(_)) {
        *value = serde_json::Value::Null;
        return;
    }
    match value {
        serde_json::Value::String(s) if s.len() > MAX_STRING => *s = cut(s, MAX_STRING),
        serde_json::Value::Array(items) => items.iter_mut().for_each(|v| cut_deep(v, depth + 1)),
        serde_json::Value::Object(map) => map.values_mut().for_each(|v| cut_deep(v, depth + 1)),
        _ => {}
    }
}

fn largest_string(value: &mut serde_json::Value) -> Option<&mut String> {
    match value {
        serde_json::Value::String(s) => Some(s),
        serde_json::Value::Array(items) => items.iter_mut().filter_map(largest_string).max_by_key(|s| s.len()),
        serde_json::Value::Object(map) => map.values_mut().filter_map(largest_string).max_by_key(|s| s.len()),
        _ => None,
    }
}

fn cut(s: &str, keep: usize) -> String {
    let mut end = keep.min(s.len());
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…[cut {} bytes]", &s[..end], s.len() - end)
}

/// One POST over a raw socket (no DNS, no client crate). The reply is read and ignored.
pub fn post(bridge: &str, token: &str, pane: &str, agent: &str, body: &[u8]) -> Result<(), String> {
    let addr: SocketAddr = bridge.parse().map_err(|_| "bad bridge address".to_string())?;
    if !addr.ip().is_loopback() {
        return Err("bridge address is not loopback".into());
    }
    let clean = |s: &str| !s.contains(['\r', '\n']);
    if !clean(token) || !clean(pane) {
        return Err("bad header value".into());
    }
    let mut stream = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT).map_err(|e| format!("connect: {}", e.kind()))?;
    let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
    let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
    let head = format!(
        "POST /hooks/event HTTP/1.1\r\nHost: {addr}\r\nX-PM-Hook-Token: {token}\r\nX-PM-Hook-Pane: {pane}\r\nX-PM-Hook-Agent: {agent}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).and_then(|_| stream.write_all(body)).map_err(|e| format!("write: {}", e.kind()))?;
    let mut reply = [0u8; 256];
    let _ = stream.read(&mut reply);
    Ok(())
}

/// One line per error, no payload content; the file is restarted at 1 MB.
fn log_error(error: &str) {
    let dir = app_data_dir().join("logs");
    let path = dir.join("hook-emitter.log");
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > LOG_CAP) {
        let _ = std::fs::remove_file(&path);
    }
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let _ = writeln!(file, "{secs} {error}");
    }
}
