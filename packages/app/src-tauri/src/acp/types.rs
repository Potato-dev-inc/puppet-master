//! The few ACP message types this lane reads, written by hand and tolerant.
//!
//! Why not the official `agent-client-protocol-schema` crate: it forces `serde_json`'s `preserve_order`
//! feature on, and Cargo unifies features across the whole app. Turning that on makes every JSON object
//! in Puppet Master keep insertion order instead of sorted order. That changed 7 existing tests the day it
//! was tried (golden files, the pinned tool-catalog version) and would silently change any signed or
//! hashed JSON. A message type is not worth that. These types cover what the state machine needs; the
//! rest of a message stays a raw `serde_json::Value` for A2's chat mapper to read.
//!
//! Every field the protocol marks optional is optional here, unknown fields are ignored, and an unknown
//! update kind fails to parse on purpose: the session keeps it raw (`SessionEvent::UpdateUnparsed`)
//! instead of guessing. The shapes follow ACP v1 (`agentclientprotocol.com`) and were checked against real
//! adapter traffic recorded in `fixtures/`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// How a turn ended, in the protocol's words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    EndTurn,
    MaxTokens,
    MaxTurnRequests,
    Refusal,
    Cancelled,
}

impl StopReason {
    pub fn parse(word: &str) -> Option<Self> {
        match word {
            "end_turn" => Some(StopReason::EndTurn),
            "max_tokens" => Some(StopReason::MaxTokens),
            "max_turn_requests" => Some(StopReason::MaxTurnRequests),
            "refusal" => Some(StopReason::Refusal),
            "cancelled" => Some(StopReason::Cancelled),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            StopReason::EndTurn => "end_turn",
            StopReason::MaxTokens => "max_tokens",
            StopReason::MaxTurnRequests => "max_turn_requests",
            StopReason::Refusal => "refusal",
            StopReason::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolStatus {
    #[default]
    Pending,
    InProgress,
    Completed,
    Failed,
}

impl ToolStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            ToolStatus::Pending => "pending",
            ToolStatus::InProgress => "in_progress",
            ToolStatus::Completed => "completed",
            ToolStatus::Failed => "failed",
        }
    }

    pub fn is_finished(self) -> bool {
        matches!(self, ToolStatus::Completed | ToolStatus::Failed)
    }
}

/// One piece of streamed text, a thought, or a replayed message. `content` is the raw content block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ContentChunk {
    pub content: Value,
    pub message_id: Option<String>,
}

impl Default for ContentChunk {
    fn default() -> Self {
        Self { content: Value::Null, message_id: None }
    }
}

