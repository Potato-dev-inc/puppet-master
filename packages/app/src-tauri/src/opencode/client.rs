//! Minimal OpenCode REST client (v1 API). See https://opencode.ai/docs/server

use serde::Deserialize;
use serde::Serialize;
use serde_json::json;
use std::time::{Duration, Instant};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

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
        match ureq::get(&url).timeout(DEFAULT_TIMEOUT).call() {
            Ok(response) if response.status() == 200 => return Ok(()),
            _ => std::thread::sleep(Duration::from_millis(250)),
        }
    }
    Err(format!(
        "OpenCode server not healthy at {base} within {timeout:?}"
    ))
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
    let response = ureq::post(&url)
        .timeout(DEFAULT_TIMEOUT)
        .send_json(switch_session_model_body(model))
        .map_err(|err| format!("opencode switch session model: {err}"))?;
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
    let response = ureq::post(&url)
        .timeout(DEFAULT_TIMEOUT)
        .send_json(prompt_async_body(text, model))
        .map_err(|err| PromptAsyncError {
            status: 0,
            body: format!("opencode prompt_async: {err}"),
        })?;
    if response.status() == 204 || (200..300).contains(&response.status()) {
        return Ok(());
    }
    let status = response.status();
    let body = response.into_string().unwrap_or_default();
    Err(PromptAsyncError { status, body })
}

