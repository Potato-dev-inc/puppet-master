//! Minimal OpenCode REST client (v1 API). See https://opencode.ai/docs/server

use serde::Deserialize;
use serde::Serialize;
use serde_json::json;
use std::time::{Duration, Instant};

mod interactions;
mod models;

pub use interactions::*;
pub use models::*;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
/// An abort is a fire-and-forget nudge; a server that does not answer in this long is gone, and
/// cancel must still finish inside the MCP shim's read timeout.
const ABORT_TIMEOUT: Duration = Duration::from_secs(5);
/// Health polling: serve usually comes up in ~1.2s, so poll tightly with a short
/// per-request timeout; the overall cap stays the caller's `timeout`.
const HEALTH_POLL_INTERVAL: Duration = Duration::from_millis(50);
const HEALTH_REQUEST_TIMEOUT: Duration = Duration::from_secs(1);
/// The idle watcher asks once a second at most; a server that does not answer in this long is
/// treated as unknown, and the screen alone decides.
const SESSION_STATUS_TIMEOUT: Duration = Duration::from_secs(2);
/// Pauses before each new try of a request that never reached the server (its port refused the
/// connection). `serve` restarting or still binding answers again within a second or two, and the
/// run that was about to send would otherwise fail on the first refusal. Windows takes about 2 s to
/// refuse a connection to a closed loopback port, so four tries are about 10 s at most.
const CONNECT_RETRY_DELAYS: [Duration; 3] = [
    Duration::from_millis(300),
    Duration::from_millis(700),
    Duration::from_millis(1500),
];

fn encode_query_component(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                char::from(byte).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

pub fn with_directory_query(url: &str, directory: Option<&str>) -> String {
    let Some(directory) = directory.map(str::trim).filter(|value| !value.is_empty()) else {
        return url.to_string();
    };
    let separator = if url.contains('?') { '&' } else { '?' };
    format!(
        "{url}{separator}directory={}",
        encode_query_component(directory)
    )
}

pub fn normalize_base_url(base_url: &str) -> String {
    base_url.trim_end_matches('/').to_string()
}

pub fn wait_for_health(base_url: &str, timeout: Duration) -> Result<(), String> {
    let base = normalize_base_url(base_url);
    let url = format!("{base}/global/health");
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        match ureq::get(&url).timeout(HEALTH_REQUEST_TIMEOUT).call() {
            Ok(response) if response.status() == 200 => return Ok(()),
            _ => std::thread::sleep(HEALTH_POLL_INTERVAL),
        }
    }
    Err(format!(
        "OpenCode server not healthy at {base} within {timeout:?}"
    ))
}

/// Whether `error` says the connection was never made, so the server cannot have seen the request
/// and sending it again cannot do it twice. A timeout or a reset is NOT one: the request may have
/// arrived.
pub fn is_connect_failure(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    ["connection failed", "connection refused", "10061", "tcp connect", "dns failed"]
        .iter()
        .any(|marker| lower.contains(marker))
        && !lower.contains("http 4")
        && !lower.contains("http 5")
}

