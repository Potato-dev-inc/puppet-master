//! Claude Code's dialogs that wait for the user: the folder-trust check and the numbered permission
//! menus (`Do you want to proceed?` for a command, `Do you want to make this edit to <file>?`).
//!
//! The wording of these dialogs changes between Claude Code versions (`Yes, and don't ask again
//! for: npm test *`, `Yes, and switch to auto mode`, a fourth option appearing and moving `No` to
//! `4.`), so detection reads the *structure* that has stayed the same instead of matching a fixed
//! option list:
//!
//! - an anchor question, `Do you want to ...?`
//! - a numbered list `1. Yes ... N. No` whose active row carries the `\u{276f}` pointer
//! - for a command, the command sits between two dashed rules above the question
//!
//! Live captures from v2.1.299 (`fixtures/`) show why the parsing is tolerant: Claude pads every
//! row of a dialog with spaces to the full pane width, so the emulator's `contents()` glues rows
//! together (`2. Yes ...<padding>3. No`) and the options of one menu can arrive on a single line.
//! Rows are therefore found by their markers (`N.`), not by line breaks.
//!
//! Answering moves the pointer with arrow keys; `write_input` then adds the Enter. A number key
//! would select and submit on its own, and the Enter that follows it could answer the *next*
//! dialog.

use serde_json::{json, Value};

use crate::tui::{generic, Reply};

mod login;
mod question;

/// Kind of the folder-trust dialog. Generic code treats this kind as a startup gate: a run parks
/// on it and the task is typed only after it is gone (see `bridge/approvals.rs`).
pub(super) const KIND_TRUST: &str = "workspace_trust";

const POINTER: char = '\u{276f}';
const ARROW_DOWN: &str = "\u{1b}[B";
const ARROW_UP: &str = "\u{1b}[A";
/// Only the end of the screen is read: a dialog is always the last thing drawn.
const SCAN_CHARS: usize = 12_000;
/// Footer rows Claude prints under a menu.
const FOOTER_MARKERS: [&str; 3] = ["esc to cancel", "enter to confirm", "tab to amend"];

/// One selectable row of a menu.
#[derive(Debug, Clone)]
struct Opt {
    /// 1-based position in the menu.
    n: usize,
    label: String,
    /// Where the row's marker (`N.` or the label itself) starts in the scanned text.
    start: usize,
    choice: Option<&'static str>,
}

fn tail(text: &str) -> &str {
    if text.len() <= SCAN_CHARS {
        return text;
    }
    let mut cut = text.len() - SCAN_CHARS;
    while !text.is_char_boundary(cut) {
        cut += 1;
    }
    &text[cut..]
}

/// `’` -> `'`, lowercase, whitespace runs collapsed.
fn norm(label: &str) -> String {
    label
        .replace(['\u{2019}', '\u{2018}'], "'")
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Rows of a dialog block: line breaks and the runs of 3+ spaces that Claude's full-width padding
/// leaves between rows glued by the emulator.
fn rows(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let mut current = String::new();
        let mut spaces = 0;
        for ch in line.chars() {
            if ch == ' ' {
                spaces += 1;
                continue;
            }
            if spaces >= 3 && !current.trim().is_empty() {
                out.push(std::mem::take(&mut current));
            } else if spaces > 0 {
                current.push_str(&" ".repeat(spaces));
            }
            spaces = 0;
            current.push(ch);
        }
        if !current.trim().is_empty() {
            out.push(current);
        }
    }
    out.into_iter().map(|row| row.trim().to_string()).filter(|row| !row.is_empty()).collect()
}

fn is_rule_row(row: &str) -> bool {
    let rule = |ch: char| matches!(ch, '\u{2500}' | '\u{254c}' | '\u{2504}' | '\u{2508}' | '-' | '\u{2550}');
    row.chars().count() >= 8 && row.chars().all(rule)
}

/// The dialog's text as the approval card shows it: rows, rules dropped.
fn clean_text(text: &str) -> String {
    let all: Vec<String> = rows(text).into_iter().filter(|row| !is_rule_row(row)).collect();
    let start = all.len().saturating_sub(30);
    all[start..].join("\n")
}

