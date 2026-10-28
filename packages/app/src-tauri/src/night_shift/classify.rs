//! Results and the exact words (SPEC n32 section 3.6). Pure: no I/O, no clock.
//!
//! `classify` turns one task's tries and the facts read from its copy into an `Outcome`. The
//! agent's name never appears in this file: it arrives as data in `Attempt.label`.

use crate::missions::model::{MissionState, Needs, Verdict};
use crate::worktrees::Changes;

use super::model::{Attempt, AttemptEnd, Class, Counts, CopyRef, Outcome, Task};

/// Most turn sentences kept on an outcome.
pub const MAX_SUMMARIES: usize = 8;
/// Most test-change lines kept before "and N more changes".
const MAX_TEST_LINES: usize = 3;
const MS_PER_MIN: u64 = 60_000;

pub const DONE_NO_CHECKS: &str = "Done. No tests declared, nothing was checked.";
pub const MADE_NO_CHANGES: &str = "Made no changes.";
pub const TAMPER: &str = "The checks pass, but the tests were changed";
pub const ASKED: &str = "Stopped to ask a question";
pub const REASON_OPEN_TERMINAL: &str = "Open its terminal to answer, then run the task again.";
pub const REASON_TAMPER: &str = "Check the test changes before you keep it.";
pub const REASON_TESTS: &str = "Look at the failing tests before you keep it.";
pub const REASON_STUCK: &str = "Look at what it changed, then keep it or throw it away.";
pub const REASON_BUDGET: &str = "Look at what it changed, or run the task again with more turns.";
pub const REASON_RUN_AGAIN: &str = "Run the task again to finish it.";
pub const REASON_USAGE: &str = "Run the task again when a plan has usage left.";
pub const REASON_FAILED: &str = "Run the task again, or open its terminal to see why.";
pub const REASON_LOOK: &str = "Look at the copy, then run the task again if it is not done.";
pub const REASON_OPEN_COPY: &str = "Open the copy to look at the files.";

/// What the io layer read about one task after its last try (NS-8 fills it).
pub struct Facts {
    /// The mission's view when the task ran as a mission. None for a single turn.
    pub mission: Option<MissionFacts>,
    /// The copy's changes against its start. None when they could not be read.
    pub changes: Option<Changes>,
    /// Turn sentences, already redacted.
    pub summaries: Vec<String>,
    /// The copy that holds the result.
    pub copy: Option<CopyRef>,
    /// The hard stop time on the local clock, e.g. "07:00".
    pub stop_hhmm: String,
    /// Why nothing ran, in plain words, e.g. "No time left".
    pub not_run: Option<String>,
}

/// The mission facts `classify` needs.
pub struct MissionFacts {
    pub state: MissionState,
    pub needs: Option<Needs>,
    /// The latest check result of the copy.
    pub verdict: Option<Verdict>,
    /// The check result before the first turn (round 0).
    pub baseline: Option<Verdict>,
    /// Turns the mission used.
    pub turns_used: u32,
}

/// The class, headline and reason of one result, before the shared fields are filled in.
struct Call {
    class: Class,
    headline: String,
    reason: Option<&'static str>,
}

fn call(class: Class, headline: impl Into<String>, reason: Option<&'static str>) -> Call {
    Call { class, headline: headline.into(), reason }
}

/// The result of one task. Pure.
pub fn classify(task: &Task, f: &Facts) -> Outcome {
    let Some(last) = task.attempts.last() else {
        let why = f.not_run.clone().unwrap_or_else(|| "it never started".to_string());
        return assemble(task, f, call(Class::DidntRun, format!("Didn't run: {why}"), None));
    };
    let c = match &f.mission {
        Some(m) => mission_call(task, last, m, f),
        None => single_call(task, last, f),
    };
    assemble(task, f, c)
}

/// The sentence for a gate code, with the agent's label as data. None for codes not listed here.
pub fn gate_sentence(code: &str, label: &str) -> Option<String> {
    match code {
        "WORKSPACE_TRUST_DECLINED" => Some(format!("{label} asked to trust the folder.")),
        "STARTUP_SCREEN_NOT_COMPLETED" | "PANE_BLOCKED_BY_PROMPT" => {
            Some(format!("{label} stopped at a question before it started."))
        }
        "AGENT_NOT_INSTALLED" => Some(format!("{label} isn't installed.")),
        "ACCOUNT_MISMATCH" => Some(format!("{label} isn't signed in to that account.")),
        _ => None,
    }
}

