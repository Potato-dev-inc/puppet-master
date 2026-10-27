//! OpenCode's chat from its local HTTP API (`crate::session_chat::SessionApi`): the conversation's messages, with
//! each tool call's input, files, output and diff, the task list, token use and turn length. The same API serves the
//! native worker's pane (`opencode serve`, whose session Puppet Master created) and the plain TUI, which is started
//! with `--port` so it serves its own (its session is the newest one this pane started).
//!
//! `map` turns messages into chat items; this file asks the server.

mod map;

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::{Duration, Instant, UNIX_EPOCH};

use parking_lot::Mutex;
use serde_json::Value;

use crate::opencode::client::{normalize_base_url, with_directory_query};
use crate::pty::registry::PaneState;
use crate::session_chat::{plain_path, ApiEndpoint, ChatItem, PaneFacts, SessionApi};

/// A server that does not answer in this long counts as not reachable; the reader asks again a second later.
const TIMEOUT: Duration = Duration::from_secs(3);
/// The model list is large (every provider): it is read once and the answer kept this long.
const LIMITS_FRESH: Duration = Duration::from_secs(300);
/// Sessions of a folder that were not touched since the pane started are not this pane's.
const SESSION_SLACK_MS: u64 = 2_000;

pub struct OpenCodeApi;

pub static API: OpenCodeApi = OpenCodeApi;

impl SessionApi for OpenCodeApi {
    fn fetch(&self, pane: &PaneFacts, endpoint: &ApiEndpoint) -> Result<Vec<ChatItem>, String> {
        let base = normalize_base_url(&endpoint.base_url);
        let directory = plain_path(&pane.cwd.to_string_lossy());
        let session = match &endpoint.session_id {
            Some(id) => id.clone(),
            None => resolve_session(&base, &directory, pane)?,
        };
        let url = with_directory_query(&format!("{base}/session/{session}/message"), Some(&directory));
        let messages = get_json(&url)?;
        let list = messages.as_array().ok_or("opencode messages: not a list")?;
        Ok(map::map_messages(list, &|provider, model| context_window(&base, &directory, provider, model)))
    }
}

/// Where a TUI pane's own server is: the port its launch arguments named (`ports`), no session yet.
pub(super) fn tui_endpoint(pane: &PaneState) -> Option<ApiEndpoint> {
    let port = ports().lock().get(&pane.info.id).copied()?;
    Some(ApiEndpoint { base_url: format!("http://127.0.0.1:{port}"), session_id: None })
}

/// Where a native worker's server is, and the session Puppet Master made on it.
pub(super) fn native_endpoint(pane: &PaneState) -> Option<ApiEndpoint> {
    let link = pane.opencode.as_ref()?;
    Some(ApiEndpoint { base_url: link.base_url.clone(), session_id: Some(link.session_id.clone()) })
}

/// The port each TUI pane was told to serve on.
fn ports() -> &'static Mutex<HashMap<String, u16>> {
    static PORTS: OnceLock<Mutex<HashMap<String, u16>>> = OnceLock::new();
    PORTS.get_or_init(Mutex::default)
}

/// A free local port, or `None` when the system gives none.
fn free_port() -> Option<u16> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).ok()?;
    listener.local_addr().ok().map(|addr| addr.port())
}

/// Launch arguments that make the TUI serve its API on a port Puppet Master knows. Nothing when the person chose
/// a port or a server themselves.
pub(super) fn prepare(pane_id: &str, user_args: &[String]) -> Vec<String> {
    let chosen = user_args.iter().any(|arg| arg == "--port" || arg.starts_with("--port=") || arg == "--hostname" || arg.starts_with("--hostname="));
    let port = if chosen { None } else { free_port() };
    match port {
        Some(port) => {
            ports().lock().insert(pane_id.to_string(), port);
            vec!["--port".into(), port.to_string(), "--hostname".into(), "127.0.0.1".into()]
        }
        None => {
            ports().lock().remove(pane_id);
            Vec::new()
        }
    }
}

