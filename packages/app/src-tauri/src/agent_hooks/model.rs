//! The hook event model (§4.5): what one vendor hook payload becomes after projection.
//!
//! Raw vendor JSON never leaves `ingest`: only these projected, length-capped fields do. Free text
//! (the reply, a command) stays in memory as [`crate::redact::Redacted`] and is never persisted;
//! the prompt is kept only as a hash and a length.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HookKind {
    SessionStart,
    PromptSubmitted,
    ToolPre,
    ToolPost,
    PermissionRequest,
    TurnEnd,
    TurnFailed,
    TurnInterrupted,
    SubagentEnd,
    Notice,
    SessionEnd,
}

impl HookKind {
    /// The `pm` value of a `[[hooks.events]]` row.
    pub fn parse(pm: &str) -> Option<Self> {
        Some(match pm {
            "session_start" => Self::SessionStart,
            "prompt_submitted" => Self::PromptSubmitted,
            "tool_pre" => Self::ToolPre,
            "tool_post" => Self::ToolPost,
            "permission_request" => Self::PermissionRequest,
            "turn_end" => Self::TurnEnd,
            "turn_failed" => Self::TurnFailed,
            "turn_interrupted" => Self::TurnInterrupted,
            "subagent_end" => Self::SubagentEnd,
            "notice" => Self::Notice,
            "session_end" => Self::SessionEnd,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::SessionStart => "session_start",
            Self::PromptSubmitted => "prompt_submitted",
            Self::ToolPre => "tool_pre",
            Self::ToolPost => "tool_post",
            Self::PermissionRequest => "permission_request",
            Self::TurnEnd => "turn_end",
            Self::TurnFailed => "turn_failed",
            Self::TurnInterrupted => "turn_interrupted",
            Self::SubagentEnd => "subagent_end",
            Self::Notice => "notice",
            Self::SessionEnd => "session_end",
        }
    }

    /// Ends the top-level turn (subagent events never do).
    pub fn ends_turn(self) -> bool {
        matches!(self, Self::TurnEnd | Self::TurnFailed | Self::TurnInterrupted)
    }
}

/// Per-kind projected fields (§4.3). Strings are cut and sanitized; free text is redacted.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct HookDetail {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_use: Option<String>,
    /// Paths stay (they are not contents); paths outside the project are reduced to a file name.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
    /// `sha256(normalize(prompt))` and the prompt's length; the text itself is never kept.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_sha: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_len: Option<usize>,
    /// The agent's own final reply (redacted, at most 64 KiB). In memory only, never serialized.
    #[serde(skip)]
    pub final_text: Option<crate::redact::Redacted<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_text_len: Option<usize>,
    /// A command or description the agent asked about (redacted, at most 2 KiB). In memory only.
    #[serde(skip)]
    pub command: Option<crate::redact::Redacted<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command_len: Option<usize>,
    /// `stop_hook_active`: the agent continues because another Stop hook blocked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub continuing: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// The emitter could not parse the vendor's stdin (`{"unparsed": true}`).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub unparsed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HookEvent {
    /// Bridge-assigned, per pane, monotonic.
    pub seq: u64,
    /// Bridge receive time (vendor clocks are ignored).
    pub at_ms: u64,
    pub pane_id: String,
    pub agent: String,
    pub kind: HookKind,
    /// The vendor's event name ("Stop", "UserPromptSubmit", ...).
    pub vendor_event: String,
    pub session: Option<String>,
    /// Claude `prompt_id`, Codex `turn_id`.
    pub turn_key: Option<String>,
    pub subagent: Option<String>,
    /// Another session than the one the pane is pinned to: never used for anything.
    pub foreign: bool,
    pub detail: HookDetail,
}
