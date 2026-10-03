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
    /// The previous turn's prompt on this pane. Its echo is still on screen (and a failed delivery
    /// can leave a stale copy in the input box), so it is never evidence for this turn.
    pub previous_task: String,
    idle_evidence: Option<String>,
    idle_ticks: u8,
    reconcile_ticks: u8,
    saw_activity: bool,
    saw_task_echo: bool,
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

    pub fn with_previous_task(mut self, previous: impl Into<String>) -> Self {
        self.previous_task = previous.into();
        self
    }

    /// Start watching again from `baseline` after the prompt was delivered a second time.
    pub fn restart(&mut self, baseline: impl Into<String>) {
        self.baseline = baseline.into();
        self.idle_evidence = None;
        self.idle_ticks = 0;
        self.reconcile_ticks = 0;
        self.saw_activity = false;
        self.saw_task_echo = false;
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
    if !watch.saw_task_echo && task_echo_on_screen(screen, &watch.task) {
        watch.saw_task_echo = true;
    }
    // With a blank baseline (the snapshot raced a lagging screen) everything already on screen,
    // banner, model footer, update notices, counts as "new". Until the pane was seen running or
    // the task's own echo appeared, there is no proof this turn started, so accept no evidence.
    let turn_started = !watch.baseline.trim().is_empty() || watch.saw_activity || watch.saw_task_echo;
    let evidence = turn_started
        .then(|| tui_turn_evidence(screen, &watch.baseline))
        .flatten()
        .and_then(|raw| clean_extracted_result_after(&raw, &watch.task, &watch.previous_task));
    if let Some(evidence) = evidence {
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

/// Whether the start of the task text is visible on screen (the TUI echoes the submitted prompt).
/// Whitespace and punctuation are ignored so a wrapped line still matches.
fn task_echo_on_screen(screen: &str, task: &str) -> bool {
    let task_n = normalize_echo(task);
    let head: String = task_n.chars().take(40).collect();
    head.len() >= 8 && normalize_echo(screen).contains(head.trim_end())
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
        || crate::tui::is_chrome_line(&lower)
        || lower.chars().all(|ch| !ch.is_alphanumeric())
}

/// Drop Cursor paste markers, prompt echoes, and acceptance tables so they
/// cannot be stored as an assistant result or reused as follow-up context.
pub fn clean_extracted_result(evidence: &str, task: &str) -> Option<String> {
    clean_extracted_result_after(evidence, task, "")
}

/// As [`clean_extracted_result`], also dropping echoes of the previous turn's prompt.
pub fn clean_extracted_result_after(
    evidence: &str,
    task: &str,
    previous_task: &str,
) -> Option<String> {
    let corpus = if previous_task.trim().is_empty() {
        task.to_string()
    } else {
        format!("{task}\n{previous_task}")
    };
    let task = corpus.as_str();
    let all: Vec<&str> = evidence.lines().map(str::trim).collect();
    // The reply comes after the agent's tool calls, so everything up to the last tool-activity
    // marker (shell header, hidden-output notice, file read) is tool output, not the answer.
    let after_tools = all
        .iter()
        .rposition(|line| crate::tui::is_tool_activity_line(&line.to_ascii_lowercase()))
        .map_or(0, |index| index + 1);
    let candidates: Vec<&str> = all[after_tools..]
        .iter()
        .copied()
        .filter(|line| !is_tui_chrome_line(line))
        .collect();
    let echoed = echoed_lines(&candidates, task);
    let mut lines: Vec<&str> = candidates
        .iter()
        .zip(&echoed)
        .filter(|(_, echoed)| !**echoed)
        .map(|(line, _)| *line)
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

/// Which lines echo the task. A line that is a whole sentence of the prompt echoes it outright;
/// a short wrapped fragment (the tail of a prompt line the TUI broke at the box width) counts
/// when it sits next to an echoed line, so `and propose a fix` cannot survive as a reply.
fn echoed_lines(lines: &[&str], task: &str) -> Vec<bool> {
    let mut echoed: Vec<bool> = lines
        .iter()
        .map(|line| line_echoes_task(line, task))
        .collect();
    if task.is_empty() {
        return echoed;
    }
    let task_n = normalize_echo(task);
    loop {
        let mut changed = false;
        for index in 0..lines.len() {
            if echoed[index] {
                continue;
            }
            let neighbour =
                (index > 0 && echoed[index - 1]) || echoed.get(index + 1) == Some(&true);
            if !neighbour {
                continue;
            }
            let stripped = lines[index].trim_start_matches(['-', '*', '•', '>']).trim();
            let line_n = normalize_echo(stripped);
            if line_n.len() >= 12 && task_n.contains(&line_n) {
                echoed[index] = true;
                changed = true;
            }
        }
        if !changed {
            return echoed;
        }
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
        if crate::tui::is_tool_activity_line(&trimmed.to_ascii_lowercase()) {
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
