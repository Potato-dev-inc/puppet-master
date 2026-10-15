//! Production mobile tunnel — bundled cloudflared + loopback PWA server.
//!
//! Which tunnel runs is decided by `tunnel_config` (quick | named | custom), with the
//! `PUPPET_MASTER_PUBLIC_URL` env var winning over the saved setting.
#![allow(dead_code)]

use parking_lot::Mutex;
use regex::Regex;
use serde::Serialize;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use once_cell::sync::OnceCell;
use tauri::path::BaseDirectory;
use tauri::{AppHandle, Manager};

use crate::app_paths;
use crate::pwa_server::{self, DevInfoPayload};
use crate::tunnel_config::{self, TunnelConfigInput, TunnelConfigView};

static TUNNEL_STATE: OnceCell<Arc<Mutex<TunnelState>>> = OnceCell::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// Nothing is running (disabled, or a dev build that never starts the tunnel).
    Stopped,
    Starting,
    Running,
    Error,
}

struct TunnelState {
    tunnel_url: Option<String>,
    tunnel_provider: Option<String>,
    bridge_direct_url: Option<String>,
    cloudflared_child: Option<Child>,
    pwa_server: Option<pwa_server::PwaServerHandle>,
    /// Effective mode after the env override: env | quick | named | custom.
    mode: &'static str,
    phase: Phase,
    last_error: Option<String>,
    /// Bumped on every (re)launch so a dead tunnel's log readers cannot overwrite the new state.
    generation: u64,
}

/// What the Settings tab and the status command show.
#[derive(Debug, Clone, Serialize)]
pub struct TunnelStatus {
    pub mode: &'static str,
    pub public_url: Option<String>,
    pub state: Phase,
    pub error: Option<String>,
    pub config: TunnelConfigView,
}

/// Result of "Test connection".
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConnectionTest {
    pub ok: bool,
    pub url: String,
    pub http_status: Option<u16>,
    pub latency_ms: u64,
    pub message: String,
}

impl TunnelState {
    fn new() -> Self {
        Self {
            tunnel_url: None,
            tunnel_provider: None,
            bridge_direct_url: None,
            cloudflared_child: None,
            pwa_server: None,
            mode: "quick",
            phase: Phase::Stopped,
            last_error: None,
            generation: 0,
        }
    }

    fn dev_info_payload(&self) -> DevInfoPayload {
        let local_url = format!("http://127.0.0.1:{}", pwa_server::pwa_port());
        let tunnel_url = self.tunnel_url.clone();
        let bridge_proxy_url = tunnel_url
            .as_ref()
            .map(|url| format!("{}/bridge", url.trim_end_matches('/')))
            .unwrap_or_else(|| format!("{local_url}/bridge"));

        DevInfoPayload {
            local_url: local_url.clone(),
            pwa_url: local_url,
            tunnel_url: tunnel_url.clone(),
            tunnel_provider: self.tunnel_provider.clone(),
            bridge_proxy_url,
            bridge_direct_url: self.bridge_direct_url.clone(),
            updated_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
        }
    }

