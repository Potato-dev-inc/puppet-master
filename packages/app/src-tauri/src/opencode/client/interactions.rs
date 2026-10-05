//! Pending permission and question requests and their replies.

use super::*;

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
