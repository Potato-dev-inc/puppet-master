//! The decision table (DESIGN-MISSIONS 3.2): what happens after a turn. A pure function of the
//! mission (whose last round has its end and verdict) and a few signals; rules are checked in order.
//! The budget (rule 10) is checked by the engine before any next turn starts, so it always wins
//! over "keep going".

use super::model::{Action, Decision, HandOffWhy, Mission, Needs, Round, RoundKind, Verdict};

/// Codes of a turn that never got going for a reason only the person can fix. Trust and sign-in
/// questions are never answered for the person.
pub const SETUP_CODES: &[&str] = &[
    "WORKSPACE_TRUST_DECLINED",
    "STARTUP_SCREEN_NOT_COMPLETED",
    "PANE_BLOCKED_BY_PROMPT",
    "AGENT_NOT_INSTALLED",
    "AGENT_NOT_DISPATCHABLE",
    "UNKNOWN_AGENT_TYPE",
    "MISSION_HEADLESS",
    "ACCOUNT_MISMATCH",
];
pub const EXIT_CODES: &[&str] = &["AGENT_EXITED", "AGENT_GONE"];
pub const RESTART_CODES: &[&str] = &["OPERATION_INTERRUPTED"];
pub const TIMEOUT_CODES: &[&str] = &["TIMEOUT"];
/// The turn never started in its pane (the prompt was not taken, the pane did not launch).
pub const PRE_TURN_CODES: &[&str] = &["PROMPT_NOT_SUBMITTED", "PROMPT_NOT_DELIVERED", "DISPATCH_FAILED", "PANE_SPAWN_FAILED", "LAUNCH_FAILED"];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Signals {
    pub stop: bool,
    pub pause: bool,
    /// The loop guard flagged the round's pane (`repeated_error`, `stalled`, ...).
    pub loop_signal: Option<String>,
    /// Rounds in a row without progress before the agent counts as stuck.
    pub stall: u32,
}

/// The best files so far: the highest `(checks passed, tests passed)` among the start and every
/// turn, later turns winning ties. Turns with a blocking test change are never "best".
pub fn best(m: &Mission, before_round: u32) -> Option<(u32, &Verdict)> {
    let mut best: Option<(u32, &Verdict)> = m.baseline.as_ref().map(|v| (0, v));
    for r in m.rounds.iter().filter(|r| r.n < before_round) {
        let Some(v) = r.verdict.as_ref() else { continue };
        if v.test_changes.blocks || v.checkpoint.is_empty() {
            continue;
        }
        if best.is_none_or(|(_, b)| v.score() >= b.score()) {
            best = Some((r.n, v));
        }
    }
    best
}

/// A check that passed in the best files fails now, or fewer tests pass while no fewer ran (fewer
/// tests running is a test change, not a regression).
pub fn regressed(best: &Verdict, now: &Verdict) -> bool {
    let check_lost = best.checks.iter().zip(&now.checks).any(|(b, n)| b.passed && !n.passed);
    let tests_lost = match (best.counts, now.counts) {
        (Some(b), Some(n)) => n.passed < b.passed && n.total >= b.total,
        _ => false,
    };
    check_lost || tests_lost
}

/// Whether `now` moved on from `prev`: more checks or tests pass, the failure is a different one,
/// or files changed without anything getting worse.
pub fn progressed(prev: &Verdict, now: &Verdict) -> bool {
    let more = now.score() > prev.score();
    let new_failure = now.failing_key != prev.failing_key;
    let worse = regressed(prev, now);
    let files = now.tree != prev.tree;
    more || new_failure || (files && !worse)
}

/// Rounds of the current agent since it took over (newest last).
fn current_stint(m: &Mission) -> Vec<&Round> {
    let agent = m.roster.current;
    let mut stint: Vec<&Round> = Vec::new();
    for r in m.rounds.iter().rev() {
        if r.agent != agent {
            break;
        }
        stint.push(r);
        if matches!(r.kind, RoundKind::HandOff { .. } | RoundKind::First | RoundKind::Retry) {
            break;
        }
    }
    stint.reverse();
    stint
}

