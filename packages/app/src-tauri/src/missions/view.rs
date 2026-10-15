//! What the desktop, the phone and `inspect_agent` read about a mission: plain words only
//! (UX-PHASE1 0.3: Mission, Preview, Separate copy, Checks, Budget, Backup agents, Hand off). Never
//! "worktree", "round", "tamper", "operation" or an error code.

use super::model::*;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Row {
    /// `turn` or `event`.
    pub kind: &'static str,
    /// One of the five glyphs: `~` working, `+` needs you, `v` done, `:` paused, `x` ended.
    pub glyph: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// `9 of 12` (tests) or `1 of 2 checks`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counts: Option<String>,
    /// `+2`, `−2`, `0`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta: Option<String>,
    pub text: String,
    /// `open_pane` | `show_diff` | `show_brief` | none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pane_id: Option<String>,
    pub at_ms: u64,
    /// The turn's checks, for the expanded row.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<CheckLine>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CheckLine {
    pub name: String,
    pub passed: bool,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BudgetView {
    pub turns_used: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turns: Option<u32>,
    pub minutes_used: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minutes: Option<u32>,
    /// `7 of 30 turns · 41 of 120 min`.
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CopyView {
    pub path: String,
    pub branch: String,
    pub base: String,
    pub base_commit: String,
    pub closed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct MissionView {
    pub id: String,
    pub handle: String,
    pub preview: bool,
    pub project: String,
    pub project_name: String,
    pub objective: String,
    pub title: String,
    pub state: &'static str,
    /// The exact state line of DESIGN-MISSIONS 2.5 ("Turn 7 · 9 of 12 pass").
    pub state_text: String,
    pub glyph: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub needs: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub card_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sentence: Option<String>,
    pub pause_requested: bool,
    pub turn: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub now: Option<String>,
    /// The progress bar: passed of total (tests when counted, else checks).
    pub passed: u32,
    pub total: u32,
    pub budget: BudgetView,
    pub agents: Vec<String>,
    /// "Claude Code → Codex": who worked on it, in order.
    pub agents_line: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_agent: Option<String>,
    /// The run handle of the agent working now: steer it with this, not the mission handle.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_run: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_pane: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub copy: Option<CopyView>,
    pub checks: Vec<CheckLine>,
    pub may_change_tests: bool,
    pub rollback: bool,
    /// The tamper card's lines (blocking test changes), at most 3 plus "and N more".
    pub test_change_lines: Vec<String>,
    /// The Done review's "Test changes" row.
    pub test_changes_text: String,
    pub timeline: Vec<Row>,
    pub started_by: String,
    /// Done review: turns, hand-offs and rollbacks.
    pub work_text: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at_ms: Option<u64>,
    pub rev: u64,
}

pub fn glyph(state: MissionState) -> &'static str {
    match state {
        MissionState::Planning | MissionState::Working | MissionState::Checking => "~",
        MissionState::NeedsYou => "+",
        MissionState::Green | MissionState::HandedOff => "v",
        MissionState::Paused | MissionState::Stopped => ":",
        MissionState::Failed => "x",
    }
}

fn plural(n: u32, unit: &str) -> String {
    if n == 1 {
        format!("{n} {unit}")
    } else {
        format!("{n} {unit}s")
    }
}

/// The exact state line (DESIGN-MISSIONS 2.5).
pub fn state_text(m: &Mission) -> String {
    let last = m.rounds.last();
    let turn = last.map(|r| r.n).unwrap_or(0);
    match m.state {
        MissionState::Planning if m.worktree.is_none() => "Getting ready · setting up the copy".into(),
        MissionState::Planning => "Getting ready · running checks once".into(),
        MissionState::Working => {
            let pass = match (m.rounds.iter().rev().find_map(|r| r.verdict.as_ref()), m.baseline.as_ref()) {
                (Some(v), _) => format!(" · {} pass", v.short()),
                (None, Some(b)) => format!(" · {} pass at the start", b.short()),
                _ => String::new(),
            };
            let paused = if m.pause_requested { " · pauses after this turn" } else { "" };
            format!("Turn {turn}{pass}{paused}")
        }
        MissionState::Checking => "Checking".into(),
        MissionState::Green => match m.latest_verdict() {
            Some(v) => format!("Done · {} pass · ready to review", v.short()),
            None => "Done · ready to review".into(),
        },
        MissionState::NeedsYou => match m.needs {
            Some(Needs::Tamper) => "Needs you · Tests were changed".into(),
            Some(Needs::Budget) => "Needs you · Budget used up".into(),
            Some(Needs::Setup) => "Needs you · Checks can't run".into(),
            Some(Needs::Stuck) => format!("Needs you · No progress after turn {turn}"),
            None => "Needs you".into(),
        },
        MissionState::Paused => "Paused".into(),
        MissionState::HandedOff => format!("Handed to you after turn {turn}"),
        MissionState::Stopped => {
            let used = m.rounds.iter().rev().find(|r| r.operation_id.is_some()).map(|r| r.n).unwrap_or(0);
            if used > 0 {
                format!("Stopped by you after turn {used}")
            } else {
                "Stopped by you".into()
            }
        }
        MissionState::Failed => format!("Ended · {}", m.sentence.clone().unwrap_or_else(|| "Puppet Master couldn't go on.".into())),
    }
}

fn delta(prev: Option<&Verdict>, now: &Verdict) -> String {
    let d = match (prev.and_then(|p| p.counts), now.counts) {
        (Some(p), Some(n)) => n.passed as i64 - p.passed as i64,
        _ => now.checks_passed as i64 - prev.map(|p| p.checks_passed as i64).unwrap_or(0),
    };
    if d > 0 {
        format!("+{d}")
    } else if d < 0 {
        format!("\u{2212}{}", -d)
    } else {
        "0".into()
    }
}

fn check_lines(v: &Verdict) -> Vec<CheckLine> {
    v.checks
        .iter()
        .map(|c| CheckLine {
            name: c.name.clone(),
            passed: c.passed,
            text: match c.counts.filter(|n| n.total > 0) {
                Some(n) => format!("{} of {} pass", n.passed, n.total),
                None if c.passed => "passes".into(),
                None => "fails".into(),
            },
        })
        .collect()
}

/// The event phrase of a turn row (DESIGN-MISSIONS 6.4).
fn turn_phrase(m: &Mission, r: &Round, live: bool) -> String {
    if live {
        return match m.state {
            MissionState::Checking => "checking\u{2026}".into(),
            _ => "working\u{2026}".into(),
        };
    }
    if let Some(end) = &r.end {
        if end.code_is(super::decide::EXIT_CODES) {
            return "closed unexpectedly".into();
        }
        if end.code_is(super::decide::TIMEOUT_CODES) {
            return format!("stopped after {} min", m.round_timeout_ms / 60_000);
        }
        if end.code_is(super::decide::RESTART_CODES) {
            return "ended when Puppet Master restarted".into();
        }
        if end.status == "not_started" {
            return "didn't start".into();
        }
    }
    if let Some(d) = &r.decision {
        if let Action::Rollback { to_round, .. } = d.action {
            return if to_round == 0 { "went back to the files from the start".into() } else { format!("went back to turn {to_round}'s files") };
        }
    }
    let mut parts = Vec::new();
    if r.outside_edit {
        parts.push("files changed outside the mission (by you?)".to_string());
    }
    if let Some(v) = &r.verdict {
        if v.test_changes.blocks {
            parts.push("changed the tests".into());
        }
    }
    parts.join(" · ")
}

pub fn timeline(m: &Mission) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    let mut prev: Option<&Verdict> = m.baseline.as_ref();
    let last_n = m.rounds.last().map(|r| r.n);
    for r in &m.rounds {
        let live = Some(r.n) == last_n && matches!(m.state, MissionState::Working | MissionState::Checking) && r.decision.is_none();
        let (counts, d) = match &r.verdict {
            Some(v) => (Some(v.short()), Some(delta(prev, v))),
            None => (None, None),
        };
        let glyph = if live {
            "~"
        } else if r.verdict.as_ref().is_some_and(|v| v.test_changes.blocks) && m.state == MissionState::NeedsYou && Some(r.n) == last_n {
            "+"
        } else {
            "v"
        };
        let action = if live {
            r.pane_id.as_ref().map(|_| "open_pane")
        } else if r.verdict.as_ref().is_some_and(|v| !v.checkpoint.is_empty()) {
            Some("show_diff")
        } else {
            None
        };
        rows.push(Row {
            kind: "turn",
            glyph: glyph.into(),
            turn: Some(r.n),
            agent: Some(m.agent_label(r.agent)),
            counts,
            delta: d,
            text: turn_phrase(m, r, live),
            action,
            pane_id: r.pane_id.clone(),
            at_ms: r.ended_at_ms.unwrap_or(r.started_at_ms),
            checks: r.verdict.as_ref().map(check_lines).unwrap_or_default(),
        });
        if let Some(v) = &r.verdict {
            prev = Some(v);
        }
    }
    for e in &m.events {
        rows.push(Row {
            kind: "event",
            glyph: e.glyph.clone(),
            turn: e.round,
            agent: None,
            counts: None,
            delta: None,
            text: e.text.clone(),
            action: (e.kind == "handed_off").then_some("show_brief"),
            pane_id: None,
            at_ms: e.at_ms,
            checks: Vec::new(),
        });
    }
    // Newest first; an event sorts after the turn it belongs to.
    rows.sort_by(|a, b| b.at_ms.cmp(&a.at_ms).then_with(|| (a.kind == "turn").cmp(&(b.kind == "turn"))));
    rows
}

fn budget(m: &Mission) -> BudgetView {
    let turns_used = m.turns_used();
    let minutes_used = (m.ended_at_ms.unwrap_or_else(super::store::now_ms).saturating_sub(m.created_at_ms) / 60_000) as u32;
    let mut parts = Vec::new();
    if let Some(t) = m.budget_turns {
        parts.push(format!("{turns_used} of {t} turns"));
    }
    if let Some(min) = m.budget_minutes {
        parts.push(format!("{minutes_used} of {min} min"));
    }
    BudgetView { turns_used, turns: m.budget_turns, minutes_used, minutes: m.budget_minutes, text: parts.join(" · ") }
}

fn project_name(path: &str) -> String {
    path.split(['\\', '/']).filter(|s| !s.is_empty()).last().unwrap_or(path).to_string()
}

pub fn view(m: &Mission) -> MissionView {
    let latest = m.latest_verdict();
    let (passed, total) = match latest {
        Some(v) => match v.counts.filter(|c| c.total > 0) {
            Some(c) => (c.passed, c.total),
            None => (v.checks_passed, v.checks_total),
        },
        None => (0, 0),
    };
    let mut worked: Vec<String> = Vec::new();
    for r in &m.rounds {
        let label = m.agent_label(r.agent);
        if worked.last() != Some(&label) {
            worked.push(label);
        }
    }
    let current = m.rounds.last();
    let hand_offs = m.rounds.iter().filter(|r| matches!(r.kind, RoundKind::HandOff { .. })).count() as u32;
    let rollbacks = m.rounds.iter().filter(|r| r.decision.as_ref().is_some_and(|d| matches!(d.action, Action::Rollback { .. }))).count() as u32;
    let mut per_agent: Vec<(String, u32)> = Vec::new();
    for r in m.rounds.iter().filter(|r| r.operation_id.is_some()) {
        let label = m.agent_label(r.agent);
        match per_agent.iter_mut().find(|(l, _)| *l == label) {
            Some((_, n)) => *n += 1,
            None => per_agent.push((label, 1)),
        }
    }
    let mut work = vec![plural(m.turns_used(), "turn")];
    if !per_agent.is_empty() {
        work.push(per_agent.iter().map(|(l, n)| format!("{l} {n}")).collect::<Vec<_>>().join(", "));
    }
    if hand_offs > 0 {
        work.push(plural(hand_offs, "hand-off"));
    }
    if rollbacks > 0 {
        work.push(if rollbacks == 1 { "1 time went back".into() } else { format!("{rollbacks} times went back") });
    }
    let test_changes_text = match (&m.tests_accepted, latest) {
        (Some(a), _) => format!("accepted by you · {}", a.summary.join(", ")),
        (None, Some(v)) if v.test_changes.findings.is_empty() => "none".into(),
        (None, Some(v)) if m.may_change_tests => format!("changed, allowed by the mission · {}", plural(v.test_changes.findings.len() as u32, "change")),
        (None, Some(v)) if v.test_changes.blocks => v.test_changes.blocking_sentences().join(", "),
        (None, Some(_)) => "small changes, no tests skipped or removed".into(),
        (None, None) => "none".into(),
    };
    MissionView {
        id: m.id.clone(),
        handle: m.handle(),
        preview: true,
        project: m.project.clone(),
        project_name: project_name(&m.project),
        objective: m.objective.clone(),
        title: super::engine::short(&m.objective),
        state: m.state.as_str(),
        state_text: state_text(m),
        glyph: glyph(m.state),
        needs: m.needs.map(Needs::as_str),
        card_title: super::cards::headline(m),
        sentence: m.sentence.clone(),
        pause_requested: m.pause_requested,
        turn: current.map(|r| r.n).unwrap_or(0),
        start: m.baseline.as_ref().map(Verdict::short),
        now: latest.map(Verdict::short),
        passed,
        total,
        budget: budget(m),
        agents: m.agents.iter().map(|a| agent_label(&a.agent_type)).collect(),
        agents_line: worked.join(" \u{2192} "),
        current_agent: current.map(|r| m.agent_label(r.agent)),
        current_run: current.and_then(|r| r.handle.clone()).filter(|_| !m.state.terminal()),
        current_pane: current.and_then(|r| r.pane_id.clone()),
        copy: m.worktree.as_ref().map(|w| CopyView {
            path: w.cwd.clone(),
            branch: w.branch.clone(),
            base: w.base.clone(),
            base_commit: w.base_commit.chars().take(7).collect(),
            closed: w.closed.is_some(),
        }),
        checks: m
            .checks
            .iter()
            .enumerate()
            .map(|(i, c)| CheckLine {
                name: c.name.clone(),
                passed: latest.and_then(|v| v.checks.get(i)).is_some_and(|o| o.passed),
                text: c.display(),
            })
            .collect(),
        may_change_tests: m.may_change_tests,
        rollback: m.rollback,
        test_change_lines: super::cards::tamper_lines(m),
        test_changes_text,
        timeline: timeline(m),
        started_by: match &m.started_by {
            Starter::Desktop => "you".into(),
            Starter::Agent { label, .. } => label.clone(),
        },
        work_text: work.join(" · "),
        created_at_ms: m.created_at_ms,
        updated_at_ms: m.updated_at_ms,
        ended_at_ms: m.ended_at_ms,
        rev: m.rev,
    }
}
