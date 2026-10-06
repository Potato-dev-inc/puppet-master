//! Cleaning a pane reply: which screen lines are TUI chrome, task echo or tool output, and what
//! is left as the assistant's result.

use std::collections::HashMap;

pub fn is_tui_chrome_line(line: &str, agent_type: &str) -> bool {
    let lower = line.trim().to_ascii_lowercase();
    lower.is_empty()
        || lower.contains("add follow-up")
        || lower.contains("add a follow-up")
        || lower == "composing"
        // Cursor's progress line, e.g. "Composing 36 tokens".
        || (lower.starts_with("composing ") && (lower.ends_with(" tokens") || lower.ends_with(" token")))
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
        || crate::tui::is_chrome_line(&lower, agent_type)
        || is_rule_line(&lower)
}

/// A line made only of rules, block fills, box art and bare prompt markers. Anything else with no
/// letters or digits (`[]`, `{}`, `""`, an emoji, a check mark) is a legitimate short reply.
fn is_rule_line(line: &str) -> bool {
    line.chars().all(|ch| {
        ch.is_whitespace()
            || matches!(
                ch,
                '-' | '=' | '_' | '~' | '*' | '#' | '+' | '.' | '|' | '/' | '\\' | ':' | '>' | '<'
                    | '\u{2026}' | '\u{b7}' | '\u{2022}' | '\u{2192}' | '\u{276f}' | '\u{203a}'
                    | '\u{bb}'
            )
            // Box drawing and block elements: the input box, rules and fills.
            || ('\u{2500}'..='\u{259f}').contains(&ch)
    })
}

/// Drop Cursor paste markers, prompt echoes, and acceptance tables so they
/// cannot be stored as an assistant result or reused as follow-up context.
pub fn clean_extracted_result(evidence: &str, task: &str, agent_type: &str) -> Option<String> {
    clean_extracted_result_after(evidence, task, "", agent_type)
}