impl ContentChunk {
    /// The text of a text block, if it is one.
    pub fn text(&self) -> Option<&str> {
        if self.content.get("type").and_then(Value::as_str) == Some("text") {
            self.content.get("text").and_then(Value::as_str)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ToolCall {
    pub tool_call_id: String,
    pub title: String,
    pub kind: String,
    pub status: ToolStatus,
    pub content: Vec<Value>,
    pub locations: Vec<Value>,
    pub raw_input: Option<Value>,
    pub raw_output: Option<Value>,
}

impl Default for ToolCall {
    fn default() -> Self {
        Self {
            tool_call_id: String::new(),
            title: String::new(),
            kind: "other".into(),
            status: ToolStatus::Pending,
            content: Vec::new(),
            locations: Vec::new(),
            raw_input: None,
            raw_output: None,
        }
    }
}

/// A change to a tool call; only `tool_call_id` is always there. Also the `toolCall` of a permission request.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ToolCallUpdate {
    pub tool_call_id: String,
    pub title: Option<String>,
    pub kind: Option<String>,
    pub status: Option<ToolStatus>,
    pub content: Option<Vec<Value>>,
    pub locations: Option<Vec<Value>>,
    pub raw_input: Option<Value>,
    pub raw_output: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CommandInput {
    pub hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AvailableCommand {
    pub name: String,
    pub description: String,
    pub input: Option<CommandInput>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AvailableCommandsUpdate {
    pub available_commands: Vec<AvailableCommand>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ConfigOptionUpdate {
    pub config_options: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CurrentModeUpdate {
    pub current_mode_id: String,
}

/// `title` may be a string, `null` (cleared) or absent (unchanged); the state machine only acts on a string.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SessionInfoUpdate {
    pub title: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UsageUpdate {
    pub used: u64,
    pub size: u64,
    pub cost: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Plan {
    pub entries: Vec<Value>,
}

/// The payload of `session/update`, tagged by `sessionUpdate`. A kind not listed here does not parse.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "sessionUpdate", rename_all = "snake_case")]
pub enum SessionUpdate {
    UserMessageChunk(ContentChunk),
    AgentMessageChunk(ContentChunk),
    AgentThoughtChunk(ContentChunk),
    ToolCall(ToolCall),
    ToolCallUpdate(ToolCallUpdate),
    Plan(Plan),
    AvailableCommandsUpdate(AvailableCommandsUpdate),
    CurrentModeUpdate(CurrentModeUpdate),
    ConfigOptionUpdate(ConfigOptionUpdate),
    SessionInfoUpdate(SessionInfoUpdate),
    UsageUpdate(UsageUpdate),
    /// A notice for the person (kept raw).
    Notice(Value),
    /// Context compaction progress (kept raw).
    CompactionUpdate(Value),
    CompactionSummaryChunk(Value),
}

impl SessionUpdate {
    /// The wire name of the kind (`agent_message_chunk`).
    pub fn kind(&self) -> &'static str {
        match self {
            SessionUpdate::UserMessageChunk(_) => "user_message_chunk",
            SessionUpdate::AgentMessageChunk(_) => "agent_message_chunk",
            SessionUpdate::AgentThoughtChunk(_) => "agent_thought_chunk",
            SessionUpdate::ToolCall(_) => "tool_call",
            SessionUpdate::ToolCallUpdate(_) => "tool_call_update",
            SessionUpdate::Plan(_) => "plan",
            SessionUpdate::AvailableCommandsUpdate(_) => "available_commands_update",
            SessionUpdate::CurrentModeUpdate(_) => "current_mode_update",
            SessionUpdate::ConfigOptionUpdate(_) => "config_option_update",
            SessionUpdate::SessionInfoUpdate(_) => "session_info_update",
            SessionUpdate::UsageUpdate(_) => "usage_update",
            SessionUpdate::Notice(_) => "notice",
            SessionUpdate::CompactionUpdate(_) => "compaction_update",
            SessionUpdate::CompactionSummaryChunk(_) => "compaction_summary_chunk",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionNotification {
    pub session_id: String,
    pub update: SessionUpdate,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionOption {
    pub option_id: String,
    pub name: String,
    /// `allow_once`, `allow_always`, `reject_once`, `reject_always` (kept as the agent's word).
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestPermissionRequest {
    pub session_id: String,
    pub tool_call: ToolCallUpdate,
    pub options: Vec<PermissionOption>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_recorded_permission_requests_of_both_adapters_parse() {
        for text in [include_str!("fixtures/permission_request_codex.json"), include_str!("fixtures/permission_request_claude.json")] {
            let request: RequestPermissionRequest = serde_json::from_str(text).expect("a real request");
            assert!(!request.session_id.is_empty());
            assert!(!request.tool_call.tool_call_id.is_empty());
            assert!(request.options.len() >= 2);
            assert!(request.options.iter().any(|o| o.kind == "allow_once"));
        }
        // The Codex one carries no title at all: still fine.
        let thin: RequestPermissionRequest = serde_json::from_str(include_str!("fixtures/permission_request_codex.json")).unwrap();
        assert_eq!(thin.tool_call.title, None);
        assert_eq!(thin.tool_call.kind.as_deref(), Some("edit"));
        assert_eq!(thin.options[1].option_id, "allow_always");
    }

    #[test]
    fn recorded_update_shapes_parse() {
        let cases = [
            json!({"sessionUpdate":"agent_message_chunk","content":{"text":"ok","type":"text"}}),
            json!({"sessionUpdate":"agent_thought_chunk","content":{"text":"\n\n","type":"text"},"messageId":"rs_1"}),
            json!({"sessionUpdate":"tool_call","_meta":{"x":1},"content":[{"newText":"ok\n","oldText":null,"path":"D:\\w\\a.txt","type":"diff"}],"kind":"edit","status":"in_progress","title":"Editing files","toolCallId":"exec-1"}),
            json!({"sessionUpdate":"tool_call_update","_meta":{"x":1},"status":"failed","toolCallId":"exec-1"}),
            json!({"sessionUpdate":"available_commands_update","availableCommands":[{"name":"plan","description":"Turn plan mode on.","input":null,"_meta":{}}]}),
            json!({"sessionUpdate":"session_info_update","title":null}),
            json!({"sessionUpdate":"session_info_update","updatedAt":"2026-11-01T10:00:00Z"}),
            json!({"sessionUpdate":"usage_update","used":10,"size":200000,"cost":{"amount":0.1,"currency":"USD"}}),
            json!({"sessionUpdate":"plan","entries":[{"content":"x","priority":"high","status":"pending"}]}),
            json!({"sessionUpdate":"config_option_update","configOptions":[{"id":"model"}]}),
            json!({"sessionUpdate":"notice","level":"warning","text":"careful"}),
        ];
        for case in cases {
            let parsed: Result<SessionUpdate, _> = serde_json::from_value(case.clone());
            assert!(parsed.is_ok(), "{case}: {parsed:?}");
        }
    }

    #[test]
    fn an_unknown_kind_does_not_parse_so_the_session_keeps_it_raw() {
        assert!(serde_json::from_value::<SessionUpdate>(json!({"sessionUpdate":"future_thing","x":1})).is_err());
        assert!(serde_json::from_value::<SessionUpdate>(json!({"nothing":true})).is_err());
    }

    #[test]
    fn an_update_serialises_back_with_its_tag() {
        let update: SessionUpdate = serde_json::from_value(json!({"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"hi"}})).unwrap();
        let back = serde_json::to_value(&update).unwrap();
        assert_eq!(back["sessionUpdate"], json!("agent_message_chunk"));
        assert_eq!(back["content"]["text"], json!("hi"));
        assert_eq!(update.kind(), "agent_message_chunk");
        if let SessionUpdate::AgentMessageChunk(chunk) = update {
            assert_eq!(chunk.text(), Some("hi"));
        }
    }

    #[test]
    fn stop_reasons_round_trip_and_unknown_words_are_none() {
        for reason in [StopReason::EndTurn, StopReason::MaxTokens, StopReason::MaxTurnRequests, StopReason::Refusal, StopReason::Cancelled] {
            assert_eq!(StopReason::parse(reason.as_str()), Some(reason));
        }
        assert_eq!(StopReason::parse("exploded"), None);
    }

    #[test]
    fn a_tool_call_missing_optional_fields_gets_defaults() {
        let call: ToolCall = serde_json::from_value(json!({"toolCallId":"t","title":"x"})).unwrap();
        assert_eq!(call.kind, "other");
        assert_eq!(call.status, ToolStatus::Pending);
    }
}