pub fn abort_session(
    base_url: &str,
    session_id: &str,
    directory: Option<&str>,
) -> Result<(), String> {
    let base = normalize_base_url(base_url);
    let url = with_directory_query(&format!("{base}/session/{session_id}/abort"), directory);
    let response = ureq::post(&url)
        .timeout(DEFAULT_TIMEOUT)
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

/// OpenCode TUI footer follows last user message model on attach, not session.model alone.
/// Stamp a zero-width user message with `model`, then abort generation.
pub fn stamp_last_user_model(
    base_url: &str,
    session_id: &str,
    directory: Option<&str>,
    model: &OpenCodeModelRef,
) -> Result<(), String> {
    // ponytail: ZWSP keeps history visually empty; upgrade = real TUI local.model API if OpenCode adds one
    prompt_async(base_url, session_id, directory, "\u{200b}", Some(model))
        .map_err(|err| err.into_message())?;
    let _ = abort_session(base_url, session_id, directory);
    Ok(())
}

#[derive(Debug, Clone, Deserialize)]
pub struct OpenCodeSessionModel {
    #[serde(rename = "providerID", default)]
    pub provider_id: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub variant: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OpenCodeSessionInfo {
    pub id: String,
    #[serde(default)]
    pub model: Option<OpenCodeSessionModel>,
}

pub fn get_session(
    base_url: &str,
    session_id: &str,
    directory: Option<&str>,
) -> Result<OpenCodeSessionInfo, String> {
    let base = normalize_base_url(base_url);
    let url = with_directory_query(&format!("{base}/session/{session_id}"), directory);
    let response = ureq::get(&url)
        .timeout(DEFAULT_TIMEOUT)
        .call()
        .map_err(|err| format!("opencode get session: {err}"))?;
    if !(200..300).contains(&response.status()) {
        return Err(format!(
            "opencode get session: HTTP {} {}",
            response.status(),
            response.into_string().unwrap_or_default()
        ));
    }
    response
        .into_json::<OpenCodeSessionInfo>()
        .map_err(|err| format!("opencode get session parse: {err}"))
}

#[derive(Debug, Clone, Deserialize)]
struct OpenCodeMessageUserModel {
    #[serde(rename = "modelID", default)]
    model_id: Option<String>,
    #[serde(rename = "providerID", default)]
    provider_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OpenCodeMessagePart {
    #[serde(rename = "type")]
    pub part_type: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub tool: Option<String>,
    #[serde(default)]
    pub state: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OpenCodeMessageInfo {
    #[serde(default)]
    pub id: Option<String>,
    pub role: String,
    #[serde(default)]
    pub model: Option<OpenCodeMessageUserModel>,
    #[serde(rename = "modelID", default)]
    pub model_id: Option<String>,
    #[serde(rename = "providerID", default)]
    pub provider_id: Option<String>,
    #[serde(default)]
    pub finish: Option<String>,
    #[serde(default)]
    pub error: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OpenCodeMessageEnvelope {
    pub info: OpenCodeMessageInfo,
    #[serde(default)]
    pub parts: Vec<OpenCodeMessagePart>,
}

pub fn list_session_messages(
    base_url: &str,
    session_id: &str,
    directory: Option<&str>,
) -> Result<Vec<OpenCodeMessageEnvelope>, String> {
    let base = normalize_base_url(base_url);
    let url = with_directory_query(&format!("{base}/session/{session_id}/message"), directory);
    let response = ureq::get(&url)
        .timeout(DEFAULT_TIMEOUT)
        .call()
        .map_err(|err| format!("opencode list messages: {err}"))?;
    if !(200..300).contains(&response.status()) {
        return Err(format!(
            "opencode list messages: HTTP {} {}",
            response.status(),
            response.into_string().unwrap_or_default()
        ));
    }
    response
        .into_json::<Vec<OpenCodeMessageEnvelope>>()
        .map_err(|err| format!("opencode list messages parse: {err}"))
}

pub fn message_model(info: &OpenCodeMessageInfo) -> Option<OpenCodeModelRef> {
    if let Some(model) = info.model.as_ref() {
        let model_id = model.model_id.as_deref()?;
        let provider_id = model.provider_id.as_deref()?;
        if !model_id.is_empty() && !provider_id.is_empty() {
            return Some(OpenCodeModelRef {
                provider_id: provider_id.to_string(),
                model_id: model_id.to_string(),
            });
        }
    }
    let model_id = info.model_id.as_deref()?;
    let provider_id = info.provider_id.as_deref()?;
    if model_id.is_empty() || provider_id.is_empty() {
        return None;
    }
    Some(OpenCodeModelRef {
        provider_id: provider_id.to_string(),
        model_id: model_id.to_string(),
    })
}

pub fn session_model_ref(
    base_url: &str,
    session_id: &str,
    directory: Option<&str>,
) -> Result<Option<OpenCodeModelRef>, String> {
    let session = get_session(base_url, session_id, directory)?;
    let Some(model) = session.model.as_ref() else {
        return Ok(None);
    };
    let Some(model_id) = model.id.as_deref().filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let Some(provider_id) = model
        .provider_id
        .as_deref()
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    Ok(Some(OpenCodeModelRef {
        provider_id: provider_id.to_string(),
        model_id: model_id.to_string(),
    }))
}

pub fn last_user_message_model(
    base_url: &str,
    session_id: &str,
    directory: Option<&str>,
) -> Result<Option<OpenCodeModelRef>, String> {
    let messages = list_session_messages(base_url, session_id, directory)?;
    Ok(messages
        .iter()
        .rev()
        .find(|message| message.info.role == "user")
        .and_then(|message| message_model(&message.info)))
}

pub fn models_match(a: &OpenCodeModelRef, b: &OpenCodeModelRef) -> bool {
    a.provider_id.eq_ignore_ascii_case(&b.provider_id)
        && a.model_id.eq_ignore_ascii_case(&b.model_id)
}

pub fn model_matches_filter(
    model: &OpenCodeModelRef,
    provider_id: Option<&str>,
    model_id: Option<&str>,
) -> bool {
    if let Some(expected) = provider_id.map(str::trim).filter(|value| !value.is_empty()) {
        if !model.provider_id.eq_ignore_ascii_case(expected) {
            return false;
        }
    }
    if let Some(expected) = model_id.map(str::trim).filter(|value| !value.is_empty()) {
        if !model.model_id.eq_ignore_ascii_case(expected) {
            return false;
        }
    }
    true
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OpenCodePermission {
    pub id: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(rename = "sessionID", default)]
    pub session_id_camel: Option<String>,
    #[serde(default)]
    pub permission: Option<String>,
    #[serde(default)]
    pub patterns: Option<Vec<String>>,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
}

pub fn list_permissions(base_url: &str) -> Result<Vec<OpenCodePermission>, String> {
    let base = normalize_base_url(base_url);
    let url = format!("{base}/permission");
    let response = ureq::get(&url)
        .timeout(DEFAULT_TIMEOUT)
        .call()
        .map_err(|err| format!("opencode list permissions: {err}"))?;
    if !(200..300).contains(&response.status()) {
        return Err(format!(
            "opencode list permissions: HTTP {}",
            response.status()
        ));
    }
    response
        .into_json::<Vec<OpenCodePermission>>()
        .map_err(|err| format!("opencode list permissions parse: {err}"))
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OpenCodeQuestionOption {
    pub label: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OpenCodeQuestionInfo {
    pub question: String,
    #[serde(default)]
    pub header: Option<String>,
    #[serde(default)]
    pub options: Vec<OpenCodeQuestionOption>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OpenCodeQuestionRequest {
    pub id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(default)]
    pub questions: Vec<OpenCodeQuestionInfo>,
}

pub fn list_questions(
    base_url: &str,
    directory: Option<&str>,
) -> Result<Vec<OpenCodeQuestionRequest>, String> {
    let base = normalize_base_url(base_url);
    let url = with_directory_query(&format!("{base}/question"), directory);
    let response = ureq::get(&url)
        .timeout(DEFAULT_TIMEOUT)
        .call()
        .map_err(|err| format!("opencode list questions: {err}"))?;
    if !(200..300).contains(&response.status()) {
        return Err(format!(
            "opencode list questions: HTTP {} {}",
            response.status(),
            response.into_string().unwrap_or_default()
        ));
    }
    response
        .into_json::<Vec<OpenCodeQuestionRequest>>()
        .map_err(|err| format!("opencode list questions parse: {err}"))
}

pub fn reply_question(
    base_url: &str,
    request_id: &str,
    directory: Option<&str>,
    labels: &[String],
) -> Result<(), String> {
    let base = normalize_base_url(base_url);
    let url = with_directory_query(&format!("{base}/question/{request_id}/reply"), directory);
    let response = ureq::post(&url)
        .timeout(DEFAULT_TIMEOUT)
        .send_json(json!({ "answers": [labels] }))
        .map_err(|err| format!("opencode question reply: {err}"))?;
    if (200..300).contains(&response.status()) {
        return Ok(());
    }
    Err(format!(
        "opencode question reply: HTTP {} {}",
        response.status(),
        response.into_string().unwrap_or_default()
    ))
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

    #[test]
    fn detects_rate_limit_status_codes() {
        assert!(is_rate_limit_status(429, ""));
        assert!(is_rate_limit_status(402, ""));
        assert!(!is_rate_limit_status(500, ""));
    }

    #[test]
    fn detects_rate_limit_body_keywords() {
        assert!(is_rate_limit_status(400, "Rate Limit exceeded"));
        assert!(is_rate_limit_status(400, "quota exhausted"));
        assert!(is_rate_limit_status(400, "usage cap"));
        assert!(is_rate_limit_status(400, "Too Many Requests"));
        assert!(!is_rate_limit_status(400, "invalid session"));
    }

    #[test]
    fn with_directory_query_encodes_windows_paths() {
        let url = with_directory_query(
            "http://127.0.0.1:4097/api/session/ses_test/model",
            Some(r"C:\repo\packages\app"),
        );
        assert!(url.contains("directory=C%3A%5Crepo%5Cpackages%5Capp"));
    }

    #[test]
    fn switch_session_model_body_uses_v2_shape() {
        let body = switch_session_model_body(&OpenCodeModelRef {
            provider_id: "openrouter".into(),
            model_id: "z-ai/glm-5.2".into(),
        });
        assert_eq!(
            body["model"],
            json!({ "id": "z-ai/glm-5.2", "providerID": "openrouter", "variant": "default" })
        );
    }

    #[test]
    fn prompt_async_body_includes_model_when_set() {
        let body = prompt_async_body(
            "hello",
            Some(&OpenCodeModelRef {
                provider_id: "anthropic".into(),
                model_id: "claude-sonnet-4".into(),
            }),
        );
        assert_eq!(
            body["model"],
            json!({ "providerID": "anthropic", "modelID": "claude-sonnet-4" })
        );
        assert_eq!(body["parts"][0]["text"], "hello");
    }

    #[test]
    fn prompt_async_body_omits_model_when_none() {
        let body = prompt_async_body("hello", None);
        assert!(body.get("model").is_none());
    }

    #[test]
    fn model_matches_filter_respects_provider_and_model() {
        let model = OpenCodeModelRef {
            provider_id: "opencode-go".into(),
            model_id: "glm-5.2".into(),
        };
        assert!(model_matches_filter(
            &model,
            Some("opencode-go"),
            Some("glm-5.2")
        ));
        assert!(model_matches_filter(&model, None, Some("glm-5.2")));
        assert!(!model_matches_filter(
            &model,
            Some("openrouter"),
            Some("glm-5.2")
        ));
        assert!(!model_matches_filter(
            &model,
            Some("opencode-go"),
            Some("deepseek")
        ));
    }
}