fn get_json(url: &str) -> Result<Value, String> {
    let response = ureq::get(url).timeout(TIMEOUT).call().map_err(|error| format!("opencode api: {error}"))?;
    response.into_json::<Value>().map_err(|error| format!("opencode api parse: {error}"))
}

/// Which conversation of the server is this pane's: one session of the folder, not a helper's, touched since the
/// pane started. A server that is working on one (`/session/status`) wins; otherwise the one this pane showed
/// before while it is still a candidate, otherwise the one touched last.
fn resolve_session(base: &str, directory: &str, pane: &PaneFacts) -> Result<String, String> {
    let started = pane.started_at.duration_since(UNIX_EPOCH).map(|since| since.as_millis() as u64).unwrap_or(0).saturating_sub(SESSION_SLACK_MS);
    let sessions = get_json(&with_directory_query(&format!("{base}/session"), Some(directory)))?;
    let mut candidates: Vec<(&str, u64)> = sessions
        .as_array()
        .into_iter()
        .flatten()
        .filter(|session| session["parentID"].is_null())
        .filter_map(|session| Some((session["id"].as_str()?, session["time"]["updated"].as_u64()?)))
        .filter(|(_, updated)| *updated >= started)
        .collect();
    candidates.sort_by_key(|(_, updated)| std::cmp::Reverse(*updated));
    let busy = get_json(&with_directory_query(&format!("{base}/session/status"), Some(directory))).ok();
    let working = |id: &str| busy.as_ref().and_then(|map| map.get(id)).is_some_and(|entry| entry["type"] != "idle");
    let mut known = known_sessions().lock();
    let chosen = candidates
        .iter()
        .find(|(id, _)| working(id))
        .or_else(|| candidates.iter().find(|(id, _)| known.get(base).is_some_and(|known| known == id)))
        .or_else(|| candidates.first())
        .map(|(id, _)| id.to_string())
        .ok_or("opencode api: this pane has no session yet")?;
    known.insert(base.to_string(), chosen.clone());
    Ok(chosen)
}

/// The session each server's pane showed last.
fn known_sessions() -> &'static Mutex<HashMap<String, String>> {
    static KNOWN: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    KNOWN.get_or_init(Mutex::default)
}

/// The context window of a model, from the server's model list. Asked once per server and kept for a while; a
/// server that cannot say gives `None` (the view then shows no percentage).
fn context_window(base: &str, directory: &str, provider: &str, model: &str) -> Option<u64> {
    static LIMITS: OnceLock<Mutex<HashMap<String, (Instant, HashMap<String, u64>)>>> = OnceLock::new();
    let limits = LIMITS.get_or_init(Mutex::default);
    let mut limits = limits.lock();
    let fresh = limits.get(base).is_some_and(|(read, _)| read.elapsed() < LIMITS_FRESH);
    if !fresh {
        // Asked even when it fails, so a server without the list is not asked at every look.
        let read = get_json(&with_directory_query(&format!("{base}/config/providers"), Some(directory))).ok();
        limits.insert(base.to_string(), (Instant::now(), read.as_ref().map(window_table).unwrap_or_default()));
    }
    limits.get(base)?.1.get(&format!("{provider}/{model}")).copied()
}

/// `"<provider>/<model>"` to context window, from the model list. Nothing else of the list is kept.
fn window_table(list: &Value) -> HashMap<String, u64> {
    let mut table = HashMap::new();
    for provider in list["providers"].as_array().into_iter().flatten() {
        let Some(provider_id) = provider["id"].as_str() else {
            continue;
        };
        for (model_id, model) in provider["models"].as_object().into_iter().flatten() {
            if let Some(window) = model["limit"]["context"].as_u64().filter(|window| *window > 0) {
                table.insert(format!("{provider_id}/{model_id}"), window);
            }
        }
    }
    table
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
