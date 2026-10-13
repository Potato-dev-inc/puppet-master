//! The data of one finished turn, and the plain sentence that describes it. No LLM: every part is
//! a recorded fact (checkpoint diff, answered prompts, pane status, run receipt).

use serde::{Deserialize, Serialize};

use crate::checkpoints::FileChange;

/// How many file names a summary keeps (the count is always exact).
const MAX_PATHS: usize = 8;
/// A prompt is shortened to this many characters in a sentence.
const MAX_PROMPT_CHARS: usize = 80;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileStats {
    pub count: u32,
    pub added: u32,
    pub deleted: u32,
    /// The first few paths, for tooltips.
    pub paths: Vec<String>,
}

impl FileStats {
    /// Totals of a checkpoint diff (the changes between "before turn n" and "after turn n").
    pub fn from_changes(changes: &[FileChange]) -> Self {
        Self {
            count: changes.len() as u32,
            added: changes.iter().map(|c| c.added).sum(),
            deleted: changes.iter().map(|c| c.deleted).sum(),
            paths: changes.iter().take(MAX_PATHS).map(|c| c.path.clone()).collect(),
        }
    }
}

/// One prompt the user (or the cockpit on their behalf) answered during the turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Approval {
    pub at_ms: u64,
    pub prompt_id: String,
    /// What was asked, shortened ("allow `npm test`?"). May be empty when it was not on screen.
    pub prompt: String,
    /// Prompt kind as the detector reported it (`command`, `edit`, ...), or `request`.
    pub kind: String,
    /// `allow` or `deny`.
    pub decision: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EndState {
    /// `idle`, `waiting_input` or `error`.
    pub kind: String,
    /// What the agent is asking, when it is waiting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
}

/// What a run-backed pane's receipt says about the turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceiptFacts {
    /// `none configured` or `<passed>/<total> passed`.
    pub checks: String,
    /// `passed_checks`, `failed_checks`, `completed_unchecked`, `failed`, `cancelled`, ...
    pub outcome: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnSummary {
    pub pane_id: String,
    /// Display name of the agent ("Claude Code").
    pub agent: String,
    pub turn: u32,
    pub started_at_ms: u64,
    pub ended_at_ms: u64,
    pub duration_ms: u64,
    /// `None` = unknown (no checkpoints for this pane).
    pub files: Option<FileStats>,
    pub approvals: Vec<Approval>,
    pub end: EndState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt: Option<ReceiptFacts>,
    /// The one-line plain sentence (`render_sentence`).
    pub sentence: String,
}

/// Everything `assemble` needs; built by the tracker from the outside world.
#[derive(Debug, Clone)]
pub struct Facts {
    pub pane_id: String,
    pub agent: String,
    pub turn: u32,
    pub started_at_ms: u64,
    pub ended_at_ms: u64,
    pub files: Option<FileStats>,
    pub approvals: Vec<Approval>,
    pub end_status: String,
    pub end_prompt: Option<String>,
    pub receipt: Option<ReceiptFacts>,
}

/// Turn the recorded facts into a summary (with its sentence).
pub fn assemble(facts: Facts) -> TurnSummary {
    let kind = match facts.end_status.as_str() {
        "waiting_input" | "error" => facts.end_status.clone(),
        _ => "idle".to_string(),
    };
    let prompt = if kind == "waiting_input" {
        facts.end_prompt.as_deref().map(shorten).filter(|p| !p.is_empty())
    } else {
        None
    };
    let mut summary = TurnSummary {
        pane_id: facts.pane_id,
        agent: facts.agent,
        turn: facts.turn,
        started_at_ms: facts.started_at_ms,
        ended_at_ms: facts.ended_at_ms,
        duration_ms: facts.ended_at_ms.saturating_sub(facts.started_at_ms),
        files: facts.files,
        approvals: facts.approvals,
        end: EndState { kind, prompt },
        receipt: facts.receipt,
        sentence: String::new(),
    };
    summary.sentence = render_sentence(&summary);
    summary
}

/// One line of whitespace-collapsed text, at most `MAX_PROMPT_CHARS` characters.
pub fn shorten(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= MAX_PROMPT_CHARS {
        return flat;
    }
    let cut: String = flat.chars().take(MAX_PROMPT_CHARS - 1).collect();
    format!("{}\u{2026}", cut.trim_end())
}

fn plural(n: u32, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// "42s", "5m", "1h 12m".
pub fn short_duration(ms: u64) -> String {
    let s = ms / 1000;
    if s < 60 {
        return format!("{s}s");
    }
    let m = s / 60;
    if m < 60 {
        return format!("{m}m");
    }
    let (h, rem) = (m / 60, m % 60);
    if rem == 0 {
        format!("{h}h")
    } else {
        format!("{h}h {rem}m")
    }
}

fn approvals_part(approvals: &[Approval]) -> Option<String> {
    let noun = |items: &[&Approval]| {
        let n = items.len() as u32;
        if items.iter().all(|a| a.kind == "command") {
            plural(n, "command", "commands")
        } else {
            plural(n, "request", "requests")
        }
    };
    let allowed: Vec<&Approval> = approvals.iter().filter(|a| a.decision == "allow").collect();
    let denied: Vec<&Approval> = approvals.iter().filter(|a| a.decision == "deny").collect();
    let mut parts = Vec::new();
    if !allowed.is_empty() {
        parts.push(format!("you approved {}", noun(&allowed)));
    }
    if !denied.is_empty() {
        parts.push(format!("you declined {}", noun(&denied)));
    }
    (!parts.is_empty()).then(|| parts.join(", "))
}

fn receipt_part(receipt: &ReceiptFacts) -> Option<String> {
    let counts = receipt.checks.split_whitespace().next().filter(|c| c.contains('/'));
    match (receipt.outcome.as_str(), counts) {
        ("passed_checks", Some(c)) => Some(format!("all checks passed ({c})")),
        ("passed_checks", None) => Some("all checks passed".into()),
        ("failed_checks", Some(c)) => Some(format!("checks failed ({c} passed)")),
        ("failed_checks", None) => Some("checks failed".into()),
        ("completed_unchecked", _) => Some("no checks ran".into()),
        ("failed", _) => Some("the run failed".into()),
        ("cancelled", _) => Some("the run was cancelled".into()),
        _ => None,
    }
}

/// "Claude Code edited 4 files (+86 −12), you approved 1 command, now waiting: allow `npm test`?"
pub fn render_sentence(s: &TurnSummary) -> String {
    let head = match &s.files {
        Some(f) if f.count > 0 => {
            format!("{} edited {} (+{} \u{2212}{})", s.agent, plural(f.count, "file", "files"), f.added, f.deleted)
        }
        Some(_) => format!("{} made no file changes", s.agent),
        None => format!("{} worked for {}", s.agent, short_duration(s.duration_ms)),
    };
    let mut parts = vec![head];
    if let Some(a) = approvals_part(&s.approvals) {
        parts.push(a);
    }
    if let Some(r) = s.receipt.as_ref().and_then(receipt_part) {
        parts.push(r);
    }
    parts.push(match (s.end.kind.as_str(), &s.end.prompt) {
        ("waiting_input", Some(p)) => format!("now waiting: {p}"),
        ("waiting_input", None) => "now waiting for you".into(),
        ("error", _) => "ended with an error".into(),
        _ => "now idle".into(),
    });
    let mut line = parts.join(", ");
    if !line.ends_with('?') {
        line.push('.');
    }
    line
}