fn alnum(text: &str) -> String {
    text.chars().filter(|ch| ch.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

fn footer_start(text: &str) -> Option<usize> {
    let lower = text.to_ascii_lowercase();
    FOOTER_MARKERS.iter().filter_map(|m| lower.find(m)).min()
}

/// The numbered options after `from` (byte offset into `text`): markers `1.`, `2.`, ... in
/// sequence, each preceded by whitespace or the pointer. Fewer than two rows is not a menu.
fn numbered_options(text: &str, from: usize) -> Vec<Opt> {
    let area_end = footer_start(&text[from..]).map_or(text.len(), |at| from + at);
    let area = &text[from..area_end];
    let bytes = area.as_bytes();
    let mut marks: Vec<(usize, usize, usize)> = Vec::new(); // (number, marker start, label start)
    let mut i = 0;
    while i < bytes.len() {
        let expected = marks.len() + 1;
        if bytes[i].is_ascii_digit() {
            let boundary = area[..i].chars().next_back().is_none_or(|ch| ch.is_whitespace() || ch == POINTER);
            let digits_end = i + bytes[i..].iter().take_while(|b| b.is_ascii_digit()).count();
            if boundary
                && digits_end - i <= 2
                && bytes.get(digits_end) == Some(&b'.')
                && bytes.get(digits_end + 1).is_some_and(|b| b.is_ascii_whitespace())
                && area[i..digits_end].parse::<usize>().ok() == Some(expected)
            {
                let label_start = digits_end + 1 + bytes[digits_end + 1..].iter().take_while(|b| b.is_ascii_whitespace()).count();
                marks.push((expected, i, label_start));
                i = label_start;
                continue;
            }
            i = digits_end;
            continue;
        }
        i += 1;
    }
    let mut options = Vec::new();
    for (index, &(n, start, label_start)) in marks.iter().enumerate() {
        let end = marks.get(index + 1).map_or(area.len(), |next| next.1);
        let raw = &area[label_start..end];
        let raw = raw.trim_end().trim_end_matches(POINTER);
        // A label ends at a line break or at the padding that precedes the next glued row.
        let label = raw
            .split('\n')
            .next()
            .unwrap_or_default()
            .split("   ")
            .next()
            .unwrap_or_default()
            .trim();
        options.push(Opt { n, label: label.to_string(), start: from + start, choice: None });
    }
    if options.len() < 2 {
        return Vec::new();
    }
    options
}

/// The 1-based option the pointer is on (`1` when it cannot be seen).
fn selected(text: &str, from: usize, options: &[Opt]) -> usize {
    let mut found = None;
    for (offset, ch) in text[from..].char_indices() {
        if ch != POINTER {
            continue;
        }
        let after = from + offset + ch.len_utf8();
        if let Some(option) = options.iter().find(|o| o.start >= after) {
            if text[after..option.start].trim().is_empty() {
                found = Some(option.n);
            }
        }
    }
    found.unwrap_or(1)
}

fn has_pointer_on_an_option(text: &str, from: usize, options: &[Opt]) -> bool {
    text[from..].char_indices().any(|(offset, ch)| {
        let after = from + offset + ch.len_utf8();
        ch == POINTER && options.iter().any(|o| o.start >= after && text[after..o.start].trim().is_empty())
    })
}

/// Map a menu row to a stable choice id. `allow_once` is a plain `Yes`; a `Yes, and ...` that
/// widens the approval (`don't ask again`, `switch to auto mode`, `accept edits for this session`)
/// is `allow_all`; `No` is `deny`. Any other row has no id and is never offered.
fn classify_option(label: &str) -> (Option<&'static str>, bool) {
    let label = norm(label);
    let word = label.split(|ch: char| !ch.is_alphanumeric() && ch != '\'').next().unwrap_or_default();
    match word {
        "no" => (Some("deny"), false),
        "yes" => {
            let rest = label[3..].trim_start_matches([',', ' ', '.']);
            if rest.is_empty() || rest.starts_with("(once)") || rest.starts_with("this time") {
                return (Some("allow_once"), false);
            }
            // "don't ask again" is the persistent rule for this exact command, the most specific
            // widening, so it wins the single `allow_all` slot over "switch to auto mode".
            let persistent = ["don't ask again", "do not ask again", "always allow", "allow always"].iter().any(|p| rest.contains(p));
            let widening = ["auto mode", "auto-accept", "auto accept", "auto-approve", "accept edits", "for this session", "bypass", "allow all"]
                .iter()
                .any(|p| rest.contains(p));
            if persistent || widening {
                (Some("allow_all"), persistent)
            } else {
                (None, false)
            }
        }
        _ => (None, false),
    }
}

/// Assign choice ids. Returns the rows, with exactly one row per id.
fn assign_choices(mut options: Vec<Opt>) -> Vec<Opt> {
    let mut all_best: Option<(usize, bool)> = None; // (index, persistent)
    for (index, option) in options.iter().enumerate() {
        if let (Some("allow_all"), persistent) = classify_option(&option.label) {
            if all_best.is_none_or(|(_, best)| persistent && !best) {
                all_best = Some((index, persistent));
            }
        }
    }
    let (mut once_done, mut deny_done) = (false, false);
    for (index, option) in options.iter_mut().enumerate() {
        option.choice = match classify_option(&option.label).0 {
            Some("allow_once") if !once_done => {
                once_done = true;
                Some("allow_once")
            }
            Some("deny") if !deny_done => {
                deny_done = true;
                Some("deny")
            }
            Some("allow_all") if all_best.is_some_and(|(best, _)| best == index) => Some("allow_all"),
            _ => None,
        };
    }
    options
}

fn menu_json(options: &[Opt], selected: usize) -> Value {
    json!({
        "selected": selected,
        "options": options.iter().map(|o| json!({"n": o.n, "label": o.label, "choice": o.choice})).collect::<Vec<_>>(),
    })
}

fn choices_from(options: &[Opt], allow_broad: bool) -> Vec<String> {
    let mut out = Vec::new();
    for id in ["allow_once", "deny", "allow_all"] {
        if id == "allow_all" && !allow_broad {
            continue;
        }
        if options.iter().any(|o| o.choice == Some(id)) {
            out.push(id.to_string());
        }
    }
    out
}

/// The last `Do you want to ...?` question that is followed by a numbered menu.
fn permission_menu(agent_type: &str, screen: &str, allow_broad: bool) -> Option<Value> {
    let text = tail(screen);
    let lower = text.to_ascii_lowercase();
    // Every candidate question, last first: an older, answered dialog may still be drawn higher up.
    let mut search_end = lower.len();
    while let Some(at) = lower[..search_end].rfind("do you want to ") {
        search_end = at;
        let question_end = match lower[at..].find('?') {
            Some(q) if q <= 220 && !lower[at..at + q].contains('\n') => at + q + 1,
            _ => continue,
        };
        let options = numbered_options(text, question_end);
        if options.is_empty() || !has_pointer_on_an_option(text, question_end, &options) {
            continue;
        }
        let options = assign_choices(options);
        // Both a plain yes and a no must be present, or the menu is not a permission dialog.
        if !options.iter().any(|o| o.choice == Some("allow_once")) || !options.iter().any(|o| o.choice == Some("deny")) {
            continue;
        }
        // The menu is the last thing on screen: only the footer may follow it. A dialog still in
        // the scrollback region with real output drawn below it was answered long ago.
        if let Some(footer) = footer_start(&text[question_end..]) {
            if text[question_end + footer..].trim().chars().count() > 120 {
                continue;
            }
        }
        let question = text[at..question_end].trim().to_string();
        let selected = selected(text, question_end, &options);
        let before = &text[..at];
        return Some(build_permission_prompt(agent_type, before, &question, &options, selected, allow_broad));
    }
    None
}

fn build_permission_prompt(agent_type: &str, before: &str, question: &str, options: &[Opt], selected: usize, allow_broad: bool) -> Value {
    let question_lower = question.to_ascii_lowercase();
    let mut prompt = generic::prompt_json(agent_type, "tool_approval", &clean_text(&format!("{before}\n{question}")), choices_from(options, allow_broad));
    prompt["menu"] = menu_json(options, selected);
    prompt["summary"] = Value::String(question.to_string());
    // The dialog's own block starts at its top rule: Claude draws one solid rule above the header.
    let block_start = match before.rfind('\u{2500}') {
        // Walk back over the whole run of rule characters.
        Some(at) => before[..at].trim_end_matches('\u{2500}').len(),
        None => {
            // No rule drawn (another version): the last stretch before the question.
            let mut cut = before.len().saturating_sub(1_500);
            while !before.is_char_boundary(cut) {
                cut += 1;
            }
            cut
        }
    };
    let block = &before[block_start..];

    let file_action = ["make this edit to ", "create ", "overwrite ", "write to ", "delete "]
        .iter()
        .find_map(|verb| question_lower.find(verb).map(|at| (*verb, at + verb.len())));
    if let Some((verb, at)) = file_action {
        let path = question[at..].trim_end_matches('?').trim().to_string();
        let action = if verb.starts_with("make") { "Edit" } else if verb == "delete " { "Delete" } else if verb == "create " { "Create" } else { "Write" };
        prompt["kind"] = Value::String("file_edit".into());
        prompt["summary"] = Value::String(format!("{action} {path}"));
        prompt["path"] = Value::String(path.clone());
        // The diff itself is part of what is being approved, so two edits to one file differ.
        prompt["identity"] = Value::String(format!("file_edit|{}|{}", alnum(&path), alnum(block)));
        return prompt;
    }
    if let Some((command, reason)) = command_box(block) {
        prompt["kind"] = Value::String("command_approval".into());
        prompt["command"] = Value::String(command);
        if let Some(reason) = reason {
            prompt["reason"] = Value::String(reason);
        }
        return prompt;
    }
    prompt["identity"] = Value::String(format!("tool_approval|{}|{}", alnum(question), alnum(block)));
    prompt
}

/// The command shown between the two dashed rules under a `... command` header, and the reason
/// row below it. `None` when the block has no such box (another tool's dialog).
fn command_box(block: &str) -> Option<(String, Option<String>)> {
    let dashed = |ch: char| matches!(ch, '\u{254c}' | '\u{2504}' | '\u{2508}');
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut start = None;
    let mut count = 0;
    for (at, ch) in block.char_indices() {
        if dashed(ch) {
            start.get_or_insert(at);
            count += 1;
        } else if let Some(s) = start.take() {
            if count >= 3 {
                runs.push((s, at));
            }
            count = 0;
        }
    }
    if let Some(s) = start {
        if count >= 3 {
            runs.push((s, block.len()));
        }
    }
    if runs.len() < 2 {
        return None;
    }
    // The last two dashed rules before the question bracket the command.
    let (open, close) = (runs[runs.len() - 2], runs[runs.len() - 1]);
    let header = block[..open.0].to_ascii_lowercase();
    if !header.contains("command") {
        return None;
    }
    let command_rows: Vec<String> = rows(&block[open.1..close.0])
        .into_iter()
        .map(|row| row.trim_start_matches(['\u{2502}', '|', ' ']).trim().to_string())
        .filter(|row| !row.is_empty())
        .collect();
    if command_rows.is_empty() {
        return None;
    }
    let reason = rows(&block[close.1..]).into_iter().find(|row| !is_rule_row(row));
    Some((command_rows.join("\n"), reason))
}

/// Folder-trust dialog (`Quick safety check: Is this a project you created or one you trust?`).
fn trust_gate(agent_type: &str, screen: &str) -> Option<Value> {
    let text = tail(screen);
    let lower = text.to_ascii_lowercase();
    let yes_at = lower.rfind("yes, i trust this folder")?;
    let yes_end = yes_at + "yes, i trust this folder".len();
    let title_at = ["quick safety check", "do you trust the files", "accessing workspace"]
        .iter()
        .filter_map(|marker| lower[..yes_at].rfind(marker))
        .min()?;
    // A live dialog ends with its footer; once answered, Claude draws its own screen over it.
    let rest = FOOTER_MARKERS.iter().fold(lower[yes_end..].to_string(), |acc, marker| acc.replace(marker, ""));
    if rest.chars().any(|ch| ch.is_alphanumeric()) {
        return None;
    }
    // The two rows. Numbered variants (`1. Yes, I trust this folder`) are read like any menu.
    let mut options: Vec<Opt> = Vec::new();
    for (label, choice) in [("no, exit", "deny"), ("yes, i trust this folder", "trust")] {
        if let Some(at) = lower[title_at..].rfind(label).map(|at| at + title_at) {
            options.push(Opt { n: 0, label: text[at..at + label.len()].to_string(), start: at, choice: Some(choice) });
        }
    }
    options.sort_by_key(|o| o.start);
    for (index, option) in options.iter_mut().enumerate() {
        option.n = index + 1;
    }
    if !options.iter().any(|o| o.choice == Some("trust")) {
        return None;
    }
    let selected = selected(text, title_at, &options);
    let folder = lower[title_at..]
        .find("accessing workspace:")
        .map(|at| title_at + at + "accessing workspace:".len())
        .and_then(|from| rows(&text[from..]).into_iter().next())
        .filter(|row| row.len() <= 260 && !row.to_ascii_lowercase().starts_with("quick safety"));
    let mut choices = vec!["trust".to_string()];
    if options.iter().any(|o| o.choice == Some("deny")) {
        choices.push("deny".to_string());
    }
    let mut prompt = generic::prompt_json(agent_type, KIND_TRUST, &clean_text(&text[title_at..]), choices);
    prompt["menu"] = menu_json(&options, selected);
    // One trust dialog per pane at a time: its text must not decide its id (it re-wraps).
    prompt["identity"] = Value::String("workspace_trust".into());
    prompt["summary"] = Value::String(match &folder {
        Some(folder) => format!("Claude Code asks to trust {folder}"),
        None => "Claude Code asks to trust this folder".into(),
    });
    if let Some(folder) = folder {
        prompt["folder"] = Value::String(folder);
    }
    prompt["labels"] = json!({"trust": "Trust this folder", "deny": "Don't trust"});
    Some(prompt)
}

/// Whether the folder-trust dialog is on screen.
pub(super) fn shows_trust_gate(screen: &str) -> bool {
    trust_gate("claude", screen).is_some()
}

/// Whether a numbered permission menu is on screen (a pane waiting for the user).
pub(super) fn shows_permission_menu(screen: &str) -> bool {
    permission_menu("claude", screen, false).is_some()
}

/// The dialog on screen, or `None`. The caller falls back to the generic detection for the older
/// `Allow once` / `Always allow` wording.
pub(super) fn detect_prompt(agent_type: &str, screen: &str, allow_broad: bool) -> Option<Value> {
    trust_gate(agent_type, screen)
        .or_else(|| permission_menu(agent_type, screen, allow_broad))
        .or_else(|| question::question_menu(agent_type, screen))
        .or_else(|| login::login_menu(agent_type, screen))
}

/// Whether Claude's own question menu (AskUserQuestion) is on screen: a pane waiting for the person.
pub(super) fn shows_question_menu(screen: &str) -> bool {
    question::shows_question_menu(screen)
}

/// Whether Claude's sign-in menu (`/login`) is on screen: a pane waiting for the person.
pub(super) fn shows_login_menu(screen: &str) -> bool {
    login::shows_login_menu(screen)
}

/// Arrow keys that move the pointer from `from` to `to` (1-based rows).
fn arrows(from: usize, to: usize) -> String {
    if to >= from {
        ARROW_DOWN.repeat(to - from)
    } else {
        ARROW_UP.repeat(from - to)
    }
}

/// Answer a prompt read by `detect_prompt`. The key is the arrow presses that put the pointer on
/// the row; the caller's Enter selects it. A prompt without a `menu` is the generic wording.
pub(super) fn prompt_reply(prompt: &Value, choice: &str, allow_broad: bool) -> Result<Reply, String> {
    let Some(menu) = prompt.get("menu") else {
        return generic::prompt_reply(prompt, choice, allow_broad);
    };
    if matches!(choice, "allow_all" | "allow_broad" | "always") && !allow_broad {
        return Err("broad approval requires explicit allow_broad".into());
    }
    // A card that names its own way out (the sign-in menu: Esc). The route adds an Enter after it, which does nothing
    // on the empty input box Claude returns to.
    if choice == "cancel" {
        if let Some(key) = prompt.get("cancel_key").and_then(Value::as_str) {
            return Ok(Reply::key(key));
        }
    }
    let options = menu.get("options").and_then(Value::as_array).cloned().unwrap_or_default();
    let target = options
        .iter()
        .find(|o| o.get("choice").and_then(Value::as_str) == Some(choice))
        .and_then(|o| o.get("n").and_then(Value::as_u64))
        .ok_or_else(|| "unsupported approval choice".to_string())? as usize;
    let from = menu.get("selected").and_then(Value::as_u64).unwrap_or(1) as usize;
    Ok(Reply::key(&arrows(from, target)))
}

#[cfg(test)]
#[path = "prompt_tests.rs"]
mod tests;
