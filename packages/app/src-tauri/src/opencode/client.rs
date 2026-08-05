//! Minimal OpenCode REST client (v1 API). See https://opencode.ai/docs/server

use serde::Deserialize;
use serde::Serialize;
use serde_json::json;
use std::time::{Duration, Instant};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

pub fn normalize_base_url(base_url: &str) -> String {
    base_url.trim_end_matches('/').to_string()
}

pub fn wait_for_health(base_url: &str, timeout: Duration) -> Result<(), String> {
    let base = normalize_base_url(base_url);
    let url = format!("{base}/global/health");
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        match ureq::get(&url).timeout(DEFAULT_TIMEOUT).call() {
            Ok(response) if response.status() == 200 => return Ok(()),
            _ => std::thread::sleep(Duration::from_millis(250)),
        }
    }
    Err(format!("OpenCode server not healthy at {base} within {timeout:?}"))
}

#[derive(Debug, Clone, Deserialize)]
pub struct OpenCodeSession {
    pub id: String,
}

pub fn create_session(base_url: &str, title: &str) -> Result<OpenCodeSession, String> {
    let base = normalize_base_url(base_url);
    let url = format!("{base}/session");
    let response = ureq::post(&url)
        .timeout(DEFAULT_TIMEOUT)
        .send_json(json!({ "title": title }))
        .map_err(|err| format!("opencode create session: {err}"))?;
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

pub fn prompt_async(base_url: &str, session_id: &str, text: &str) -> Result<(), String> {
    let base = normalize_base_url(base_url);
    let url = format!("{base}/session/{session_id}/prompt_async");
    let response = ureq::post(&url)
        .timeout(DEFAULT_TIMEOUT)
        .send_json(json!({
            "parts": [{ "type": "text", "text": text }],
        }))
        .map_err(|err| format!("opencode prompt_async: {err}"))?;
    if response.status() == 204 || (200..300).contains(&response.status()) {
        return Ok(());
    }
    Err(format!(
        "opencode prompt_async: HTTP {} {}",
        response.status(),
        response.into_string().unwrap_or_default()
    ))
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OpenCodePermission {
    pub id: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(rename = "sessionID", default)]
    pub session_id_camel: Option<String>,
}

pub fn list_permissions(base_url: &str) -> Result<Vec<OpenCodePermission>, String> {
    let base = normalize_base_url(base_url);
    let url = format!("{base}/permission");
    let response = ureq::get(&url)
        .timeout(DEFAULT_TIMEOUT)
        .call()
        .map_err(|err| format!("opencode list permissions: {err}"))?;
    if !(200..300).contains(&response.status()) {
        return Err(format!("opencode list permissions: HTTP {}", response.status()));
    }
    response
        .into_json::<Vec<OpenCodePermission>>()
        .map_err(|err| format!("opencode list permissions parse: {err}"))
}

pub fn reply_permission(base_url: &str, request_id: &str, reply: &str) -> Result<(), String> {
    let base = normalize_base_url(base_url);
    let url = format!("{base}/permission/{request_id}/reply");
    let response = ureq::post(&url)
        .timeout(DEFAULT_TIMEOUT)
        .send_json(json!({ "reply": reply }))
        .map_err(|err| format!("opencode permission reply: {err}"))?;
    if (200..300).contains(&response.status()) {
        return Ok(());
    }
    Err(format!(
        "opencode permission reply: HTTP {} {}",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_base_url() {
        assert_eq!(
            normalize_base_url("http://127.0.0.1:4096/"),
            "http://127.0.0.1:4096"
        );
    }
}
