//! The end-of-Focus summary (SPEC n33 3.4 D). Pure: `build` takes the session, the ledger, the finished turns and
//! the number of cards still open, and returns the counts and one plain sentence. No clock reads, no I/O.
//!
//! Counts only. Nothing here carries a name, a pane id or a sentence. The per-agent lines for the desktop card come
//! from `turn_summary::digest`, not from here.

use super::model::{HeldCounts, Ledger, Session, Tier, Waiter};
use crate::turn_summary::TurnSummary;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Why Focus ended: the time ran out, or the person ended it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndedBy {
    Timer,
    Early,
}

/// The numbers in one summary. Counts only: no names, no ids, no sentences.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counts {
    pub turns_done: u32,
    pub agents: u32,
    pub failed: u32,
    pub asked: u32,
    pub urgent: u32,
    pub still_waiting: u32,
    #[serde(default)]
    pub held: HeldCounts,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FocusSummary {
    pub session_id: String,
    pub started_at_ms: u64,
    pub ended_at_ms: u64,
    pub minutes: u32,
    pub ended_by: EndedBy,
    pub counts: Counts,
    /// One plain sentence, for example `While you focused (45 min): 4 turns done, 1 failed.`
    pub line: String,
}

/// One finished turn, as the summary needs it. The text of the turn is not kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnFact {
    pub pane_id: String,
    pub turn: u32,
    pub ended_at_ms: u64,
    /// `idle`, `waiting_input` or `error`.
    pub end: String,
    /// The run receipt's outcome (`passed_checks`, `failed_checks`, ...), when the pane has one.
    pub outcome: Option<String>,
}

impl From<&TurnSummary> for TurnFact {
    fn from(turn: &TurnSummary) -> Self {
        Self {
            pane_id: turn.pane_id.clone(),
            turn: turn.turn,
            ended_at_ms: turn.ended_at_ms,
            end: turn.end.kind.clone(),
            outcome: turn.receipt.as_ref().map(|r| r.outcome.clone()),
        }
    }
}

/// Build the one summary of a session. `open_now` is how many blocking cards are open at the end.
pub fn build(
    session: &Session,
    ledger: &Ledger,
    turns: &[TurnFact],
    open_now: u32,
    ended_at_ms: u64,
    ended_by: EndedBy,
) -> FocusSummary {
    let counts = counts(session, ledger, turns, open_now);
    let actual_ms = ended_at_ms.saturating_sub(session.started_at_ms);
    FocusSummary {
        session_id: session.id.clone(),
        started_at_ms: session.started_at_ms,
        ended_at_ms,
        minutes: session.minutes,
        ended_by,
        line: line(&counts, session.minutes, ended_by, actual_ms),
        counts,
    }
}

/// The counts of one session. Only turns that ended at or after the start count; each turn is counted once
/// (by its pane and turn number).
fn counts(session: &Session, ledger: &Ledger, turns: &[TurnFact], open_now: u32) -> Counts {
    let mut panes = BTreeSet::new();
    let mut done = BTreeSet::new();
    let mut failed = BTreeSet::new();
    for turn in turns.iter().filter(|t| t.ended_at_ms >= session.started_at_ms) {
        let key = (turn.pane_id.as_str(), turn.turn);
        panes.insert(turn.pane_id.as_str());
        if turn.end == "idle" {
            done.insert(key);
        }
        if turn.end == "error" || matches!(turn.outcome.as_deref(), Some("failed_checks" | "failed")) {
            failed.insert(key);
        }
    }
    let (asked, urgent) = waiting_counts(&ledger.waiters);
    Counts {
        turns_done: count_of(done.len()),
        agents: count_of(panes.len()),
        // A failed run shows up both as a turn and as a held run. Take the larger of the two, never the sum.
        failed: count_of(failed.len()).max(ledger.counts.run_failed),
        asked,
        urgent,
        still_waiting: open_now,
        held: ledger.counts.clone(),
    }
}

/// How many waiting cards are Quiet (they asked for you) and how many are Through (urgent).
pub fn waiting_counts(waiters: &[Waiter]) -> (u32, u32) {
    let (mut quiet, mut urgent) = (0u32, 0u32);
    for waiter in waiters {
        match waiter.tier {
            Tier::Quiet => quiet += 1,
            Tier::Through => urgent += 1,
            Tier::Hold => {}
        }
    }
    (quiet, urgent)
}

/// The full sentence: `While you focused (45 min): 4 turns done, 1 failed. 1 still waits.` An early end reads
/// `Focus ended after 18 min: ...`. With nothing to report it reads `Quiet. Nothing happened while you focused (45 min).`
pub fn line(counts: &Counts, minutes: u32, ended_by: EndedBy, actual_ms: u64) -> String {
    let head = match ended_by {
        EndedBy::Timer => format!("While you focused ({minutes} min)"),
        EndedBy::Early => format!("Focus ended after {}", span(actual_ms)),
    };
    let body = body(counts);
    if body.is_empty() {
        let length = match ended_by {
            EndedBy::Timer => format!("{minutes} min"),
            EndedBy::Early => span(actual_ms),
        };
        return format!("Quiet. Nothing happened while you focused ({length}).");
    }
    format!("{head}: {body}")
}

/// The counts part only, with no header: `4 turns done, 1 failed, 2 asked for you. 1 still waits.` Used by the
/// phone push and Telegram. Empty when there is nothing to say.
pub fn body(counts: &Counts) -> String {
    let mut parts = Vec::new();
    if counts.turns_done > 0 {
        parts.push(plural(counts.turns_done, "turn done", "turns done"));
    }
    if counts.failed > 0 {
        parts.push(format!("{} failed", counts.failed));
    }
    if counts.asked > 0 {
        parts.push(format!("{} asked for you", counts.asked));
    }
    if counts.urgent > 0 {
        parts.push(format!("{} urgent", counts.urgent));
    }
    let held = counts.held.companion + counts.held.review + counts.held.night;
    if held > 0 {
        parts.push(plural(held, "update held", "updates held"));
    }

    let mut out = String::new();
    if !parts.is_empty() {
        out.push_str(&parts.join(", "));
        out.push('.');
    }
    if counts.still_waiting > 0 {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&if counts.still_waiting == 1 {
            "1 still waits.".to_string()
        } else {
            format!("{} still wait.", counts.still_waiting)
        });
    }
    out
}

/// True when nothing happened: no agent turns, nothing held, nothing asked, nothing still waiting.
/// An empty summary shows no card and sends nothing.
pub fn is_empty(summary: &FocusSummary) -> bool {
    let c = &summary.counts;
    c.turns_done == 0
        && c.agents == 0
        && c.failed == 0
        && c.asked == 0
        && c.urgent == 0
        && c.still_waiting == 0
        && c.held.total() == 0
}

/// `1 turn done` / `4 turns done`.
fn plural(n: u32, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// `45 min`, or `less than 1 min` under a whole minute.
fn span(ms: u64) -> String {
    match ms / 60_000 {
        0 => "less than 1 min".to_string(),
        minutes => format!("{minutes} min"),
    }
}

fn count_of(len: usize) -> u32 {
    u32::try_from(len).unwrap_or(u32::MAX)
}
