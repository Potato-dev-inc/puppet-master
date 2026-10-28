//! Preview origins: which local URLs the preview may open, and the navigation guard (PF-1).
//!
//! A URL is allowed only when it is plain `http` or `https` to exactly `localhost`, `127.0.0.1`
//! or `[::1]`, with no user info, and its port is not one of Puppet Master's own (SPEC n32 S1, S2,
//! S4). The host is checked as typed. A URL parser's normalising is not trusted: `0x7f.1` or
//! `localhost.` are refused, never rewritten to a loopback address.

use crate::app_paths::bridge_port_range_for;
use crate::pane_preview::PreviewUrl;

/// The loopback host names a preview may use (lower case).
const LOOPBACK_HOSTS: [&str; 3] = ["localhost", "127.0.0.1", "[::1]"];
/// The WebView2 remote-debugging port the founder's app uses (never previewed).
const CDP_PORT: u16 = 9333;

/// One local origin: `scheme://host:port`. `host` is `localhost`, `127.0.0.1` or `[::1]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewOrigin {
    pub scheme: String,
    pub host: String,
    pub port: u16,
}

impl PreviewOrigin {
    /// `scheme://host:port`, e.g. `http://localhost:5173`. The port is always written.
    pub fn as_str(&self) -> String {
        format!("{}://{}:{}", self.scheme, self.host, self.port)
    }

    /// True when `url` is this same origin (scheme, host and port). A refused URL never matches.
    pub fn matches(&self, url: &str) -> bool {
        parse(url).map_or(false, |origin| origin == *self)
    }
}

/// The ports Puppet Master uses itself. Previews never open these.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnPorts {
    pub ports: Vec<u16>,
    pub ranges: Vec<(u16, u16)>,
}

impl OwnPorts {
    /// The ports this app uses now: the phone server (1420 or the bound port), the bridge range,
    /// the WebView2 debugging port (from the environment, and 9333), and the dev server port.
    pub fn current(dev_url_port: Option<u16>) -> Self {
        let mut ports = vec![crate::pwa_server::PWA_PORT, crate::pwa_server::pwa_port(), CDP_PORT];
        if let Ok(args) = std::env::var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS") {
            ports.extend(cdp_port_in(&args));
        }
        ports.extend(dev_url_port);
        ports.sort_unstable();
        ports.dedup();
        OwnPorts { ports, ranges: vec![bridge_port_range_for(None)] }
    }

    pub fn contains(&self, port: u16) -> bool {
        self.ports.contains(&port) || self.ranges.iter().any(|&(lo, hi)| (lo..=hi).contains(&port))
    }
}

/// Why a preview cannot open an address. `message` is the sentence the person sees.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    NotRunning,
    NotLocal,
    OwnPort,
    BadUrl,
}

impl Refusal {
    pub fn message(&self) -> &'static str {
        match self {
            Refusal::NotRunning => "That server is not running any more.",
            Refusal::NotLocal => "Point and Fix only opens pages on this computer (localhost).",
            Refusal::OwnPort => "That port belongs to Puppet Master, so it cannot be previewed.",
            Refusal::BadUrl => "That is not a web address Point and Fix can open.",
        }
    }
}

/// What the preview window may do with a navigation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Nav {
    Allow,
    /// Cancel the navigation. `host` is shown in the notice ("Blocked a link to example.com.").
    Block { host: String },
}

/// The origin of `url` if it may be previewed: local loopback host, not one of our ports.
pub fn allowed(url: &str, own: &OwnPorts) -> Result<PreviewOrigin, Refusal> {
    let origin = parse(url)?;
    if own.contains(origin.port) {
        return Err(Refusal::OwnPort);
    }
    Ok(origin)
}

