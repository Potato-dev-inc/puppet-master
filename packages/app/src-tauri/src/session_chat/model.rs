//! The chat items a session file turns into, and the trait each CLI's file format implements.

use serde::Serialize;

/// Longest tool output kept in a `ToolResult`, in characters.
pub const MAX_OUTPUT_CHARS: usize = 4000;

/// One row of the chat view. Serialises with `kind` as the tag, e.g. `{"kind":"toolCall", ...}`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ChatItem {
    /// What the person typed (never injected context).
    User { id: String, at: Option<String>, text: String },
    /// Agent text. `final_answer` is true for the last message of a turn when the format says so.
    Assistant {
        id: String,
        at: Option<String>,
        text: String,
        model: Option<String>,
        final_answer: bool,
    },
    /// A tool call. `summary` is one short line: the command, the file path, or the tool's main input.
    ToolCall { id: String, at: Option<String>, call_id: String, name: String, summary: String },
    /// A tool result. `output` is cut to MAX_OUTPUT_CHARS (4000) with a trailing "…".
    ToolResult { id: String, at: Option<String>, call_id: String, ok: bool, output: String },
    TurnEnd { id: String, at: Option<String>, outcome: TurnOutcome, duration_ms: Option<u64> },
    Usage {
        id: String,
        at: Option<String>,
        input_tokens: u64,
        output_tokens: u64,
        context_window: Option<u64>,
    },
    Session {
        id: String,
        at: Option<String>,
        model: Option<String>,
        cli_version: Option<String>,
    },
}

impl ChatItem {
    /// The stable id: `"<byte offset of the line>:<index within the line>"`.
    pub fn id(&self) -> &str {
        match self {
            ChatItem::User { id, .. }
            | ChatItem::Assistant { id, .. }
            | ChatItem::ToolCall { id, .. }
            | ChatItem::ToolResult { id, .. }
            | ChatItem::TurnEnd { id, .. }
            | ChatItem::Usage { id, .. }
            | ChatItem::Session { id, .. } => id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TurnOutcome {
    Completed,
    Interrupted,
    Failed,
}

/// One CLI's session-file format. Implemented under src/tui/<agent>/session_file.rs.
pub trait SessionFormat: Send + Sync {
    /// Items for one complete JSON line. Unknown or noise lines give an empty Vec; never panic, never error.
    fn parse_line(&self, line: &str, offset: u64) -> Vec<ChatItem>;
}

/// Cuts `text` to `MAX_OUTPUT_CHARS` characters. A cut text ends with "…"; a short one is returned as is.
pub fn cut(text: &str) -> String {
    match text.char_indices().nth(MAX_OUTPUT_CHARS) {
        None => text.to_string(),
        Some((byte_end, _)) => format!("{}…", &text[..byte_end]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tool_call_serialises_with_camel_case_kind_and_fields() {
        let item = ChatItem::ToolCall {
            id: "10:0".into(),
            at: None,
            call_id: "call-1".into(),
            name: "shell".into(),
            summary: "git status".into(),
        };
        let value = serde_json::to_value(&item).unwrap();
        assert_eq!(
            value,
            json!({
                "kind": "toolCall",
                "id": "10:0",
                "at": null,
                "callId": "call-1",
                "name": "shell",
                "summary": "git status"
            })
        );
    }

    #[test]
    fn assistant_and_turn_end_use_camel_case_fields() {
        let assistant = ChatItem::Assistant {
            id: "1:0".into(),
            at: Some("2026-10-27T10:00:00Z".into()),
            text: "done".into(),
            model: None,
            final_answer: true,
        };
        let value = serde_json::to_value(&assistant).unwrap();
        assert_eq!(value["kind"], "assistant");
        assert_eq!(value["finalAnswer"], true);
        assert!(value.get("final_answer").is_none());

        let end = ChatItem::TurnEnd {
            id: "2:0".into(),
            at: None,
            outcome: TurnOutcome::Interrupted,
            duration_ms: Some(1500),
        };
        let value = serde_json::to_value(&end).unwrap();
        assert_eq!(value["kind"], "turnEnd");
        assert_eq!(value["outcome"], "interrupted");
        assert_eq!(value["durationMs"], 1500);
    }

    #[test]
    fn usage_and_session_use_camel_case_fields() {
        let usage = ChatItem::Usage {
            id: "3:0".into(),
            at: None,
            input_tokens: 12,
            output_tokens: 34,
            context_window: Some(200_000),
        };
        let value = serde_json::to_value(&usage).unwrap();
        assert_eq!(value["kind"], "usage");
        assert_eq!(value["inputTokens"], 12);
        assert_eq!(value["outputTokens"], 34);
        assert_eq!(value["contextWindow"], 200_000);

        let session = ChatItem::Session {
            id: "0:0".into(),
            at: None,
            model: None,
            cli_version: Some("1.2.3".into()),
        };
        let value = serde_json::to_value(&session).unwrap();
        assert_eq!(value["kind"], "session");
        assert_eq!(value["cliVersion"], "1.2.3");
    }

    #[test]
    fn turn_outcome_names_are_camel_case() {
        assert_eq!(serde_json::to_value(TurnOutcome::Completed).unwrap(), "completed");
        assert_eq!(serde_json::to_value(TurnOutcome::Failed).unwrap(), "failed");
    }

    #[test]
    fn id_returns_the_id_of_every_variant() {
        let user = ChatItem::User { id: "5:1".into(), at: None, text: "hi".into() };
        assert_eq!(user.id(), "5:1");
        let session = ChatItem::Session { id: "0:0".into(), at: None, model: None, cli_version: None };
        assert_eq!(session.id(), "0:0");
    }

    #[test]
    fn cut_keeps_short_text_unchanged() {
        assert_eq!(cut(""), "");
        assert_eq!(cut("hello"), "hello");
        let exact: String = "a".repeat(MAX_OUTPUT_CHARS);
        assert_eq!(cut(&exact), exact);
    }

    #[test]
    fn cut_counts_characters_not_bytes() {
        // Each "é" is two bytes; 4000 of them are 8000 bytes and must not be cut.
        let exact: String = "é".repeat(MAX_OUTPUT_CHARS);
        assert_eq!(cut(&exact), exact);

        let longer: String = "é".repeat(MAX_OUTPUT_CHARS + 5);
        let cut_text = cut(&longer);
        assert_eq!(cut_text.chars().count(), MAX_OUTPUT_CHARS + 1);
        assert!(cut_text.ends_with('…'));
        assert_eq!(cut_text, format!("{}…", "é".repeat(MAX_OUTPUT_CHARS)));
    }

    #[test]
    fn cut_never_splits_a_multibyte_character() {
        // A four-byte emoji at the cut point must go whole, not as a broken byte sequence.
        let text = format!("{}😀tail", "x".repeat(MAX_OUTPUT_CHARS - 1));
        let cut_text = cut(&text);
        assert_eq!(cut_text, format!("{}😀…", "x".repeat(MAX_OUTPUT_CHARS - 1)));
    }
}
