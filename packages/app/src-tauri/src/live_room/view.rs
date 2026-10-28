//! What a viewer may see: chat items and pane lines, as the relay frames carry them (SPEC-n32, 3.3 and 4 #3).
//! Every text goes through `clean`: strict redaction first (secrets, e-mails, long opaque runs, the user's folder),
//! then paths cut to their last names, then a length cap. Tool inputs and outputs are never copied, nor the
//! content of any card. Viewers render the text as plain text, so nothing here adds markup.

use serde::Serialize;

use crate::pty::status::PaneStatus;
use crate::redact::{mask_emails, RedactContext};
use crate::session_chat::detail::{cut_to, PlanStatus};
use crate::session_chat::export_text::cut_paths;
use crate::session_chat::model::{ChatItem, TurnOutcome};
use crate::tui::{Activity, ActivityKind};

/// The fixed line a pane shows while it waits for the owner. Never the question, the command or the choices.
pub const WAITING_LINE: &str = "Waiting for the owner";
/// The fixed text of a shared pull request (the URL is dropped).
pub const PR_NOTE: &str = "Opened a pull request";

const USER_CHARS: usize = 500;
const ASSISTANT_CHARS: usize = 1500;
const TOOL_NAME_CHARS: usize = 60;
const TOOL_SUMMARY_CHARS: usize = 160;
const NOTICE_CHARS: usize = 200;
const TASK_CHARS: usize = 160;
const MAX_TASKS: usize = 20;
const PANE_CHARS: usize = 60;
const LINE_CHARS: usize = 72;

/// One chat row as a viewer gets it. `t` is the tag; ids are a per-room counter, never the agent's own ids.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "t", rename_all = "camelCase")]
pub enum LiveItem {
    User { id: u64, pane: String, text: String },
    Agent { id: u64, pane: String, text: String },
    Tool { id: u64, pane: String, name: String, summary: String, files: u32, writes: bool },
    Result { id: u64, pane: String, ok: bool, lines: u32 },
    End { id: u64, pane: String, outcome: String, ms: Option<u64> },
    Plan { id: u64, pane: String, tasks: Vec<LiveTask>, replace: bool },
    Note { id: u64, pane: String, text: String },
}

/// One task of a plan. `status` is `pending`, `in_progress`, `done` or `deleted`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LiveTask {
    pub text: String,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveState {
    Working,
    Idle,
    Waiting,
    Error,
    Gone,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LivePane {
    pub key: String,
    pub label: String,
    pub title: String,
    pub state: LiveState,
    pub line: Option<String>,
}

/// Redacted, path-cut and capped text. E-mail addresses are masked first: the strict redactor keeps them
/// (`redact::mask_emails` is separate), and a viewer must never see one. Cutting comes last so a secret is found
/// whole before it could be cut.
pub fn clean(text: &str, max_chars: usize, ctx: &RedactContext) -> String {
    cut_to(&cut_paths(&mask_emails(text), ctx), max_chars)
}

/// `clean`, or `None` when nothing is left to show.
fn clean_text(text: &str, max_chars: usize, ctx: &RedactContext) -> Option<String> {
    let cleaned = clean(text, max_chars, ctx);
    if cleaned.trim().is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

/// The viewer row for one chat item of pane `pane_key`, or `None` when the item is not shared.
/// Takes its id from `next_id` and advances it only when a row is returned.
pub fn to_live(item: &ChatItem, pane_key: &str, next_id: &mut u64, ctx: &RedactContext) -> Option<LiveItem> {
    let id = *next_id;
    let pane = pane_key.to_string();
    let out = match item {
        ChatItem::User { text, .. } => LiveItem::User { id, pane, text: clean_text(text, USER_CHARS, ctx)? },
        ChatItem::Assistant { text, .. } => {
            LiveItem::Agent { id, pane, text: clean_text(text, ASSISTANT_CHARS, ctx)? }
        }
        ChatItem::ToolCall { name, summary, detail, .. } => LiveItem::Tool {
            id,
            pane,
            name: clean(name, TOOL_NAME_CHARS, ctx),
            summary: clean(summary, TOOL_SUMMARY_CHARS, ctx),
            files: detail.paths.len() as u32,
            writes: detail.writes,
        },
        ChatItem::ToolResult { ok, output, .. } => {
            LiveItem::Result { id, pane, ok: *ok, lines: output.lines().count() as u32 }
        }
        ChatItem::TurnEnd { outcome, duration_ms, .. } => {
            LiveItem::End { id, pane, outcome: outcome_word(outcome).to_string(), ms: *duration_ms }
        }
        ChatItem::Plan { items, replace, .. } => LiveItem::Plan {
            id,
            pane,
            tasks: items
                .iter()
                .take(MAX_TASKS)
                .map(|task| LiveTask {
                    text: clean(&task.text, TASK_CHARS, ctx),
                    status: task.status.map(status_word).map(str::to_string),
                })
                .collect(),
            replace: *replace,
        },
        ChatItem::Notice { notice_kind, text, .. } => match notice_kind.as_str() {
            "compacted" | "error" | "title" => LiveItem::Note { id, pane, text: clean_text(text, NOTICE_CHARS, ctx)? },
            "pr" => LiveItem::Note { id, pane, text: PR_NOTE.to_string() },
            _ => return None,
        },
        ChatItem::Usage { .. } | ChatItem::Session { .. } => return None,
    };
    *next_id += 1;
    Some(out)
}

fn outcome_word(outcome: &TurnOutcome) -> &'static str {
    match outcome {
        TurnOutcome::Completed => "completed",
        TurnOutcome::Interrupted => "interrupted",
        TurnOutcome::Failed => "failed",
    }
}

fn status_word(status: PlanStatus) -> &'static str {
    match status {
        PlanStatus::Pending => "pending",
        PlanStatus::InProgress => "in_progress",
        PlanStatus::Completed => "done",
        PlanStatus::Deleted => "deleted",
    }
}

/// The pane as a viewer sees it. A waiting card (status, activity or a pending question) shows only `WAITING_LINE`.
/// The activity line is shown only for working and idle activity. `status` `None` means the pane is gone.
pub fn pane_view(
    key: &str,
    agent_label: &str,
    title: Option<&str>,
    status: Option<PaneStatus>,
    activity: Option<&Activity>,
    pending_card: bool,
    ctx: &RedactContext,
) -> LivePane {
    let label = clean(agent_label, PANE_CHARS, ctx);
    let title = title.map(|t| clean(t, PANE_CHARS, ctx)).unwrap_or_default();
    let activity_kind = activity.map(|a| a.kind);
    let (state, line) = match status {
        None => (LiveState::Gone, None),
        Some(_)
            if pending_card
                || status == Some(PaneStatus::WaitingInput)
                || activity_kind == Some(ActivityKind::Waiting) =>
        {
            (LiveState::Waiting, Some(WAITING_LINE.to_string()))
        }
        Some(PaneStatus::Error) => (LiveState::Error, None),
        Some(_) if activity_kind == Some(ActivityKind::Error) => (LiveState::Error, None),
        Some(st) => {
            let state = if st == PaneStatus::Running { LiveState::Working } else { LiveState::Idle };
            let line = activity
                .filter(|a| matches!(a.kind, ActivityKind::Working | ActivityKind::Idle))
                .and_then(|a| clean_text(&a.text, LINE_CHARS, ctx));
            (state, line)
        }
    };
    LivePane { key: key.to_string(), label, title, state, line }
}