/// As [`clean_extracted_result`], also dropping echoes of the previous turn's prompt.
pub fn clean_extracted_result_after(
    evidence: &str,
    task: &str,
    previous_task: &str,
    agent_type: &str,
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
        .rposition(|line| {
            crate::tui::is_tool_activity_line(&line.to_ascii_lowercase(), agent_type)
        })
        .map_or(0, |index| index + 1);
    let candidates: Vec<&str> = all[after_tools..]
        .iter()
        .copied()
        .filter(|line| !is_tui_chrome_line(line, agent_type))
        .collect();
    let mut echoed = echoed_lines(&candidates, task);
    let rescued = keep_last_copy_of_repeated_echoes(&candidates, &mut echoed, previous_task);
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
    let cleaned = crate::tui::tidy_reply(&lines.join("\n"), agent_type);
    // A reply rescued from a repeated echo is wholly contained in the prompt by construction.
    if !rescued && is_mostly_prompt_echo(&cleaned, task, agent_type) {
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

/// A reply can legitimately repeat text from the prompt ("reply with exactly: X"). The prompt echo
/// comes first, so when a line judged an echo appears again later, the last copy is the reply.
/// Text that only the previous turn's prompt contains is never rescued: that is the stale-prompt
/// case, which must stay dropped.
fn keep_last_copy_of_repeated_echoes(
    lines: &[&str],
    echoed: &mut [bool],
    previous_task: &str,
) -> bool {
    let mut rescued = false;
    let previous_n = normalize_echo(previous_task);
    let mut handled: Vec<String> = Vec::new();
    for index in 0..lines.len() {
        if !echoed[index] {
            continue;
        }
        let key = lines[index].trim().to_string();
        if handled.contains(&key) {
            continue;
        }
        let copies: Vec<usize> = (0..lines.len())
            .filter(|other| echoed[*other] && lines[*other].trim() == key)
            .collect();
        handled.push(key.clone());
        let line_n = normalize_echo(&key);
        let only_in_previous = !previous_n.is_empty() && previous_n.contains(&line_n);
        if copies.len() >= 2 && !only_in_previous {
            echoed[*copies.last().expect("two copies")] = false;
            rescued = true;
        }
    }
    // A reply that repeats part of a prompt row without being a row of its own: the prompt
    // "Reply with exactly this sentence, no tools: S" is echoed as one row and the reply S is a
    // slice of it. The echo comes first and has already used up that text, so a later echoed row
    // wholly contained in the rows before it is the reply, not a second echo. A wrapped echo
    // continues the text and is never contained in what precedes it.
    let mut consumed = String::new();
    for index in 0..lines.len() {
        if !echoed[index] {
            continue;
        }
        let line_n = normalize_echo(lines[index]);
        if line_n.len() >= 8
            && (previous_n.is_empty() || !previous_n.contains(&line_n))
            && format!(" {consumed} ").contains(&format!(" {line_n} "))
        {
            echoed[index] = false;
            rescued = true;
            continue;
        }
        consumed.push(' ');
        consumed.push_str(&line_n);
    }
    rescued
}

pub(super) fn normalize_echo(text: &str) -> String {
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
        || task
            .lines()
            .any(|task_line| normalize_echo(task_line) == line_n)
}

fn is_mostly_prompt_echo(cleaned: &str, task: &str, agent_type: &str) -> bool {
    if task.is_empty() {
        return false;
    }
    let task_n = normalize_echo(task);
    let out_n = normalize_echo(cleaned);
    if out_n.is_empty() {
        // Nothing alphanumeric left. Whitespace-only is an echo of nothing; a line of data
        // punctuation (`[]`, `{}`) is the reply itself, since a prompt echo is far longer.
        return cleaned.trim().is_empty();
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
        .filter(|line| line_echoes_task(line, task) || is_tui_chrome_line(line, agent_type))
        .count();
    echoed * 2 >= content_lines.len()
}

/// Cursor prints a tool's output indented deeper than the tool line itself (`$ cmd 14s`, then the
/// output at +2 columns), and the agent's reply back at the tool line's column. Drop every line
/// indented deeper than a tool marker, up to the next blank or shallower line. Indentation is
/// the only thing that separates the two, so this must run before lines are trimmed.
pub fn strip_tool_output_blocks(text: &str, agent_type: &str) -> String {
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
        if crate::tui::is_tool_activity_line(&trimmed.to_ascii_lowercase(), agent_type) {
            block_indent = Some(indent(line));
        }
        out.push(line);
    }
    out.join("\n")
}

/// The text after the last tool-call row (`Edited x +3`, `$ cmd 9s`, `Read f`, `Globbed ...`).
/// What a TUI prints after its last tool call is the final message; everything above it is tool
/// output and the commentary that introduced each call ("Now I will run the tests"), which are
/// not the answer. With no tool row the text is returned whole. Run after
/// [`strip_tool_output_blocks`] and before chrome rows are dropped, since the tool rows are
/// themselves chrome and would otherwise be gone by the time the cut is made.
fn after_last_tool_block<'a>(text: &'a str, agent_type: &str) -> &'a str {
    let mut offset = 0;
    let mut cut = 0;
    for line in text.split_inclusive('\n') {
        offset += line.len();
        if crate::tui::is_tool_activity_line(&line.trim().to_ascii_lowercase(), agent_type) {
            cut = offset;
        }
    }
    &text[cut..]
}

/// New non-chrome TUI text since `baseline`. Occurrence counts so a repeated
/// identical answer still counts as this turn's result.
pub fn tui_turn_evidence(screen: &str, baseline: &str, agent_type: &str) -> Option<String> {
    if screen == baseline {
        return None;
    }
    let (screen, baseline) = (
        strip_tool_output_blocks(&crate::tui::strip_dialogs(screen, agent_type), agent_type),
        strip_tool_output_blocks(&crate::tui::strip_dialogs(baseline, agent_type), agent_type),
    );
    let (screen, baseline) = (screen.as_str(), baseline.as_str());
    if screen == baseline {
        return None;
    }
    if let Some(suffix) = screen.strip_prefix(baseline) {
        let suffix = after_last_tool_block(suffix, agent_type);
        let added = suffix
            .lines()
            .map(str::trim)
            .filter(|line| !is_tui_chrome_line(line, agent_type))
            .collect::<Vec<_>>();
        if !added.is_empty() {
            return Some(added.join("\n"));
        }
    }
    let mut before: HashMap<&str, usize> = HashMap::new();
    for line in baseline
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        *before.entry(line).or_insert(0) += 1;
    }
    let mut seen: HashMap<&str, usize> = HashMap::new();
    let mut added = Vec::new();
    let screen = after_last_tool_block(screen, agent_type);
    for line in screen
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        if is_tui_chrome_line(line, agent_type) {
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
