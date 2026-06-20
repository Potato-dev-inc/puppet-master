//! Production mobile tunnel — bundled cloudflared + loopback PWA server.

use parking_lot::Mutex;
use regex::Regex;
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
use crate::pwa_server::{self, DevInfoPayload, PWA_PORT};

static TUNNEL_STATE: OnceCell<Arc<Mutex<TunnelState>>> = OnceCell::new();

struct TunnelState {
    tunnel_url: Option<String>,
    tunnel_provider: Option<String>,
    bridge_direct_url: Option<String>,
    cloudflared_child: Option<Child>,
    _pwa_server: Option<pwa_server::PwaServerHandle>,
}

impl TunnelState {
    fn new() -> Self {
        Self {
            tunnel_url: None,
            tunnel_provider: None,
            bridge_direct_url: None,
            cloudflared_child: None,
            _pwa_server: None,
        }
    }

    fn dev_info_payload(&self) -> DevInfoPayload {
        let local_url = format!("http://127.0.0.1:{PWA_PORT}");
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
}

pub fn tunnel_state() -> Arc<Mutex<TunnelState>> {
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

pub fn start_mobile_tunnel(
    app: &AppHandle,
    bridge_direct_url: String,
) -> Result<(), String> {
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
    let pwa_server =
        pwa_server::start_pwa_server(dist_root, bridge_port_file, dev_info_supplier)?;
    {
        let mut guard = state.lock();
        guard._pwa_server = Some(pwa_server);
    }

    if let Some(custom) = custom_public_url() {
        let mut guard = state.lock();
        guard.tunnel_url = Some(custom);
        guard.tunnel_provider = Some("custom".into());
        tracing::info!(url = %guard.tunnel_url.as_deref().unwrap_or(""), "using custom public URL for mobile tunnel");
        return Ok(());
    }

    let cloudflared_bin = resolve_cloudflared_bin(app)?;
    let local_target = format!("http://127.0.0.1:{PWA_PORT}");
    let mut child = Command::new(&cloudflared_bin)
        .args(["tunnel", "--url", &local_target])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("spawn cloudflared {}: {err}", cloudflared_bin.display()))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "cloudflared stdout unavailable".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "cloudflared stderr unavailable".to_string())?;

    let state_for_reader = state.clone();
    thread::Builder::new()
        .name("cloudflared-stdout".into())
        .spawn(move || watch_cloudflared_stream(BufReader::new(stdout), state_for_reader))
        .map_err(|err| format!("spawn cloudflared stdout reader: {err}"))?;

    let state_for_reader = state.clone();
    thread::Builder::new()
        .name("cloudflared-stderr".into())
        .spawn(move || watch_cloudflared_stream(BufReader::new(stderr), state_for_reader))
        .map_err(|err| format!("spawn cloudflared stderr reader: {err}"))?;

    {
        let mut guard = state.lock();
        guard.cloudflared_child = Some(child);
        guard.tunnel_provider = Some("cloudflared".into());
    }

    tracing::info!(target = %local_target, "started cloudflared quick tunnel for mobile PWA");
    Ok(())
}

fn watch_cloudflared_stream(reader: BufReader<impl std::io::Read>, state: Arc<Mutex<TunnelState>>) {
    let url_re = Regex::new(r"https://[a-z0-9-]+\.trycloudflare\.com").expect("url regex");
    for line in reader.lines().map_while(Result::ok) {
        if let Some(found) = url_re.find(&line) {
            let url = found.as_str().to_string();
            let mut guard = state.lock();
            if guard.tunnel_url.as_deref() != Some(url.as_str()) {
                guard.tunnel_url = Some(url.clone());
                tracing::info!(%url, "cloudflared public URL ready");
            }
        }
    }
}

fn resolve_pwa_dist_root(app: &AppHandle) -> Result<PathBuf, String> {
    if let Ok(path) = app
        .path()
        .resolve("pwa-dist", BaseDirectory::Resource)
    {
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

    Err(format!(
        "bundled cloudflared not found — run bundle-cloudflared during build"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_public_url_strips_bridge_suffix() {
        std::env::set_var("PUPPET_MASTER_PUBLIC_URL", "https://example.com/bridge/");
        assert_eq!(
            custom_public_url().as_deref(),
            Some("https://example.com")
        );
        std::env::remove_var("PUPPET_MASTER_PUBLIC_URL");
    }
}