/// How many rounds in a row, ending with the last, made no progress.
pub fn stalled_rounds(m: &Mission) -> u32 {
    let mut count = 0;
    let rounds: Vec<&Round> = m.rounds.iter().filter(|r| r.verdict.is_some()).collect();
    for (i, r) in rounds.iter().enumerate().rev() {
        let now = r.verdict.as_ref().unwrap();
        let prev = if i == 0 { m.baseline.as_ref() } else { rounds[i - 1].verdict.as_ref() };
        let Some(prev) = prev else { break };
        if progressed(prev, now) {
            break;
        }
        count += 1;
    }
    count
}

/// The next agent to try, or `None` when nobody is left. The list wraps once: after every agent
/// had a go, the first one gets a fresh conversation.
pub fn next_agent(m: &Mission) -> Option<usize> {
    if m.roster.wrapped {
        // Everyone had a go and the first agent had its fresh try: ask the person.
        return None;
    }
    let n = m.agents.len();
    let current = m.roster.current;
    for step in 1..n {
        let i = (current + step) % n;
        if !m.roster.out.contains(&i) {
            return Some(i);
        }
    }
    (!m.roster.wrapped).then_some(0)
}

fn plural(n: u32, unit: &str) -> String {
    if n == 1 {
        format!("{n} {unit}")
    } else {
        format!("{n} {unit}s")
    }
}

fn hand_off_or_stuck(m: &Mission, why: HandOffWhy, line_for: impl Fn(&str) -> String) -> Decision {
    let n = m.last_round().map(|r| r.n).unwrap_or(0);
    match next_agent(m) {
        Some(to) => Decision { action: Action::HandOff { to, why }, line: line_for(&m.agent_label(to)) },
        None => Decision { action: Action::NeedsYou { why: Needs::Stuck }, line: format!("No progress after turn {n}") },
    }
}

/// `9 of 12 pass (+2)`.
fn follow_up_line(prev: Option<&Verdict>, now: &Verdict) -> String {
    let delta = match (prev.and_then(|p| p.counts), now.counts) {
        (Some(p), Some(n)) => n.passed as i64 - p.passed as i64,
        _ => now.checks_passed as i64 - prev.map(|p| p.checks_passed as i64).unwrap_or(0),
    };
    let sign = if delta > 0 { format!(" (+{delta})") } else if delta < 0 { format!(" (\u{2212}{})", -delta) } else { " (0)".into() };
    match now.counts.filter(|c| c.total > 0) {
        Some(c) => format!("{} of {} pass{sign}", c.passed, c.total),
        None => format!("{} of {} checks pass{sign}", now.checks_passed, now.checks_total),
    }
}

