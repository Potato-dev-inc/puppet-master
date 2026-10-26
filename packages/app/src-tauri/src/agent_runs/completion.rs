//! Deterministic turn-completion for TUI workers.
//!
//! A fake backend drives `observe_tui_turn` with pane status + screen snapshots.
//! Live supervision is a thin loop over the same function. Reply-text cleaning is in `clean`.

mod clean;
mod links;

use crate::operations::ResultCapture;
use clean::normalize_echo;
pub use clean::{clean_extracted_result, clean_extracted_result_after, clean_extracted_result_in_turn, tui_turn_evidence};
pub use links::restore_link_paths;

pub const STABLE_IDLE_TICKS: u8 = 2;
pub const RECONCILE_IDLE_TICKS: u8 = 4;
/// A turn that was seen running but has shown no reply text yet gets this many idle ticks (250ms
/// each) before it is declared ambiguous. The TUI can look idle (echoed prompt, input footer) for
/// seconds between submitting and the first token, so a 1s window ended turns before the reply.
pub const AWAITING_REPLY_TICKS: u8 = 40;
/// The same wait for a turn that stopped on a question (a permission or trust menu) and went on after it was answered:
/// the agent runs its tool in silence and draws no reply for a while (a command that takes half a minute), so that turn
/// is not declared ambiguous on the short wait.
pub const AFTER_QUESTION_TICKS: u8 = 200;
/// A turn that never showed activity gets this many idle ticks (250ms each) before it is declared
/// not submitted. Cursor can take several seconds to start reacting, and its screen lags the PTY.
pub const NOT_SUBMITTED_TICKS: u8 = 24;
/// Halfway to that, Enter is sent once more in case the first one was dropped.
pub const RETRY_SUBMIT_TICKS: u8 = 12;

#[derive(Debug, Clone, Default)]
pub struct TuiTurnWatch {
    pub baseline: String,
    pub task: String,
    /// The previous turn's prompt on this pane. Its echo is still on screen (and a failed delivery
    /// can leave a stale copy in the input box), so it is never evidence for this turn.
    pub previous_task: String,
    idle_evidence: Option<String>,
    idle_ticks: u8,
    reconcile_ticks: u8,
    saw_activity: bool,
    saw_task_echo: bool,
    /// Reply text (after cleaning) has been on screen at some point this turn.
    saw_reply: bool,
    /// The pane stopped on a question this turn (an approval menu, a trust question): it was answered, and the work it
    /// allowed has to be waited for.
    asked_question: bool,
    submit_retried: bool,
    /// The delivery protocol already verified the prompt was typed, submitted and the turn
    /// started (`TuiProfile::verifies_prompt_delivery`), so no blind second Enter is ever sent.
    delivery_verified: bool,
}

impl TuiTurnWatch {
    pub fn from_baseline(baseline: impl Into<String>) -> Self {
        Self {
            baseline: baseline.into(),
            ..Self::default()
        }
    }

    pub fn with_task(mut self, task: impl Into<String>) -> Self {
        self.task = task.into();
        self
    }

    pub fn with_verified_delivery(mut self, verified: bool) -> Self {
        self.delivery_verified = verified;
        self
    }

    pub fn set_delivery_verified(&mut self, verified: bool) {
        self.delivery_verified = verified;
    }

    pub fn with_previous_task(mut self, previous: impl Into<String>) -> Self {
        self.previous_task = previous.into();
        self
    }

    /// Whether the task's echo has been seen since the baseline (the turn's output has begun).
    pub fn saw_task_echo(&self) -> bool {
        self.saw_task_echo
    }

    /// Start watching again from `baseline` after the prompt was delivered a second time.
    pub fn restart(&mut self, baseline: impl Into<String>) {
        self.baseline = baseline.into();
        self.idle_evidence = None;
        self.idle_ticks = 0;
        self.reconcile_ticks = 0;
        self.saw_activity = false;
        self.saw_task_echo = false;
        self.saw_reply = false;
        self.asked_question = false;
        self.submit_retried = false;
    }
}