fn mission_call(task: &Task, last: &Attempt, m: &MissionFacts, f: &Facts) -> Call {
    if m.state == MissionState::Green {
        return green_call(m, f);
    }
    match m.needs {
        Some(Needs::Tamper) => call(Class::NeedsLook, TAMPER, Some(REASON_TAMPER)),
        Some(Needs::Budget) => call(
            Class::NeedsLook,
            format!("Used its budget: {} of {} turns", m.turns_used, task.spec.turns),
            Some(REASON_BUDGET),
        ),
        Some(Needs::Stuck) => call(
            Class::NeedsLook,
            format!("It stopped making progress after {} turns", m.turns_used),
            Some(REASON_STUCK),
        ),
        Some(Needs::Setup) | None => from_end(task, last, f, Some(m)),
    }
}

/// Every check passes at the end of a mission. Blocking test changes win over a green check.
fn green_call(m: &MissionFacts, f: &Facts) -> Call {
    let Some(v) = m.verdict.as_ref() else {
        return call(Class::NeedsLook, "No check result was recorded", Some(REASON_LOOK));
    };
    if v.test_changes.blocks || !v.test_changes.blocking_sentences().is_empty() {
        return call(Class::NeedsLook, TAMPER, Some(REASON_TAMPER));
    }
    if !v.all_pass() {
        return tests_fail_call(v, m.baseline.as_ref());
    }
    match &f.changes {
        None => call(Class::NeedsLook, "Couldn't read the changes", Some(REASON_OPEN_COPY)),
        Some(c) if c.files_changed == 0 => call(Class::NoChanges, MADE_NO_CHANGES, None),
        Some(_) => call(Class::Ready, format!("Tests pass {}", v.short()), None),
    }
}

/// Checks fail at the end. When the tests already failed before the task, say so with both numbers.
fn tests_fail_call(v: &Verdict, baseline: Option<&Verdict>) -> Call {
    let before = baseline.filter(|b| !b.all_pass()).and_then(|b| b.counts).filter(|c| c.total > 0);
    let now = v.counts.filter(|c| c.total > 0);
    let headline = match (before, now) {
        (Some(b), Some(n)) => {
            format!("Tests still fail ({} of {}, was {} of {})", n.passed, n.total, b.passed, b.total)
        }
        _ => format!("Tests fail: {} pass", v.short()),
    };
    call(Class::NeedsLook, headline, Some(REASON_TESTS))
}

fn single_call(task: &Task, last: &Attempt, f: &Facts) -> Call {
    match &last.end {
        Some(AttemptEnd::Completed) => match &f.changes {
            None => call(Class::NeedsLook, "Couldn't read the changes", Some(REASON_OPEN_COPY)),
            Some(c) if c.files_changed == 0 => call(Class::NoChanges, MADE_NO_CHANGES, None),
            Some(_) => call(Class::NoChecks, DONE_NO_CHECKS, None),
        },
        _ => from_end(task, last, f, None),
    }
}

