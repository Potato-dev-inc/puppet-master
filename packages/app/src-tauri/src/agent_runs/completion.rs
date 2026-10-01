//! Deterministic turn-completion for TUI workers.
//!
//! A fake backend drives `observe_tui_turn` with pane status + screen snapshots.
//! Live supervision is a thin loop over the same function.

use crate::operations::ResultCapture;
use std::collections::HashMap;

pub const STABLE_IDLE_TICKS: u8 = 2;
pub const RECONCILE_IDLE_TICKS: u8 = 4;

#[derive(Debug, Clone, Default)]
pub struct TuiTurnWatch {
    pub baseline: String,
    pub task: String,
    idle_evidence: Option<String>,
    idle_ticks: u8,
    reconcile_ticks: u8,
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
    if watch.reconcile_ticks >= RECONCILE_IDLE_TICKS {
        TuiTurnDecision::AmbiguousIdle
    } else {
        TuiTurnDecision::Continue
    }
}

pub fn is_tui_chrome_line(line: &str) -> bool {
    let lower = line.trim().to_ascii_lowercase();
    lower.is_empty()
        || lower.contains("add follow-up")
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
        || lower.chars().all(|ch| !ch.is_alphanumeric())
}

/// Drop Cursor paste markers, prompt echoes, and acceptance tables so they
/// cannot be stored as an assistant result or reused as follow-up context.
pub fn clean_extracted_result(evidence: &str, task: &str) -> Option<String> {
    let lines: Vec<&str> = evidence
        .lines()
        .map(str::trim)
        .filter(|line| !is_tui_chrome_line(line) && !line_echoes_task(line, task))
        .collect();
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

/// New non-chrome TUI text since `baseline`. Occurrence counts so a repeated
/// identical answer still counts as this turn's result.
pub fn tui_turn_evidence(screen: &str, baseline: &str) -> Option<String> {
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