    fn kill_child(&mut self) {
        if let Some(mut child) = self.cloudflared_child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn tunnel_state() -> Arc<Mutex<TunnelState>> {
    TUNNEL_STATE
        .get_or_init(|| Arc::new(Mutex::new(TunnelState::new())))
        .clone()
}

pub fn is_tunnel_enabled() -> bool {
    match std::env::var("PUPPET_MASTER_TUNNEL") {
        Ok(value) => value.trim() != "0",
        Err(_) => true,
    }
}

pub fn custom_public_url() -> Option<String> {
    let raw = std::env::var("PUPPET_MASTER_PUBLIC_URL").ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let without_bridge = trimmed.trim_end_matches('/').trim_end_matches("/bridge");
    Some(without_bridge.to_string())
}

pub fn get_mobile_tunnel_info() -> DevInfoPayload {
    tunnel_state().lock().dev_info_payload()
}

/// Current mode, public URL and running/error state (`get_tunnel_status`).
pub fn tunnel_status() -> TunnelStatus {
    let state = tunnel_state();
    let guard = state.lock();
    TunnelStatus {
        mode: guard.mode,
        public_url: guard.tunnel_url.clone(),
        state: guard.phase,
        error: guard.last_error.clone(),
        config: tunnel_config::load(&tunnel_config::tunnel_file()).view(),
    }
}

pub fn start_mobile_tunnel(app: &AppHandle, bridge_direct_url: String) -> Result<(), String> {
    if !is_tunnel_enabled() {
        tracing::info!("mobile tunnel disabled (PUPPET_MASTER_TUNNEL=0)");
        return Ok(());
    }

    let dist_root = resolve_pwa_dist_root(app)?;
    let bridge_port_file = app_paths::bridge_port_file();
    let state = tunnel_state();

    {
        let mut guard = state.lock();
        guard.bridge_direct_url = Some(bridge_direct_url.clone());
    }

    let dev_info_supplier: Arc<dyn Fn() -> DevInfoPayload + Send + Sync> =
        Arc::new(|| get_mobile_tunnel_info());
    let pwa_server = pwa_server::start_pwa_server(dist_root, bridge_port_file, dev_info_supplier)?;
    {
        let mut guard = state.lock();
        guard.pwa_server = Some(pwa_server);
    }

    // A second app on this computer (`PUPPET_MASTER_PROFILE`) opens no public address unless asked.
    if crate::app_paths::profile().is_some() && std::env::var("PUPPET_MASTER_TUNNEL").ok().as_deref() != Some("1") {
        tracing::info!("profile: public tunnel off (set PUPPET_MASTER_TUNNEL=1 to open one)");
        return Ok(());
    }

    launch_tunnel(app)
}

/// Stop the current tunnel process (if any) and start whatever the saved setting asks for.
/// The loopback PWA server keeps running; only the public side changes.
pub fn restart_mobile_tunnel(app: &AppHandle) -> Result<TunnelStatus, String> {
    if !is_tunnel_enabled() {
        return Err("The mobile tunnel is turned off (PUPPET_MASTER_TUNNEL=0).".into());
    }
    if tunnel_state().lock().pwa_server.is_none() {
        return Err("The tunnel only runs in the installed app, not in a dev build.".into());
    }
    launch_tunnel(app)?;
    Ok(tunnel_status())
}

/// Persist a new setting from the UI, then restart the tunnel so it takes effect.
pub fn apply_tunnel_config(
    app: &AppHandle,
    input: TunnelConfigInput,
) -> Result<TunnelStatus, String> {
    let path = tunnel_config::tunnel_file();
    let current = tunnel_config::load(&path);
    let next = tunnel_config::config_from_input(input, &current)?;
    tunnel_config::save(&path, &next)?;
    let restarted = restart_mobile_tunnel(app);
    let mut status = tunnel_status();
    if let Err(err) = restarted {
        // The setting is saved; say why it is not live yet.
        status.error = Some(err);
    }
    Ok(status)
}

fn launch_tunnel(app: &AppHandle) -> Result<(), String> {
    let state = tunnel_state();
    let config = tunnel_config::load(&tunnel_config::tunnel_file());
    let plan = tunnel_config::resolve_plan(custom_public_url(), &config);

    let generation = {
        let mut guard = state.lock();
        guard.kill_child();
        guard.generation += 1;
        guard.mode = plan.mode();
        guard.tunnel_url = plan.known_public_url();
        guard.last_error = None;
        guard.tunnel_provider = Some(
            match &plan {
                tunnel_config::TunnelPlan::Quick => "cloudflared",
                tunnel_config::TunnelPlan::Named { .. } => "cloudflared-named",
                _ => "custom",
            }
            .into(),
        );
        if plan.runs_cloudflared() {
            guard.phase = Phase::Starting;
        } else {
            guard.phase = Phase::Running;
            tracing::info!(
                mode = plan.mode(),
                url = %guard.tunnel_url.as_deref().unwrap_or(""),
                "using a fixed public URL for the mobile tunnel"
            );
            return Ok(());
        }
        guard.generation
    };

    let fail = |message: String| -> String {
        let mut guard = state.lock();
        guard.phase = Phase::Error;
        guard.last_error = Some(message.clone());
        message
    };

    let cloudflared_bin = resolve_cloudflared_bin(app).map_err(&fail)?;
    let local_target = format!("http://127.0.0.1:{}", pwa_server::pwa_port());
    let mut command = Command::new(&cloudflared_bin);
    command
        .args(tunnel_config::cloudflared_args(&plan, &local_target))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    match (&plan, &config) {
        (
            tunnel_config::TunnelPlan::Named { .. },
            tunnel_config::TunnelConfig::Named { token, .. },
        ) => {
            command.env(tunnel_config::TUNNEL_TOKEN_ENV, token);
        }
        _ => {
            command.env_remove(tunnel_config::TUNNEL_TOKEN_ENV);
        }
    }
    let named = matches!(plan, tunnel_config::TunnelPlan::Named { .. });
    let mut child = command.spawn().map_err(|err| {
        fail(format!(
            "spawn cloudflared {}: {err}",
            cloudflared_bin.display()
        ))
    })?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| fail("cloudflared stdout unavailable".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| fail("cloudflared stderr unavailable".into()))?;

    let readers: [(&str, Box<dyn std::io::Read + Send>); 2] = [
        ("cloudflared-stdout", Box::new(stdout)),
        ("cloudflared-stderr", Box::new(stderr)),
    ];
    for (name, reader) in readers {
        let state_for_reader = state.clone();
        thread::Builder::new()
            .name(name.into())
            .spawn(move || {
                watch_cloudflared_stream(BufReader::new(reader), state_for_reader, generation, named)
            })
            .map_err(|err| fail(format!("spawn {name} reader: {err}")))?;
    }

    state.lock().cloudflared_child = Some(child);
    tracing::info!(mode = plan.mode(), target = %local_target, "started cloudflared for the mobile PWA");
    Ok(())
}

fn watch_cloudflared_stream(
    reader: BufReader<impl std::io::Read>,
    state: Arc<Mutex<TunnelState>>,
    generation: u64,
    named: bool,
) {
    let url_re = Regex::new(r"https://[a-z0-9-]+\.trycloudflare\.com").expect("url regex");
    for line in reader.lines().map_while(Result::ok) {
        let mut guard = state.lock();
        if guard.generation != generation {
            return; // a newer launch owns the state
        }
        if named {
            match tunnel_config::classify_named_line(&line) {
                tunnel_config::NamedLine::Connected => {
                    guard.phase = Phase::Running;
                    guard.last_error = None;
                }
                tunnel_config::NamedLine::Failed(message) => {
                    guard.phase = Phase::Error;
                    guard.last_error = Some(message);
                }
                tunnel_config::NamedLine::Other => {}
            }
        } else if let Some(found) = url_re.find(&line) {
            let url = found.as_str().to_string();
            if guard.tunnel_url.as_deref() != Some(url.as_str()) {
                guard.tunnel_url = Some(url.clone());
                guard.phase = Phase::Running;
                tracing::info!(%url, "cloudflared public URL ready");
            }
        }
    }
    // Stream closed: cloudflared exited. A restart bumps the generation first, so this is a crash.
    let mut guard = state.lock();
    if guard.generation == generation && guard.phase != Phase::Error {
        guard.phase = Phase::Error;
        guard.last_error = Some("cloudflared stopped. Press Restart tunnel.".into());
    }
}

/// Interpret the answer of `<public>/bridge/health`. Pure so it is testable without a network.
pub fn interpret_health(
    url: &str,
    http_status: Option<u16>,
    body: &str,
    latency_ms: u64,
    transport_error: Option<&str>,
) -> ConnectionTest {
    let (ok, message) = match (http_status, transport_error) {
        (_, Some(err)) => (false, format!("Could not reach {url}: {err}")),
        (Some(200), None) => {
            let looks_like_pm = serde_json::from_str::<serde_json::Value>(body)
                .ok()
                .is_some_and(|v| v.get("ok").and_then(serde_json::Value::as_bool) == Some(true));
            if looks_like_pm {
                (true, format!("Connected in {latency_ms} ms."))
            } else {
                (
                    false,
                    "The address answered, but it is not Puppet Master. Check the proxy target."
                        .to_string(),
                )
            }
        }
        (Some(502 | 503 | 504 | 530), None) => (
            false,
            "The address is up, but it cannot reach Puppet Master. Is the tunnel running?"
                .to_string(),
        ),
        (Some(code), None) => (
            false,
            format!("The address answered with HTTP {code}, not Puppet Master's health check."),
        ),
        (None, None) => (false, "No answer.".to_string()),
    };
    ConnectionTest {
        ok,
        url: url.to_string(),
        http_status,
        latency_ms,
        message,
    }
}

/// Fetch `<public>/bridge/health` (blocking). `None` tests the tunnel's current public URL.
pub fn test_connection(public_url: Option<String>) -> Result<ConnectionTest, String> {
    let origin = match public_url.filter(|u| !u.trim().is_empty()) {
        Some(raw) => tunnel_config::normalize_origin(&raw, false)?,
        None => tunnel_state()
            .lock()
            .tunnel_url
            .clone()
            .ok_or_else(|| "No public address yet. Start the tunnel first.".to_string())?,
    };
    let url = tunnel_config::health_url(&origin);
    let started = std::time::Instant::now();
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(10))
        .build();
    let result = agent.get(&url).call();
    let latency = started.elapsed().as_millis() as u64;
    Ok(match result {
        Ok(resp) => {
            let status = resp.status();
            let body = resp.into_string().unwrap_or_default();
            interpret_health(&url, Some(status), &body, latency, None)
        }
        Err(ureq::Error::Status(code, resp)) => {
            let body = resp.into_string().unwrap_or_default();
            interpret_health(&url, Some(code), &body, latency, None)
        }
        Err(err) => interpret_health(&url, None, "", latency, Some(&err.to_string())),
    })
}

fn resolve_pwa_dist_root(app: &AppHandle) -> Result<PathBuf, String> {
    if let Ok(path) = app.path().resolve("pwa-dist", BaseDirectory::Resource) {
        if path.is_dir() {
            return Ok(path);
        }
    }

    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../dist");
    if dev.is_dir() {
        return Ok(dev);
    }

    Err("PWA dist not found — rebuild with stage-pwa-dist".into())
}

fn resolve_cloudflared_bin(app: &AppHandle) -> Result<PathBuf, String> {
    let bundled_name = if cfg!(target_os = "windows") {
        "bin/cloudflared.exe"
    } else {
        "bin/cloudflared"
    };

    if let Ok(path) = app.path().resolve(bundled_name, BaseDirectory::Resource) {
        if path.is_file() {
            return Ok(path);
        }
    }

    if let Ok(from_env) = std::env::var("CLOUDFLARED_BIN") {
        let path = PathBuf::from(from_env);
        if path.is_file() {
            return Ok(path);
        }
    }

    Err("bundled cloudflared not found — run bundle-cloudflared during build".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_public_url_strips_bridge_suffix() {
        std::env::set_var("PUPPET_MASTER_PUBLIC_URL", "https://example.com/bridge/");
        assert_eq!(custom_public_url().as_deref(), Some("https://example.com"));
        std::env::remove_var("PUPPET_MASTER_PUBLIC_URL");
    }

    #[test]
    fn health_interpretation() {
        let u = "https://pm.example.com/bridge/health";
        assert!(interpret_health(u, Some(200), r#"{"ok":true,"version":"1"}"#, 40, None).ok);
        assert!(!interpret_health(u, Some(200), "<html>hi</html>", 40, None).ok);
        let down = interpret_health(u, Some(530), "", 40, None);
        assert!(!down.ok && down.message.contains("tunnel"));
        assert!(!interpret_health(u, None, "", 5, Some("dns error")).ok);
        assert!(interpret_health(u, Some(404), "", 5, None).message.contains("404"));
    }

    #[test]
    fn test_connection_hits_a_live_health_endpoint() {
        // A tiny loopback server standing in for `<public>/bridge/health`.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            use std::io::{Read, Write};
            let (mut s, _) = listener.accept().unwrap();
            let mut buf = [0u8; 1024];
            let n = s.read(&mut buf).unwrap();
            let request = String::from_utf8_lossy(&buf[..n]).to_string();
            let body = r#"{"ok":true}"#;
            let _ = write!(
                s,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            request
        });
        let result = test_connection(Some(format!("http://127.0.0.1:{port}"))).unwrap();
        assert!(result.ok, "{result:?}");
        assert!(server.join().unwrap().starts_with("GET /bridge/health "));
    }
}