/// Run `op`; while it fails with an error `never_arrived` accepts, wait the next delay and run it
/// again. Any other error, or running out of delays, returns the last error. `sleep` is injected.
fn retry_never_arrived<T, E>(
    delays: &[Duration],
    mut sleep: impl FnMut(Duration),
    never_arrived: impl Fn(&E) -> bool,
    mut op: impl FnMut() -> Result<T, E>,
) -> Result<T, E> {
    let mut delays = delays.iter();
    loop {
        match op() {
            Err(error) if never_arrived(&error) => match delays.next() {
                Some(delay) => sleep(*delay),
                None => return Err(error),
            },
            other => return other,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct OpenCodeSession {
    pub id: String,
}

pub fn create_session(base_url: &str, title: &str) -> Result<OpenCodeSession, String> {
    let base = normalize_base_url(base_url);
    let url = format!("{base}/session");
    let response = retry_never_arrived(&CONNECT_RETRY_DELAYS, std::thread::sleep, |e: &String| is_connect_failure(e), || {
        ureq::post(&url)
            .timeout(DEFAULT_TIMEOUT)
            .send_json(json!({ "title": title }))
            .map_err(|err| format!("opencode create session: {err}"))
    })?;
    if !(200..300).contains(&response.status()) {
        return Err(format!(
            "opencode create session: HTTP {} {}",
            response.status(),
            response.into_string().unwrap_or_default()
        ));
    }
    response
        .into_json::<OpenCodeSession>()
        .map_err(|err| format!("opencode create session parse: {err}"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptAsyncError {
    pub status: u16,
    pub body: String,
}

impl PromptAsyncError {
    pub fn into_message(self) -> String {
        format!("opencode prompt_async: HTTP {} {}", self.status, self.body)
    }
}

pub fn is_rate_limit_status(status: u16, body: &str) -> bool {
    if status == 429 || status == 402 {
        return true;
    }
    let lower = body.to_lowercase();
    ["rate limit", "quota", "usage", "too many requests"]
        .iter()
        .any(|keyword| lower.contains(keyword))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenCodeModelRef {
    #[serde(rename = "providerID")]
    pub provider_id: String,
    #[serde(rename = "modelID")]
    pub model_id: String,
}

pub fn switch_session_model_body(model: &OpenCodeModelRef) -> serde_json::Value {
    json!({
        "model": {
            "id": model.model_id,
            "providerID": model.provider_id,
            "variant": "default",
        }
    })
}

/// Switch the session default model (updates TUI footer, not just one prompt).
pub fn switch_session_model(
    base_url: &str,
    session_id: &str,
    directory: Option<&str>,
    model: &OpenCodeModelRef,
) -> Result<(), String> {
    let base = normalize_base_url(base_url);
    let url = with_directory_query(&format!("{base}/api/session/{session_id}/model"), directory);
    let response = retry_never_arrived(&CONNECT_RETRY_DELAYS, std::thread::sleep, |e: &String| is_connect_failure(e), || {
        ureq::post(&url)
            .timeout(DEFAULT_TIMEOUT)
            .send_json(switch_session_model_body(model))
            .map_err(|err| format!("opencode switch session model: {err}"))
    })?;
    if response.status() == 204 || (200..300).contains(&response.status()) {
        return Ok(());
    }
    Err(format!(
        "opencode switch session model: HTTP {} {}",
        response.status(),
        response.into_string().unwrap_or_default()
    ))
}

pub fn prompt_async_body(text: &str, model: Option<&OpenCodeModelRef>) -> serde_json::Value {
    let mut body = json!({
        "parts": [{ "type": "text", "text": text }],
    });
    if let Some(model) = model {
        body["model"] = json!({
            "providerID": model.provider_id,
            "modelID": model.model_id,
        });
    }
    body
}

pub fn prompt_async(
    base_url: &str,
    session_id: &str,
    directory: Option<&str>,
    text: &str,
    model: Option<&OpenCodeModelRef>,
) -> Result<(), PromptAsyncError> {
    let base = normalize_base_url(base_url);
    let url = with_directory_query(
        &format!("{base}/session/{session_id}/prompt_async"),
        directory,
    );
    let response = retry_never_arrived(
        &CONNECT_RETRY_DELAYS,
        std::thread::sleep,
        |e: &PromptAsyncError| e.status == 0 && is_connect_failure(&e.body),
        || {
            ureq::post(&url)
                .timeout(DEFAULT_TIMEOUT)
                .send_json(prompt_async_body(text, model))
                .map_err(|err| PromptAsyncError {
                    status: 0,
                    body: format!("opencode prompt_async: {err}"),
                })
        },
    )?;
    if response.status() == 204 || (200..300).contains(&response.status()) {
        return Ok(());
    }
    let status = response.status();
    let body = response.into_string().unwrap_or_default();
    Err(PromptAsyncError { status, body })
}

/// Whether an error from this client means the OpenCode server itself could not be reached
/// (refused, reset, not listening), as opposed to the server answering with an error.
pub fn is_unreachable_error(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    [
        "connection failed",
        "connection refused",
        "10061",
        "connection reset",
        "error sending request",
        "tcp connect",
        "timed out",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
        && !lower.contains("http 4")
        && !lower.contains("http 5")
}

pub fn abort_session(
    base_url: &str,
    session_id: &str,
    directory: Option<&str>,
) -> Result<(), String> {
    let base = normalize_base_url(base_url);
    let url = with_directory_query(&format!("{base}/session/{session_id}/abort"), directory);
    let response = ureq::post(&url)
        .timeout(ABORT_TIMEOUT)
        .call()
        .map_err(|err| format!("opencode abort session: {err}"))?;
    if response.status() == 204 || (200..300).contains(&response.status()) {
        return Ok(());
    }
    Err(format!(
        "opencode abort session: HTTP {} {}",
        response.status(),
        response.into_string().unwrap_or_default()
    ))
}

pub fn health_ok(base_url: &str) -> bool {
    let base = normalize_base_url(base_url);
    let url = format!("{base}/global/health");
    ureq::get(&url)
        .timeout(Duration::from_secs(3))
        .call()
        .map(|r| r.status() == 200)
        .unwrap_or(false)
}

/// Whether the server reports `session_id` as working (`busy`, or `retry` while it waits to try
/// again). `GET /session/status` answers a map of session id to `{"type": ...}` and leaves idle
/// sessions out. Read from the server's own state, so it holds when the TUI screen is static (a
/// long tool call) and when the last message has no `finish` yet.
pub fn session_is_busy(
    base_url: &str,
    session_id: &str,
    directory: Option<&str>,
) -> Result<bool, String> {
    let base = normalize_base_url(base_url);
    let url = with_directory_query(&format!("{base}/session/status"), directory);
    let response = ureq::get(&url)
        .timeout(SESSION_STATUS_TIMEOUT)
        .call()
        .map_err(|err| format!("opencode session status: {err}"))?;
    let body: serde_json::Value = response
        .into_json()
        .map_err(|err| format!("opencode session status parse: {err}"))?;
    session_busy_in(&body, session_id)
        .ok_or_else(|| "opencode session status: not a map of sessions".to_string())
}

/// [`session_is_busy`] on an already-read body. `None` when the body is not a map.
pub fn session_busy_in(body: &serde_json::Value, session_id: &str) -> Option<bool> {
    let map = body.as_object()?;
    Some(map.get(session_id).is_some_and(|entry| {
        entry
            .get("type")
            .and_then(serde_json::Value::as_str)
            .is_none_or(|kind| !kind.eq_ignore_ascii_case("idle"))
    }))
}

#[cfg(test)]
mod tests;
