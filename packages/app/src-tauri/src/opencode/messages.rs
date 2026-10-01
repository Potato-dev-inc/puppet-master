//! Structured OpenCode session messages (model output without TUI scrollback).

use parking_lot::Mutex;
use serde::Serialize;
use serde_json::Value;

use super::client::{self, OpenCodeMessageEnvelope, OpenCodeModelRef};

#[derive(Debug, Clone, Serialize)]
pub struct QuestionOption {
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PendingQuestion {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    pub question: String,
    pub options: Vec<QuestionOption>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MessagePartView {
    Text {
        text: String,
    },
    Question {
        question: String,
        options: Vec<QuestionOption>,
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<String>,
    },
    Tool {
        tool: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct MessageView {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<OpenCodeModelRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub parts: Vec<MessagePartView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionMessagesView {
    pub pane_id: String,
    pub session_id: String,
    pub messages: Vec<MessageView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_question: Option<PendingQuestion>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_assistant_text: Option<String>,
}

pub fn read_pane_messages(
    registry: &Mutex<crate::pty::PaneRegistry>,
    pane_id: &str,
    limit: usize,
    role_filter: Option<&str>,
) -> Result<SessionMessagesView, String> {
    let (base_url, session_id, directory) = {
        let reg = registry.lock();
        let pane = reg
            .panes
            .get(pane_id)
            .ok_or_else(|| format!("unknown pane: {pane_id}"))?;
        let link = pane
            .opencode
            .as_ref()
            .ok_or_else(|| format!("pane {pane_id} is not an opencode native worker"))?;
        (
            link.base_url.clone(),
            link.session_id.clone(),
            link.directory.clone(),
        )
    };
    let raw = client::list_session_messages(&base_url, &session_id, Some(&directory))?;
    let api_pending = session_pending_question_from_api(&base_url, &session_id, Some(&directory));
    Ok(build_view(
        pane_id,
        &session_id,
        raw,
        limit.max(1),
        role_filter,
        api_pending,
    ))
}

pub fn pending_question_for_pane(
    registry: &Mutex<crate::pty::PaneRegistry>,
    pane_id: &str,
) -> Option<PendingQuestion> {
    let view = read_pane_messages(registry, pane_id, 8, Some("assistant")).ok()?;
    view.pending_question
}

pub fn build_view(
    pane_id: &str,
    session_id: &str,
    raw: Vec<OpenCodeMessageEnvelope>,
    limit: usize,
    role_filter: Option<&str>,
    api_pending: Option<PendingQuestion>,
) -> SessionMessagesView {
    let filtered: Vec<_> = raw
        .into_iter()
        .filter(|message| role_matches(&message.info.role, role_filter))
        .collect();
    let start = filtered.len().saturating_sub(limit);
    let messages: Vec<MessageView> = filtered[start..].iter().map(message_view).collect();
    let pending_question = api_pending.or_else(|| find_pending_question(&filtered));
    let last_assistant_text = last_assistant_text(&filtered);
    SessionMessagesView {
        pane_id: pane_id.to_string(),
        session_id: session_id.to_string(),
        messages,
        pending_question,
        last_assistant_text,
    }
}

fn role_matches(role: &str, filter: Option<&str>) -> bool {
    match filter.map(str::trim).filter(|value| !value.is_empty()) {
        None | Some("all") => true,
        Some(want) => role.eq_ignore_ascii_case(want),
    }
}

fn message_view(message: &OpenCodeMessageEnvelope) -> MessageView {
    let parts = message
        .parts
        .iter()
        .filter_map(part_view)
        .collect::<Vec<_>>();
    MessageView {
        id: message.info.id.clone(),
        role: message.info.role.clone(),
        model: client::message_model(&message.info),
        finish: message.info.finish.clone(),
        error: message_error(&message.info.error),
        parts,
    }
}

fn message_error(error: &Option<Value>) -> Option<String> {
    let error = error.as_ref()?;
    error
        .get("data")
        .and_then(|data| data.get("message"))
        .and_then(Value::as_str)
        .or_else(|| error.get("name").and_then(Value::as_str))
        .map(str::to_string)
}

fn part_view(part: &client::OpenCodeMessagePart) -> Option<MessagePartView> {
    match part.part_type.as_str() {
        "text" => {
            let text = part.text.as_deref()?.trim();
            if text.is_empty() {
                return None;
            }
            Some(MessagePartView::Text {
                text: text.to_string(),
            })
        }
        "tool" if part.tool.as_deref() == Some("question") => {
            let state = part.state.as_ref()?;
            let (question, options) = parse_question_input(state)?;
            Some(MessagePartView::Question {
                question,
                options,
                status: tool_status(state),
            })
        }
        "tool" => Some(MessagePartView::Tool {
            tool: part.tool.clone().unwrap_or_else(|| "unknown".into()),
            status: part.state.as_ref().and_then(tool_status),
        }),
        "step-start" | "step-finish" | "reasoning" => None,
        _ => None,
    }
}

fn tool_status(state: &Value) -> Option<String> {
    state
        .get("status")
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn parse_question_input(state: &Value) -> Option<(String, Vec<QuestionOption>)> {
    let questions = state
        .get("input")
        .and_then(|input| input.get("questions"))
        .and_then(Value::as_array)?;
    let first = questions.first()?;
    let question = first
        .get("question")
        .and_then(Value::as_str)
        .unwrap_or("Question")
        .to_string();
    let options = first
        .get("options")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let label = item.get("label").and_then(Value::as_str)?.to_string();
                    let description = item
                        .get("description")
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    Some(QuestionOption { label, description })
                })
                .collect()
        })
        .unwrap_or_default();
    Some((question, options))
}

pub fn session_pending_question_from_api(
    base_url: &str,
    session_id: &str,
    directory: Option<&str>,
) -> Option<PendingQuestion> {
    let pending = client::list_questions(base_url, directory).ok()?;
    let request = pending
        .into_iter()
        .find(|item| item.session_id == session_id)?;
    let first = request.questions.first()?;
    Some(PendingQuestion {
        request_id: Some(request.id),
        message_id: None,
        question: first.question.clone(),
        options: first
            .options
            .iter()
            .map(|option| QuestionOption {
                label: option.label.clone(),
                description: option.description.clone(),
            })
            .collect(),
        status: Some("pending".into()),
    })
}

fn find_pending_question(messages: &[OpenCodeMessageEnvelope]) -> Option<PendingQuestion> {
    for message in messages.iter().rev() {
        if !message.info.role.eq_ignore_ascii_case("assistant") {
            continue;
        }
        for part in message.parts.iter().rev() {
            if part.part_type != "tool" || part.tool.as_deref() != Some("question") {
                continue;
            }
            let state = part.state.as_ref()?;
            if !is_pending_question_state(state) {
                continue;
            }
            let (question, options) = parse_question_input(state)?;
            return Some(PendingQuestion {
                request_id: None,
                message_id: message.info.id.clone(),
                question,
                options,
                status: tool_status(state),
            });
        }
    }
    None
}

fn is_pending_question_state(state: &Value) -> bool {
    let status = state
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    match status.as_str() {
        "completed" | "done" | "success" | "error" | "failed" | "cancelled" | "dismissed" => false,
        "running" | "pending" | "waiting" | "open" => true,
        _ => {
            state.get("time").and_then(|time| time.get("end")).is_none()
                && state.get("error").is_none()
        }
    }
}

fn last_assistant_text(messages: &[OpenCodeMessageEnvelope]) -> Option<String> {
    for message in messages.iter().rev() {
        if !message.info.role.eq_ignore_ascii_case("assistant") {
            continue;
        }
        for part in message.parts.iter().rev() {
            if part.part_type == "text" {
                let text = part.text.as_deref()?.trim();
                if !text.is_empty() {
                    return Some(text.to_string());
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn envelope_from_json(value: serde_json::Value) -> OpenCodeMessageEnvelope {
        serde_json::from_value(value).expect("message envelope")
    }

    #[test]
    fn extracts_assistant_text_and_pending_question() {
        let raw = vec![
            envelope_from_json(json!({
                "info": { "id": "msg_user", "role": "user" },
                "parts": [{ "type": "text", "text": "hello" }]
            })),
            envelope_from_json(json!({
                "info": {
                    "id": "msg_asst",
                    "role": "assistant",
                    "modelID": "gpt-5.6-luna",
                    "providerID": "opencode-go",
                    "finish": "tool-calls"
                },
                "parts": [
                    { "type": "text", "text": "Here is my answer." },
                    {
                        "type": "tool",
                        "tool": "question",
                        "state": {
                            "status": "running",
                            "input": {
                                "questions": [{
                                    "question": "Should I continue?",
                                    "options": [
                                        { "label": "Yes", "description": "Continue" },
                                        { "label": "No", "description": "Stop" }
                                    ]
                                }]
                            }
                        }
                    }
                ]
            })),
        ];
        let view = build_view("pane-1", "ses_test", raw, 10, Some("assistant"), None);
        assert_eq!(view.messages.len(), 1);
        assert_eq!(
            view.last_assistant_text.as_deref(),
            Some("Here is my answer.")
        );
        let pending = view.pending_question.expect("pending question");
        assert_eq!(pending.question, "Should I continue?");
        assert_eq!(pending.options.len(), 2);
    }

    #[test]
    fn dismissed_question_is_not_pending() {
        let raw = vec![envelope_from_json(json!({
            "info": { "id": "msg_asst", "role": "assistant" },
            "parts": [{
                "type": "tool",
                "tool": "question",
                "state": {
                    "status": "error",
                    "error": "The user dismissed this question",
                    "input": {
                        "questions": [{
                            "question": "Proceed?",
                            "options": [{ "label": "Yes" }]
                        }]
                    }
                }
            }]
        }))];
        let view = build_view("pane-1", "ses_test", raw, 10, None, None);
        assert!(view.pending_question.is_none());
    }
}
