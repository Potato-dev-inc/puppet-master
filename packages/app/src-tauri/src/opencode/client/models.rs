//! Session and message reads: stored session model, message envelopes, model matching.

use super::*;

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
pub struct OpenCodeMessageUserModel {
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