/// The URL to open for a pane's dev server `port`. The URL comes only from `previews` (what the
/// pane printed) or from `job_ports` (a background job's port), never from the person's input.
pub fn resolve_open(port: u16, previews: &[PreviewUrl], job_ports: &[u16]) -> Result<String, Refusal> {
    let url = match previews.iter().find(|preview| preview.port == port) {
        Some(preview) => preview.url.clone(),
        None if job_ports.contains(&port) => format!("http://localhost:{port}/"),
        None => return Err(Refusal::NotRunning),
    };
    let origin = allowed(&url, &OwnPorts::current(None))?;
    if origin.port != port {
        return Err(Refusal::BadUrl);
    }
    Ok(url)
}

/// The navigation guard for a preview window showing `origin`. The same origin, another local
/// origin that is not ours, and `about:blank` are allowed. Everything else is blocked.
pub fn nav(origin: &PreviewOrigin, url: &str) -> Nav {
    if url == "about:blank" || origin.matches(url) {
        return Nav::Allow;
    }
    match parse(url) {
        Ok(other) if !OwnPorts::current(None).contains(other.port) => Nav::Allow,
        _ => Nav::Block { host: host_for_notice(url) },
    }
}

/// The port in a WebView2 argument string, e.g. `--remote-debugging-port=9222`. None if absent or
/// not a valid port.
pub fn cdp_port_in(args: &str) -> Option<u16> {
    args.split_whitespace()
        .find_map(|token| token.strip_prefix("--remote-debugging-port="))
        .and_then(parse_port)
}

/// Parse `url` to a local origin, without the own-port check. Errors are `NotLocal` for a host
/// that is not loopback, and `BadUrl` for everything else.
fn parse(url: &str) -> Result<PreviewOrigin, Refusal> {
    if url.is_empty() || url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(Refusal::BadUrl);
    }
    let (scheme, rest) = url.split_once("://").ok_or(Refusal::BadUrl)?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return Err(Refusal::BadUrl);
    }
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    // User info ("http://localhost@evil.example") is never allowed.
    if authority.contains('@') {
        return Err(Refusal::BadUrl);
    }
    let (host_text, port_text) = split_host_port(authority)?;
    let host = host_text.to_ascii_lowercase();
    if !LOOPBACK_HOSTS.contains(&host.as_str()) {
        return Err(Refusal::NotLocal);
    }
    let port = match port_text {
        None if scheme == "https" => 443,
        None => 80,
        Some(text) => parse_port(text).ok_or(Refusal::BadUrl)?,
    };
    Ok(PreviewOrigin { scheme, host, port })
}

/// Split `host[:port]` or `[v6][:port]`. The host is returned as typed.
fn split_host_port(authority: &str) -> Result<(&str, Option<&str>), Refusal> {
    if authority.starts_with('[') {
        let close = authority.find(']').ok_or(Refusal::BadUrl)?;
        let host = &authority[..=close];
        let after = &authority[close + 1..];
        return match after {
            "" => Ok((host, None)),
            _ => after.strip_prefix(':').map(|port| (host, Some(port))).ok_or(Refusal::BadUrl),
        };
    }
    Ok(match authority.split_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (authority, None),
    })
}

/// A port from plain decimal digits, 1 to 65535 (no sign, no spaces, at most five digits).
fn parse_port(text: &str) -> Option<u16> {
    if text.is_empty() || text.len() > 5 || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    match text.parse::<u32>() {
        Ok(port) if (1..=65535).contains(&port) => Some(port as u16),
        _ => None,
    }
}

/// The host to show in a block notice: no scheme, user info, port or path. Never the full URL.
fn host_for_notice(url: &str) -> String {
    let Some((_, rest)) = url.split_once("://") else {
        return "an unknown address".to_string();
    };
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    let host_port = authority.rsplit_once('@').map_or(authority, |(_, host)| host);
    let host = match host_port.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or(""),
        None => host_port.split(':').next().unwrap_or(""),
    };
    if host.is_empty() {
        "an unknown address".to_string()
    } else {
        host.to_string()
    }
}