/// Decide what follows the mission's last round. The last round has its `end` (and, unless it
/// could not start, its `verdict`).
pub fn decide(m: &Mission, sig: &Signals) -> Decision {
    let line = |action: Action, line: String| Decision { action, line };
    let Some(round) = m.last_round() else {
        return line(Action::FollowUp, String::new());
    };
    let agent = m.agent_label(round.agent);
    let end = round.end.clone().unwrap_or_default();

    // 1, 2: the person.
    if sig.stop {
        return line(Action::Stop, "Stopped by you".into());
    }
    // 3: a turn that never got going for a reason only the person can fix.
    if end.code_is(SETUP_CODES) {
        let sentence = end.message.clone().unwrap_or_else(|| "it is waiting for you".into());
        return line(Action::NeedsYou { why: Needs::Setup }, format!("{agent} couldn't start: {sentence}"));
    }
    if sig.pause {
        return line(Action::Pause, "Paused by you".into());
    }
    // A follow-up that could not be created (its pane was closed) or never got its prompt: a new
    // conversation, once; twice in a row asks the person.
    if end.status == "not_started" || end.code_is(PRE_TURN_CODES) {
        let previous_too = m
            .rounds
            .iter()
            .rev()
            .nth(1)
            .and_then(|r| r.end.as_ref())
            .is_some_and(|e| e.status == "not_started" || e.code_is(PRE_TURN_CODES));
        if previous_too {
            let sentence = end.message.clone().unwrap_or_else(|| "it didn't start".into());
            return line(Action::NeedsYou { why: Needs::Setup }, format!("{agent} couldn't start: {sentence}"));
        }
        let why = if end.status == "not_started" { "'s pane was gone" } else { " didn't get the prompt" };
        return line(Action::Resume { after_restart: false }, format!("{agent}{why}. Started it again."));
    }
    // 4: the agent's program ended.
    if end.code_is(EXIT_CODES) {
        let exits = current_stint(m).iter().filter(|r| r.end.as_ref().is_some_and(|e| e.code_is(EXIT_CODES))).count();
        if exits <= 1 {
            return line(Action::Resume { after_restart: false }, format!("{agent} closed unexpectedly. Started it again."));
        }
        return hand_off_or_stuck(m, HandOffWhy::Exited, |next| format!("Handed to {next}: {agent} closed twice"));
    }
    let Some(now) = round.verdict.as_ref() else {
        return line(Action::NeedsYou { why: Needs::Setup }, "The checks didn't run".into());
    };
    // 6: checks that can't start in the copy.
    if let Some(c) = now.cant_run() {
        return line(Action::NeedsYou { why: Needs::Setup }, format!("The checks can't run in the separate copy: {}", c.detail));
    }
    // 7: green, unless the tests were changed to get there.
    if now.all_pass() {
        if now.test_changes.blocks && !m.may_change_tests && m.tests_accepted.is_none() {
            return line(Action::NeedsYou { why: Needs::Tamper }, "The checks pass, but the tests were changed".into());
        }
        let all = match now.counts.filter(|c| c.total > 0) {
            Some(c) => format!("All {} pass", c.total),
            None => format!("All {} pass", plural(now.checks_total, "check")),
        };
        return line(Action::Green, all);
    }
    // The turn ended because Puppet Master restarted: its pane is gone, a new one continues.
    if end.code_is(RESTART_CODES) {
        return line(Action::Resume { after_restart: true }, "Resumed after Puppet Master restarted".into());
    }
    let prev = m.rounds.iter().rev().nth(1).and_then(|r| r.verdict.as_ref()).or(m.baseline.as_ref());
    // 8: worse than the best files so far: go back to them.
    if m.rollback {
        if let Some((to_round, best_v)) = best(m, round.n) {
            if regressed(best_v, now) {
                let after_rollback_before = m.rounds.iter().rev().nth(1).and_then(|r| r.decision.as_ref()).is_some_and(|d| matches!(d.action, Action::Rollback { .. }));
                let next = if after_rollback_before {
                    let d = hand_off_or_stuck(m, HandOffWhy::Regressed, |_| String::new());
                    d.action
                } else {
                    Action::FollowUp
                };
                let target = if to_round == 0 { "the files from the start".to_string() } else { format!("turn {to_round}'s files") };
                return line(
                    Action::Rollback { to_round, next: Box::new(next) },
                    format!("Tests got worse ({} \u{2192} {}). Went back to {target}.", best_v.short(), now.short()),
                );
            }
        }
    }
    // 9: no progress for N turns, a loop on the pane, or two timeouts in a row.
    let stint = current_stint(m);
    let timeouts = stint.iter().rev().take(2).filter(|r| r.end.as_ref().is_some_and(|e| e.code_is(TIMEOUT_CODES))).count();
    let stalled = stalled_rounds(m);
    let n_stall = sig.stall.clamp(3, 10);
    let stint_stalled = stalled.min(stint.len() as u32) >= n_stall;
    if stint_stalled || sig.loop_signal.is_some() || timeouts >= 2 {
        let turns = stalled.max(1).min(stint.len() as u32).max(1);
        return hand_off_or_stuck(m, HandOffWhy::NoProgress, |next| format!("Handed to {next}: {agent} made no progress in {}", plural(turns, "turn")));
    }
    // 11: keep going.
    line(Action::FollowUp, follow_up_line(prev, now))
}
