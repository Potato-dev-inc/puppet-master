//! Dev-server URLs a pane has printed ("Local: http://localhost:5173/"), so the cockpit can offer
//! a Preview button. Agent-agnostic: it reads the visible screen tail, never an agent's name.
//!
//! Cost model: no work in the PTY reader. `observe` is called from `pane_activity::tick_with` for
//! the panes the activity ticker already decided to recompute (at most once per second per pane,
//! only when the screen changed), so the regex runs on a 24-line tail at that cadence and never
//! per byte. Per pane we keep the most recent distinct URLs (at most `MAX_PER_PANE`, one per
//! port); they are dropped when the pane is gone or its process has exited.
//!
//! Only loopback and private-LAN hosts count: a public URL in the output is a link, not a local
//! preview, and is never offered as one.

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use regex::Regex;
use serde::Serialize;
use std::collections::HashMap;

pub const MAX_PER_PANE: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewUrl {
    /// Address to open on this computer (`localhost` is used for `0.0.0.0` and `127.0.0.1`).
    pub url: String,
    pub port: u16,
    /// The LAN address the server printed for the same port ("Network: ..."), when it did.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_url: Option<String>,
}

static URL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r#"(?i)\bhttps?://(localhost|127\.0\.0\.1|\[::1\]|0\.0\.0\.0|192\.168\.\d{1,3}\.\d{1,3}|10\.\d{1,3}\.\d{1,3}\.\d{1,3}|172\.(?:1[6-9]|2\d|3[01])\.\d{1,3}\.\d{1,3}):(\d{2,5})(/[^\s'"<>)\]}]*)?"#,
    )
    .unwrap()
});

fn is_local_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "[::1]" | "0.0.0.0")
}

/// URLs in a screen tail, in reading order (later lines are newer). One entry per port; a local
/// address wins over the LAN one for the same port and carries it as `network_url`.
pub fn detect_urls(tail: &str) -> Vec<PreviewUrl> {
    let mut out: Vec<PreviewUrl> = Vec::new();
    for caps in URL_RE.captures_iter(tail) {
        let whole = caps.get(0).map_or("", |m| m.as_str());
        let host = caps[1].to_ascii_lowercase();
        let Ok(port) = caps[2].parse::<u16>() else { continue };
        if port == 0 {
            continue;
        }
        let scheme = if whole.to_ascii_lowercase().starts_with("https") { "https" } else { "http" };
        let path = caps.get(3).map_or("", |m| m.as_str()).trim_end_matches(['.', ',', ';', ':', '!']);
        let shown_host = if is_local_host(&host) { "localhost".to_string() } else { host.clone() };
        let url = format!("{scheme}://{shown_host}:{port}{path}");
        match out.iter_mut().find(|p| p.port == port) {
            Some(existing) => {
                if is_local_host(&host) {
                    if existing.network_url.is_none() && !existing.url.contains("localhost") {
                        existing.network_url = Some(existing.url.clone());
                    }
                    existing.url = url;
                } else if existing.network_url.is_none() && existing.url != url {
                    existing.network_url = Some(url);
                }
            }
            None if is_local_host(&host) => out.push(PreviewUrl { url, port, network_url: None }),
            None => out.push(PreviewUrl { url: url.clone(), port, network_url: Some(url) }),
        }
    }
    out
}

type Sink = Box<dyn Fn(&str, &[PreviewUrl]) + Send + Sync>;

#[derive(Default)]
struct State {
    /// Newest first.
    panes: HashMap<String, Vec<PreviewUrl>>,
    sink: Option<Sink>,
}

static STATE: Lazy<Mutex<State>> = Lazy::new(Default::default);

/// Where changes go (the Tauri event and the SSE push). Set once at startup.
pub fn install_sink(sink: Sink) {
    STATE.lock().sink = Some(sink);
}

/// The URLs of every pane that has any, newest first.
pub fn snapshot() -> HashMap<String, Vec<PreviewUrl>> {
    STATE.lock().panes.clone()
}

fn publish(state: &State, pane_id: &str, urls: &[PreviewUrl]) {
    if let Some(sink) = &state.sink {
        sink(pane_id, urls);
    }
}

/// Merge what a pane's tail shows into its list. New ports go to the front, known ports keep their
/// place (a re-print of the same server does not reshuffle), the list is capped. Emits only when
/// the list changed.
pub fn observe(pane_id: &str, tail: &str) {
    let found = detect_urls(tail);
    if found.is_empty() {
        return;
    }
    let mut state = STATE.lock();
    let before = state.panes.get(pane_id).cloned().unwrap_or_default();
    let mut list = before.clone();
    for item in found {
        match list.iter_mut().find(|p| p.port == item.port) {
            Some(known) => {
                if known.network_url.is_none() && item.network_url.is_some() {
                    known.network_url = item.network_url.clone();
                }
                if known.url != item.url && item.url.contains("localhost") {
                    known.url = item.url;
                }
            }
            None => list.insert(0, item),
        }
    }
    list.truncate(MAX_PER_PANE);
    if list != before {
        state.panes.insert(pane_id.to_string(), list.clone());
        publish(&state, pane_id, &list);
    }
}

/// Forget panes that are gone or whose process has exited, telling listeners with an empty list.
pub fn retain_alive(alive: &dyn Fn(&str) -> bool) {
    let mut state = STATE.lock();
    let gone: Vec<String> = state.panes.keys().filter(|id| !alive(id)).cloned().collect();
    for id in gone {
        state.panes.remove(&id);
        publish(&state, &id, &[]);
    }
}

#[cfg(test)]
mod tests;
