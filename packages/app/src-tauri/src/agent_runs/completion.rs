//! Deterministic turn-completion for TUI workers.
//!
//! A fake backend drives `observe_tui_turn` with pane status + screen snapshots.
//! Live supervision is a thin loop over the same function.

use crate::operations::ResultCapture;
use std::collections::HashMap;

pub const STABLE_IDLE_TICKS: u8 = 2;
pub const RECONCILE_IDLE_TICKS: u8 = 4;
/// A turn that never showed activity gets this many idle ticks (250ms each) before it is declared
/// not submitted. Cursor can take several seconds to start reacting, and its screen lags the PTY.
pub const NOT_SUBMITTED_TICKS: u8 = 24;
/// Halfway to that, Enter is sent once more in case the first one was dropped.
pub const RETRY_SUBMIT_TICKS: u8 = 12;

#[derive(Debug, Clone, Default)]
pub struct TuiTurnWatch {
    pub baseline: String,
    pub task: String,
    idle_evidence: Option<String>,
    idle_ticks: u8,
    reconcile_ticks: u8,
    saw_activity: bool,
    submit_retried: bool,
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
) -> TuiTurnDecision {
    if has_prompt || pane_status == "waiting_input" {
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
    if let Some(evidence) = tui_turn_evidence(screen, &watch.baseline)
        .and_then(|raw| clean_extracted_result(&raw, &watch.task))
    {
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
        return if watch.reconcile_ticks >= RECONCILE_IDLE_TICKS {
            TuiTurnDecision::AmbiguousIdle
        } else {
            TuiTurnDecision::Continue
        };
    }
    if watch.reconcile_ticks >= NOT_SUBMITTED_TICKS {
        TuiTurnDecision::NotSubmitted
    } else if watch.reconcile_ticks >= RETRY_SUBMIT_TICKS && !watch.submit_retried {
        watch.submit_retried = true;
        TuiTurnDecision::RetrySubmit
    } else {
        TuiTurnDecision::Continue
    }
}

pub fn is_tui_chrome_line(line: &str) -> bool {
    let lower = line.trim().to_ascii_lowercase();
    lower.is_empty()
        || lower.contains("add follow-up")
        || lower.contains("add a follow-up")
        || lower == "composing"
        || lower == "working"
        || lower.starts_with("working...")
        || lower.contains("allow once")
        || lower.contains("yes, allow")
        || lower.contains("don't ask again")
        || lower.contains("[pasted text")
        || lower.starts_with("pasted text")
        || lower.starts_with("acceptance criteria")
        || lower.starts_with("expected output:")
        || lower.starts_with("# context packet")
        || lower.starts_with("task:")
        || lower.starts_with("scope:")
        || lower.starts_with("constraints:")
        || lower.starts_with("facts:")
        || is_tui_status_line(&lower)
        || is_tui_header_line(&lower)
        || is_cursor_tool_line(&lower)
        || is_cursor_idle_chrome(&lower)
        || lower.chars().all(|ch| !ch.is_alphanumeric())
}

/// What an idle, untouched Cursor pane shows: the input placeholder (`→ Plan, search, build
/// anything`) and a bare model footer (`Composer 2.5`, before any context usage is printed).
fn is_cursor_idle_chrome(lower: &str) -> bool {
    let text = lower.trim_start_matches(['→', '>', ' ']);
    if text.starts_with("plan, search, build anything") {
        return true;
    }
    text.strip_prefix("composer ").is_some_and(|version| {
        !version.is_empty()
            && version.len() <= 8
            && version.chars().all(|ch| ch.is_ascii_digit() || ch == '.')
    })
}

/// Cursor's tool-activity rows shown above a reply: the collapsed-output notice
/// (`… 2 output lines hidden · ctrl+o to expand`), a shell header (`$ cd … 11s`) and a one-file
/// read (`Read todo.py`). None of them is part of the reply text.
fn is_cursor_tool_line(lower: &str) -> bool {
    if lower.contains("ctrl+o to expand")
        || (lower.contains("output line") && lower.contains("hidden"))
    {
        return true;
    }
    if let Some(command) = lower.strip_prefix("$ ") {
        let last = command.split_whitespace().last().unwrap_or("");
        let is_duration = last.strip_suffix('s').is_some_and(|digits| {
            !digits.is_empty() && digits.chars().all(|ch| ch.is_ascii_digit() || ch == '.')
        });
        if is_duration {
            return true;
        }
    }
    lower.strip_prefix("read ").is_some_and(|target| {
        !target.contains(' ')
            && target.chars().any(|ch| matches!(ch, '.' | '/' | '\\'))
            && !target.ends_with('.')
    })
}

/// Cursor's startup banner and footer rows: `Cursor Agent`, `v2026.10.01-e373342`, `Tip: ...`,
/// and the `~\folder` working-directory line. None of them is ever a reply.
fn is_tui_header_line(lower: &str) -> bool {
    let is_version = lower.strip_prefix('v').is_some_and(|rest| {
        rest.len() >= 8
            && rest.as_bytes()[..4].iter().all(u8::is_ascii_digit)
            && rest.as_bytes()[4] == b'.'
    });
    lower == "cursor agent"
        || lower.starts_with("cursor agent v")
        || is_version
        || lower.starts_with("tip:")
        || ((lower.starts_with("~\\") || lower.starts_with("~/")) && !lower.contains(' '))
}

/// Footer such as `Composer 2.5 · 7.2%` (model name and context usage).
fn is_tui_status_line(lower: &str) -> bool {
    lower.len() <= 80 && lower.contains(" · ") && lower.ends_with('%')
}

/// Drop Cursor paste markers, prompt echoes, and acceptance tables so they
/// cannot be stored as an assistant result or reused as follow-up context.
pub fn clean_extracted_result(evidence: &str, task: &str) -> Option<String> {
    let all: Vec<&str> = evidence.lines().map(str::trim).collect();
    // The reply comes after the agent's tool calls, so everything up to the last tool-activity
    // marker (shell header, hidden-output notice, file read) is tool output, not the answer.
    let after_tools = all
        .iter()
        .rposition(|line| is_cursor_tool_line(&line.to_ascii_lowercase()))
        .map_or(0, |index| index + 1);
    let mut lines: Vec<&str> = all[after_tools..]
        .iter()
        .copied()
        .filter(|line| !is_tui_chrome_line(line) && !line_echoes_task(line, task))
        .collect();
    lines.dedup();
    if lines.is_empty() {
        return None;
    }
    let cleaned = lines.join("\n");
    if is_mostly_prompt_echo(&cleaned, task) {
        None
    } else {
        Some(cleaned)
    }
}

fn normalize_echo(text: &str) -> String {
    text.chars()
        .filter(|ch| ch.is_alphanumeric() || ch.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn line_echoes_task(line: &str, task: &str) -> bool {
    if task.is_empty() {
        return false;
    }
    let trimmed = line.trim();
    let is_bullet = trimmed.starts_with(['-', '*', '•']);
    let stripped = trimmed.trim_start_matches(['-', '*', '•', '>']).trim();
    if stripped.len() < 8 {
        return false;
    }
    let task_n = normalize_echo(task);
    let line_n = normalize_echo(stripped);
    if line_n.is_empty() || !task_n.contains(&line_n) {
        return false;
    }
    is_bullet
        || stripped.len() >= 24
        || task.lines().any(|task_line| normalize_echo(task_line) == line_n)
}

fn is_mostly_prompt_echo(cleaned: &str, task: &str) -> bool {
    if task.is_empty() {
        return false;
    }
    let task_n = normalize_echo(task);
    let out_n = normalize_echo(cleaned);
    if out_n.is_empty() {
        return true;
    }
    if out_n.len() >= 40 && task_n.contains(&out_n) {
        return true;
    }
    let content_lines = cleaned
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    if content_lines.len() < 3 {
        return false;
    }
    let echoed = content_lines
        .iter()
        .filter(|line| line_echoes_task(line, task) || is_tui_chrome_line(line))
        .count();
    echoed * 2 >= content_lines.len()
}

/// Cursor prints a tool's output indented deeper than the tool line itself (`$ cmd 14s`, then the
/// output at +2 columns), and the agent's reply back at the tool line's column. Drop every line
/// indented deeper than a tool marker, up to the next blank or shallower line. Indentation is
/// the only thing that separates the two, so this must run before lines are trimmed.
pub fn strip_tool_output_blocks(text: &str) -> String {
    let indent = |line: &str| line.len() - line.trim_start().len();
    let mut out: Vec<&str> = Vec::new();
    let mut block_indent: Option<usize> = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(marker_indent) = block_indent {
            if !trimmed.is_empty() && indent(line) > marker_indent {
                continue;
            }
            block_indent = None;
        }
        if is_cursor_tool_line(&trimmed.to_ascii_lowercase()) {
            block_indent = Some(indent(line));
        }
        out.push(line);
    }
    out.join("\n")
}

/// New non-chrome TUI text since `baseline`. Occurrence counts so a repeated
/// identical answer still counts as this turn's result.
pub fn tui_turn_evidence(screen: &str, baseline: &str) -> Option<String> {
    if screen == baseline {
        return None;
    }
    let (screen, baseline) = (strip_tool_output_blocks(screen), strip_tool_output_blocks(baseline));
    let (screen, baseline) = (screen.as_str(), baseline.as_str());
    if screen == baseline {
        return None;
    }
    if let Some(suffix) = screen.strip_prefix(baseline) {
        let added = suffix
            .lines()
            .map(str::trim)
            .filter(|line| !is_tui_chrome_line(line))
            .collect::<Vec<_>>();
        if !added.is_empty() {
            return Some(added.join("\n"));
        }
    }
    let mut before: HashMap<&str, usize> = HashMap::new();
    for line in baseline.lines().map(str::trim).filter(|line| !line.is_empty()) {
        *before.entry(line).or_insert(0) += 1;
    }
    let mut seen: HashMap<&str, usize> = HashMap::new();
    let mut added = Vec::new();
    for line in screen.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if is_tui_chrome_line(line) {
            continue;
        }
        let count = seen.entry(line).or_insert(0);
        *count += 1;
        if *count > *before.get(line).unwrap_or(&0) {
            added.push(line);
        }
    }
    if added.is_empty() {
        None
    } else {
        Some(added.join("\n"))
    }
}