/// The result when the last try did not complete: gate, question, usage, failure, cut off, stop.
fn from_end(task: &Task, last: &Attempt, f: &Facts, m: Option<&MissionFacts>) -> Call {
    let verdict = m.and_then(|m| m.verdict.as_ref());
    match &last.end {
        Some(AttemptEnd::OutOfUsage { .. }) => {
            let tries = task.attempts.iter().filter(|a| matches!(a.end, Some(AttemptEnd::OutOfUsage { .. }))).count();
            let headline = if tries >= 2 { "Ran out of usage twice" } else { "Ran out of usage" };
            call(Class::NeedsLook, headline, Some(REASON_USAGE))
        }
        Some(AttemptEnd::Gate { code, sentence }) => match gate_sentence(code, &last.label) {
            Some(text) if code == "AGENT_NOT_INSTALLED" => {
                call(Class::DidntRun, format!("Didn't run: {}", text.trim_end_matches('.')), None)
            }
            Some(text) => call(Class::NeedsLook, text, Some(REASON_OPEN_TERMINAL)),
            None => call(Class::NeedsLook, non_empty(sentence, code), Some(REASON_OPEN_TERMINAL)),
        },
        Some(AttemptEnd::Asked { .. }) => call(Class::NeedsLook, ASKED, Some(REASON_OPEN_TERMINAL)),
        Some(AttemptEnd::NeedsYou { why, sentence }) => {
            call(Class::NeedsLook, non_empty(sentence, why), Some(REASON_LOOK))
        }
        Some(AttemptEnd::Failed { code, sentence }) => {
            call(Class::NeedsLook, non_empty(sentence, code), Some(REASON_FAILED))
        }
        Some(AttemptEnd::CutOff) => {
            let head = format!("Cut off at {} while working", f.stop_hhmm);
            let headline = match verdict {
                Some(v) => format!("{head}. Last check: {} pass", v.short()),
                None => format!("{head}."),
            };
            call(Class::NeedsLook, headline, Some(REASON_RUN_AGAIN))
        }
        Some(AttemptEnd::StoppedByPerson) => call(Class::NeedsLook, "Stopped by you", Some(REASON_RUN_AGAIN)),
        Some(AttemptEnd::Completed) | None => match (m, verdict) {
            (Some(m), Some(v)) if !v.all_pass() => tests_fail_call(v, m.baseline.as_ref()),
            (Some(_), _) => call(Class::NeedsLook, "Stopped before the tests passed", Some(REASON_LOOK)),
            (None, _) => call(Class::NeedsLook, "Stopped before it finished", Some(REASON_LOOK)),
        },
    }
}

fn non_empty(sentence: &str, fallback: &str) -> String {
    if sentence.trim().is_empty() {
        fallback.to_string()
    } else {
        sentence.to_string()
    }
}

/// Fills the fields every class shares. Test counts come from the mission's latest check.
fn assemble(task: &Task, f: &Facts, c: Call) -> Outcome {
    let m = f.mission.as_ref();
    let verdict = m.and_then(|m| m.verdict.as_ref());
    let tests = verdict.and_then(|v| v.counts).filter(|c| c.total > 0).map(|c| Counts {
        passed: c.passed,
        total: c.total,
    });
    let before = m
        .and_then(|m| m.baseline.as_ref())
        .filter(|b| !b.all_pass())
        .and_then(|b| b.counts)
        .filter(|c| c.total > 0)
        .map(|c| Counts { passed: c.passed, total: c.total });
    let blocking = verdict.map(|v| v.test_changes.blocking_sentences()).unwrap_or_default();
    let turns = m.map(|m| m.turns_used).unwrap_or(task.attempts.len() as u32);
    let ms: u64 = task
        .attempts
        .iter()
        .map(|a| a.ended_ms.unwrap_or(a.started_ms).saturating_sub(a.started_ms))
        .sum();
    Outcome {
        class: c.class,
        headline: c.headline,
        reason: c.reason.map(str::to_string),
        tests,
        tests_before: before,
        checks_declared: !task.checks.is_empty(),
        test_change_lines: test_change_lines(&blocking),
        files: f.changes.as_ref().map(|c| (c.files_changed as u32, c.additions, c.deletions)),
        turns,
        minutes: (ms / MS_PER_MIN) as u32,
        summaries: f.summaries.iter().take(MAX_SUMMARIES).cloned().collect(),
        copy: f.copy.clone(),
    }
}

/// Up to three blocking sentences, then "and N more changes" (as `missions::cards::tamper_lines`).
fn test_change_lines(blocking: &[String]) -> Vec<String> {
    let mut lines: Vec<String> = blocking.iter().take(MAX_TEST_LINES).cloned().collect();
    match blocking.len().saturating_sub(MAX_TEST_LINES) {
        0 => {}
        1 => lines.push("and 1 more change".to_string()),
        rest => lines.push(format!("and {rest} more changes")),
    }
    lines
}

#[cfg(test)]
#[path = "classify_tests.rs"]
mod tests;