/// The model to record for a completed pane run: the TUI footer when it is on screen (confirmed),
/// else the CLI's configured default (not a confirmation). `None` only when neither is known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionModel {
    pub model: String,
    pub confirmed: bool,
}

pub fn completion_model(
    agent_type: &str,
    screen: &str,
    already_resolved: Option<&str>,
    default_model: impl FnOnce() -> Option<String>,
) -> Option<CompletionModel> {
    let profile = crate::tui::profile_for(agent_type);
    if let Some(model) = profile.detect_footer_model(screen) {
        return Some(CompletionModel {
            model,
            confirmed: true,
        });
    }
    if already_resolved.is_some_and(|model| !model.trim().is_empty()) {
        return None;
    }
    if !profile.probes_default_model() {
        return None;
    }
    default_model().map(|model| CompletionModel {
        model,
        confirmed: false,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TuiTurnDecision {
    Continue,
    WaitingInput,
    Complete {
        result: String,
        capture: ResultCapture,
    },
    AmbiguousIdle,
    /// The pane has not reacted yet: press Enter once more (typed text is waiting in the input).
    RetrySubmit,
    /// The pane never left idle and showed no new output: the prompt was not submitted.
    NotSubmitted,
}

pub fn observe_tui_turn(
    watch: &mut TuiTurnWatch,
    pane_status: &str,
    screen: &str,
    has_prompt: bool,
    agent_type: &str,
) -> TuiTurnDecision {
    // A startup screen (an update notice, a trust question, a login page) is never a turn's answer.
    // The pane reads "running" while the task is typed into it and "idle" once it settles, which
    // looked exactly like a finished turn: a Codex run completed with its update menu as the reply.
    // The turn cannot have ended while the TUI has not even reached its input box.
    let startup_gate = crate::tui::profile_for(agent_type).startup_gate(screen).is_some();
    if has_prompt || startup_gate || pane_status == "waiting_input" {
        watch.asked_question |= has_prompt || pane_status == "waiting_input";
        watch.idle_evidence = None;
        watch.idle_ticks = 0;
        watch.reconcile_ticks = 0;
        return TuiTurnDecision::WaitingInput;
    }
    if pane_status != "idle" {
        watch.saw_activity = true;
        watch.idle_evidence = None;
        watch.idle_ticks = 0;
        watch.reconcile_ticks = 0;
        return TuiTurnDecision::Continue;
    }
    if !watch.saw_task_echo && task_echo_on_screen(screen, &watch.baseline, &watch.task) {
        watch.saw_task_echo = true;
    }
    // With a blank baseline (the snapshot raced a lagging screen) everything already on screen,
    // banner, model footer, update notices, counts as "new". Until the pane was seen running or
    // the task's own echo appeared, there is no proof this turn started, so accept no evidence.
    // With a task to look for, a non-blank baseline is not proof either: whatever else changed on
    // screen (a wrapped footer row, an update notice) is not a reply. Only a task-less watch,
    // which has nothing to echo, falls back to the baseline alone.
    let turn_started = watch.saw_activity
        || watch.saw_task_echo
        || (watch.task.trim().is_empty() && !watch.baseline.trim().is_empty());
    let evidence = turn_started.then(|| turn_reply(screen, watch, agent_type)).flatten();
    if let Some(evidence) = evidence {
        watch.saw_reply = true;
        if watch.idle_evidence.as_deref() == Some(evidence.as_str()) {
            watch.idle_ticks = watch.idle_ticks.saturating_add(1);
        } else {
            watch.idle_evidence = Some(evidence);
            watch.idle_ticks = 1;
        }
        watch.reconcile_ticks = 0;
        if watch.idle_ticks >= STABLE_IDLE_TICKS {
            if let Some(result) = watch.idle_evidence.take() {
                return TuiTurnDecision::Complete {
                    result,
                    capture: ResultCapture::Inferred,
                };
            }
        }
        return TuiTurnDecision::Continue;
    }
    watch.idle_evidence = None;
    watch.idle_ticks = 0;
    watch.reconcile_ticks = watch.reconcile_ticks.saturating_add(1);
    if watch.saw_activity {
        let patience = if watch.saw_reply {
            RECONCILE_IDLE_TICKS
        } else if watch.asked_question {
            AFTER_QUESTION_TICKS
        } else {
            AWAITING_REPLY_TICKS
        };
        return if watch.reconcile_ticks >= patience {
            // The screen would not say whether the turn ended: one hint that it may have changed.
            crate::canary::note_signal(agent_type, crate::canary::Signal::AmbiguousIdle);
            TuiTurnDecision::AmbiguousIdle
        } else {
            TuiTurnDecision::Continue
        };
    }
    if watch.reconcile_ticks >= NOT_SUBMITTED_TICKS {
        TuiTurnDecision::NotSubmitted
    } else if watch.reconcile_ticks >= RETRY_SUBMIT_TICKS
        && !watch.submit_retried
        && !watch.delivery_verified
    {
        watch.submit_retried = true;
        TuiTurnDecision::RetrySubmit
    } else {
        TuiTurnDecision::Continue
    }
}

/// What this turn's reply is on `screen`.
///
/// The turn begins at the echo of the task THIS turn delivered, so the reply is what follows that echo, cleaned
/// by the profile's chrome, thinking and tool-activity tables: the banner above the echo, the previous turn's
/// text and the stale status rows are before it and can never be part of the reply. (The old rule, "whatever is
/// new since the baseline", counted all of them as new whenever the baseline held only the last few rows of a
/// busier screen, and counted every changing `Thinking... 4s` row as new text.) Only when the echo is not on the
/// screen does the baseline decide: a task the agent never echoes is measured against the baseline, and a task
/// whose echo scrolled off the top (seen earlier this turn) leaves the whole screen to the turn.
fn turn_reply(screen: &str, watch: &TuiTurnWatch, agent_type: &str) -> Option<String> {
    match reply_after_echo(screen, &watch.task, &watch.previous_task, agent_type) {
        EchoSlice::Found(reply) => return reply,
        EchoSlice::NoEcho => {}
    }
    let baseline = if watch.saw_task_echo && !same_task(&watch.task, &watch.previous_task) {
        ""
    } else {
        watch.baseline.as_str()
    };
    tui_turn_evidence(screen, baseline, agent_type)
        .and_then(|raw| clean_extracted_result_after(&raw, &watch.task, &watch.previous_task, agent_type))
}

/// The same instruction sent twice in a row: its echo on screen could be the earlier turn's.
fn same_task(task: &str, previous: &str) -> bool {
    let (a, b) = (normalize_echo(task), normalize_echo(previous));
    !a.is_empty() && a == b
}

/// The outcome of looking for this turn's task echo on a screen.
#[derive(Debug, PartialEq, Eq)]
enum EchoSlice {
    /// The screen shows no echo of the task (or the task is too short to look for, or it repeats the previous
    /// turn's, whose echo cannot be told apart).
    NoEcho,
    /// The echo is there; this is the cleaned reply after it (`None`: nothing but chrome follows).
    Found(Option<String>),
}

fn reply_after_echo(screen: &str, task: &str, previous_task: &str, agent_type: &str) -> EchoSlice {
    if same_task(task, previous_task) {
        return EchoSlice::NoEcho;
    }
    match echo_position(screen, task) {
        Some(at) => {
            let lines: Vec<&str> = screen.lines().collect();
            let turn = lines[at..].join("
");
            EchoSlice::Found(reply_in_turn(&turn, task, previous_task, agent_type))
        }
        None => EchoSlice::NoEcho,
    }
}

/// The index of the line where the LAST echo of `task` starts in `text`, if it is there.
fn echo_position(text: &str, task: &str) -> Option<usize> {
    let task_n = normalize_echo(task);
    let head: String = task_n.chars().take(40).collect();
    let head = head.trim_end();
    if head.len() < 8 {
        return None;
    }
    let lines: Vec<&str> = text.lines().collect();
    (0..lines.len()).rev().find(|&index| {
        let own = normalize_echo(lines[index]);
        if own.is_empty() || !(head.starts_with(own.as_str()) || own.starts_with(head)) {
            return false;
        }
        own.len() >= 8 && {
            let span = lines[index..lines.len().min(index + 6)].join(" ");
            normalize_echo(&span).contains(head)
        }
    })
}

/// Whether the start of the task text is visible on screen (the TUI echoes the submitted prompt).
/// Whitespace and punctuation are ignored so a wrapped line still matches.
fn task_echo_on_screen(screen: &str, baseline: &str, task: &str) -> bool {
    let task_n = normalize_echo(task);
    let head: String = task_n.chars().take(40).collect();
    let screen_n = normalize_echo(screen);
    if head.len() >= 8 {
        return screen_n.contains(head.trim_end());
    }
    // Too short to be distinctive on its own: its echo must be new since the baseline.
    !head.is_empty()
        && screen_n.matches(head.as_str()).count()
            > normalize_echo(baseline).matches(head.as_str()).count()
}

/// A captured reply, and whether it is known to be only the tail of the real one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WidenedReply {
    pub text: String,
    /// The start of the turn could not be found, so lines above the visible rows may be missing.
    pub truncated: bool,
}

/// `result` was read from the visible rows. When this turn's prompt echo is not on them, the
/// reply scrolled past the top and `result` is only its tail: re-read the turn from `full` (the
/// scrollback plus the screen), starting at the last echo of the prompt. When even `full` does not
/// hold the echo (alternate screen, history too short) the result is flagged `truncated` rather
/// than passed off as whole.
pub fn widen_reply(
    visible: &str,
    full: &str,
    task: &str,
    previous_task: &str,
    result: &str,
    agent_type: &str,
) -> WidenedReply {
    widen_reply_anchored(visible, full, task, previous_task, result, agent_type, None)
}

/// [`widen_reply`], with a second way to find where the turn begins when its task's echo is nowhere in `full`
/// (an agent that does not echo the task, or echoes it in a form nobody can match): `since_turn` is the text of the
/// pane that is new since the moment before the task was typed (`pty::pane_facts::rows_since_turn_mark`). The reply
/// is that text, cleaned, when it holds more than what the visible rows gave.
pub fn widen_reply_anchored(
    visible: &str,
    full: &str,
    task: &str,
    previous_task: &str,
    result: &str,
    agent_type: &str,
    since_turn: Option<&str>,
) -> WidenedReply {
    let unchanged = |truncated| WidenedReply {
        text: result.to_string(),
        truncated,
    };
    let task_n = normalize_echo(task);
    let head: String = task_n.chars().take(40).collect();
    let head = head.trim_end();
    if head.len() < 8 || normalize_echo(visible).contains(head) {
        return unchanged(false);
    }
    if let Some(text) = scrollback_reply(full, task, previous_task, agent_type) {
        return WidenedReply { text, truncated: false };
    }
    // The task's echo is nowhere: the pane's own text since just before the task was typed is the turn.
    let anchored = since_turn
        .and_then(|rows| tui_turn_evidence(rows, "", agent_type))
        .and_then(|raw| clean_extracted_result_after(&raw, task, previous_task, agent_type))
        .filter(|text| text.lines().count() > result.lines().count());
    match anchored {
        Some(text) => WidenedReply { text, truncated: false },
        None => unchanged(true),
    }
}

/// The reply to `task` read from `full` (scrollback plus screen): what follows the last echo of
/// the prompt, cleaned. `None` when the echo is not there or nothing but chrome follows it, so a
/// reply is never attributed to a turn on the strength of an older turn's text.
pub fn scrollback_reply(
    full: &str,
    task: &str,
    previous_task: &str,
    agent_type: &str,
) -> Option<String> {
    let at = echo_position(full, task)?;
    let lines: Vec<&str> = full.lines().collect();
    let turn = lines[at..].join("
");
    reply_in_turn(&turn, task, previous_task, agent_type)
}

/// The reply in `turn`, the rows from this turn's task echo on. When the agent printed tool rows under the echo, what is
/// read is what follows the last of them, so the echo is above the text that is cleaned (`echo_above`).
fn reply_in_turn(turn: &str, task: &str, previous_task: &str, agent_type: &str) -> Option<String> {
    let tools_after_echo = turn
        .lines()
        .skip(1)
        .any(|line| crate::tui::is_tool_activity_line(&line.trim().to_ascii_lowercase(), agent_type));
    tui_turn_evidence(turn, "", agent_type)
        .and_then(|raw| clean_extracted_result_in_turn(&raw, task, previous_task, agent_type, tools_after_echo))
}

/// A last attempt to read this turn's reply before it is declared not captured: the visible rows
/// again (the screen may have finished painting), then the whole history.
pub fn recapture_reply(
    watch: &TuiTurnWatch,
    visible: &str,
    full: &str,
    agent_type: &str,
) -> Option<String> {
    turn_reply(visible, watch, agent_type)
        .or_else(|| scrollback_reply(full, &watch.task, &watch.previous_task, agent_type))
}

impl TuiTurnWatch {
    /// What the watch saw, attached to a not-captured failure so the next occurrence is diagnosable.
    pub fn diagnostics(&self) -> serde_json::Value {
        let tail = |text: &str| {
            let lines: Vec<&str> = text.lines().collect();
            lines[lines.len().saturating_sub(40)..].join(
                "
",
            )
        };
        serde_json::json!({
            "saw_activity": self.saw_activity,
            "saw_task_echo": self.saw_task_echo,
            "saw_reply": self.saw_reply,
            "asked_question": self.asked_question,
            "delivery_verified": self.delivery_verified,
            "reconcile_ticks": self.reconcile_ticks,
            "task_head": self.task.chars().take(80).collect::<String>(),
            "baseline_tail": tail(&self.baseline),
        })
    }
}

#[cfg(test)]
mod anchored_tests {
    use super::*;

    const AGENT: &str = "no-such-agent-for-the-anchor-tests";

    fn rows(n: usize) -> String {
        (1..=n).map(|i| format!("row {i} is here")).collect::<Vec<_>>().join("\n")
    }

    /// An agent that does not echo the task: nothing on the screen or in the scrollback says where the turn began, and
    /// the visible rows are only the tail of a reply taller than the pane.
    #[test]
    fn without_an_echo_the_text_since_the_turn_began_is_the_reply() {
        let visible = rows(50)[rows(50).find("row 31 ").unwrap()..].to_string();
        let full = format!("earlier note\n{}", rows(50));
        let tail = clean_extracted_result("row 31 is here\nrow 32 is here", "", AGENT).unwrap();
        let widened = widen_reply_anchored(&visible, &full, "Write the long answer please", "", &tail, AGENT, Some(&rows(50)));
        assert!(!widened.truncated);
        assert_eq!(widened.text.lines().count(), 50);
        assert!(widened.text.starts_with("row 1 is here") && widened.text.ends_with("row 50 is here"), "{}", widened.text);
        // no anchor: the old behaviour, the tail, flagged as truncated
        let plain = widen_reply_anchored(&visible, &full, "Write the long answer please", "", &tail, AGENT, None);
        assert!(plain.truncated);
        assert_eq!(plain.text, tail);
        // an anchor that holds no more than what the visible rows gave changes nothing
        let same = widen_reply_anchored(&visible, &full, "Write the long answer please", "", &tail, AGENT, Some("row 31 is here\nrow 32 is here"));
        assert!(same.truncated && same.text == tail);
    }

    #[test]
    fn the_echo_still_wins_when_it_is_in_the_scrollback() {
        let full = format!("old\n> Write the long answer please\n{}", rows(60));
        let tail = "row 41 is here".to_string();
        let widened = widen_reply_anchored("row 41 is here", &full, "Write the long answer please", "", &tail, AGENT, Some("a different anchor"));
        assert!(!widened.truncated);
        assert_eq!(widened.text.lines().count(), 60, "{}", widened.text);
    }
}
