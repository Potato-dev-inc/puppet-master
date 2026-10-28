//! "Save as Markdown": the same chat as `export` makes, as one Markdown file. Same pipeline as the page
//! (`export_text`): every text is redacted whole, the home folder becomes `~`, folder paths shrink to their last name,
//! and only then is the text shaped as Markdown. Tool calls are one line, an edit is a ```diff block, tool output is
//! at most `MAX_LINES` lines. Markdown has no markup escaping, so raw HTML is neutralised here: outside code a `<`
//! becomes `&lt;` (no tag can start), and an image opener `![` is written `!&#91;` (no image is loaded).

use serde_json::Value;

use super::detail::{PlanItem, PlanStatus};
use super::export::{turn_end_parts, turn_numbers, Meta, TurnRange};
use super::export_text::{cut_lines, cut_paths, MAX_LINES};
use super::model::ChatItem;
use crate::redact::RedactContext;

const FOOTER: &str = "Saved from Puppet Master";

/// The whole Markdown file: the items in `range`, under a heading and a footer.
pub fn markdown(items: &[ChatItem], meta: &Meta, range: TurnRange, ctx: &RedactContext) -> String {
    let turns = turn_numbers(items);
    let mut blocks: Vec<String> = Vec::new();
    for (item, turn) in items.iter().zip(&turns) {
        if range.holds(*turn) {
            item_md(&mut blocks, item, meta, ctx);
        }
    }
    if blocks.is_empty() {
        blocks.push("_Nothing to show._".into());
    }
    let mut head = vec![format!("# {}", line_text(&meta.agent, ctx)), format!("{} · {}", line_text(&meta.folder, ctx), meta.date)];
    if range != TurnRange::default() {
        let last = turns.last().copied().unwrap_or(0);
        head.push(format!("Turns {} to {}", range.from.unwrap_or(0), range.to.unwrap_or(last)));
    }
    format!("{}\n\n---\n\n{}\n\n---\n\n_{FOOTER}_\n", head.join("\n\n"), blocks.join("\n\n"))
}

fn item_md(out: &mut Vec<String>, item: &ChatItem, meta: &Meta, ctx: &RedactContext) {
    match item {
        ChatItem::User { text, .. } => message(out, "You", text, ctx),
        ChatItem::Assistant { text, .. } => message(out, &line_text(&meta.agent, ctx), text, ctx),
        ChatItem::ToolCall { name, summary, detail, .. } => tool_md(out, name, summary, detail.input.as_ref(), ctx),
        ChatItem::ToolResult { ok, output, .. } => result_md(out, *ok, output, ctx),
        ChatItem::TurnEnd { outcome, duration_ms, .. } => {
            let parts = turn_end_parts(outcome, *duration_ms);
            if !parts.is_empty() {
                out.push(format!("_{}_", inline(&parts.join(" · "))));
            }
        }
        ChatItem::Plan { items, .. } => plan_md(out, items, ctx),
        ChatItem::Notice { text, .. } => {
            if !text.trim().is_empty() {
                out.push(format!("_{}_", line_text(text, ctx)));
            }
        }
        // Token counts and the session header are not part of the conversation.
        ChatItem::Usage { .. } | ChatItem::Session { .. } => {}
    }
}

/// `who` is already safe to show (a label, or the output of `line_text`).
fn message(out: &mut Vec<String>, who: &str, text: &str, ctx: &RedactContext) {
    if text.trim().is_empty() {
        return;
    }
    out.push(format!("**{who}**\n\n{}", block_text(text, ctx)));
}

/// One line: the tool's name, then its target. An edit also shows its change as a diff block.
fn tool_md(out: &mut Vec<String>, name: &str, summary: &str, input: Option<&Value>, ctx: &RedactContext) {
    let target = if summary.trim().is_empty() { String::new() } else { format!(" {}", line_text(summary, ctx)) };
    out.push(format!("- **{}**{target}", line_text(name, ctx)));
    if let Some((old, new)) = input.and_then(edit_sides) {
        diff_md(out, old, new, ctx);
    }
}

/// The old and new text of an edit-like call, when its input has both.
fn edit_sides(input: &Value) -> Option<(&str, &str)> {
    Some((input.get("old_string")?.as_str()?, input.get("new_string")?.as_str()?))
}

fn diff_md(out: &mut Vec<String>, old: &str, new: &str, ctx: &RedactContext) {
    let (removed, removed_more) = cut_lines(old, ctx, MAX_LINES);
    let (added, added_more) = cut_lines(new, ctx, MAX_LINES);
    let mut body: Vec<String> = removed.iter().map(|line| format!("- {line}")).collect();
    body.extend(added.iter().map(|line| format!("+ {line}")));
    out.push(fenced("diff", &body.join("\n")));
    if removed_more > 0 {
        out.push(format!("_… {removed_more} more removed lines_"));
    }
    if added_more > 0 {
        out.push(format!("_… {added_more} more added lines_"));
    }
}

/// A tool's output, at most `MAX_LINES` lines.
fn result_md(out: &mut Vec<String>, ok: bool, output: &str, ctx: &RedactContext) {
    if output.trim().is_empty() {
        return;
    }
    let (shown, more) = cut_lines(output, ctx, MAX_LINES);
    if !ok {
        out.push("_Failed._".into());
    }
    out.push(fenced("", &shown.join("\n")));
    if more > 0 {
        out.push(format!("_… {more} more lines_"));
    }
}

fn plan_md(out: &mut Vec<String>, items: &[PlanItem], ctx: &RedactContext) {
    let rows: Vec<String> = items
        .iter()
        .filter(|item| !item.text.trim().is_empty())
        .map(|item| {
            let text = line_text(&item.text, ctx);
            match item.status {
                Some(PlanStatus::Completed) => format!("- [x] {text}"),
                Some(PlanStatus::InProgress) => format!("- [ ] {text} _(in progress)_"),
                Some(PlanStatus::Deleted) => format!("- ~~{text}~~"),
                Some(PlanStatus::Pending) | None => format!("- [ ] {text}"),
            }
        })
        .collect();
    if !rows.is_empty() {
        out.push(format!("**Task list**\n\n{}", rows.join("\n")));
    }
}

/// A code block whose fence is longer than any backtick run in `body`, so the body cannot end it.
fn fenced(info: &str, body: &str) -> String {
    let mut longest = 0;
    let mut run = 0;
    for c in body.chars() {
        run = if c == '`' { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    let fence = "`".repeat(longest.max(2) + 1);
    format!("{fence}{info}\n{body}\n{fence}")
}

/// A short piece of text on one line (a name, a target, a task): redacted, path-cut and escaped for Markdown.
fn line_text(text: &str, ctx: &RedactContext) -> String {
    inline(&cut_paths(text, ctx))
}

/// A block of chat text (a message): redacted, path-cut and kept as Markdown, with raw HTML neutralised.
fn block_text(text: &str, ctx: &RedactContext) -> String {
    neutralise(cut_paths(text, ctx).trim_end())
}

/// Escapes what would be read as Markdown in one line of text; newlines become spaces.
fn inline(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\n' | '\r' | '\t' => out.push(' '),
            '\\' | '`' | '*' | '_' | '[' | ']' => {
                out.push('\\');
                out.push(c);
            }
            '<' => out.push_str("&lt;"),
            _ => out.push(c),
        }
    }
    out
}

/// Makes a block of chat text safe for Markdown. Outside code, `<` becomes `&lt;` and `![` becomes `!&#91;`. Fenced
/// code blocks and backtick spans are left as they are: a renderer shows them as text. The opening rule for a fence
/// is the strict one (see `fence_run`), so a line is only kept as code when a renderer would keep it as code too.
/// A fence still open at the end is closed, so the rest of the page keeps its shape.
pub fn neutralise(text: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    for line in text.split('\n') {
        match fence {
            Some((ch, len)) => {
                if fence_run(line).is_some_and(|(c, l, info)| c == ch && l >= len && info.trim().is_empty()) {
                    fence = None;
                }
                out.push(line.to_string());
            }
            None => match fence_run(line) {
                Some((ch, len, _)) => {
                    fence = Some((ch, len));
                    out.push(line.to_string());
                }
                None => out.push(escape_line(line)),
            },
        }
    }
    if let Some((ch, len)) = fence {
        out.push(ch.to_string().repeat(len));
    }
    out.join("\n")
}

/// A fence line: up to three spaces, then three or more backticks or tildes. For backticks the rest of the line
/// may not hold a backtick. Returns the fence character, its length and the text after it.
fn fence_run(line: &str) -> Option<(char, usize, &str)> {
    let rest = line.trim_start_matches(' ');
    if line.len() - rest.len() > 3 {
        return None;
    }
    let ch = rest.chars().next()?;
    if ch != '`' && ch != '~' {
        return None;
    }
    let len = rest.chars().take_while(|c| *c == ch).count();
    if len < 3 {
        return None;
    }
    let info = &rest[len..];
    if ch == '`' && info.contains('`') {
        return None;
    }
    Some((ch, len, info))
}

/// One line outside code: `<` and `![` are neutralised; a backtick span and a backslash escape are copied as they are.
fn escape_line(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(line.len() + 8);
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && chars.get(i + 1).is_some_and(|next| next.is_ascii_punctuation()) {
            out.push(c);
            out.push(chars[i + 1]);
            i += 2;
            continue;
        }
        if c == '`' {
            // A span with a matching closing run is copied whole; a lone run is copied as it is.
            let run = chars[i..].iter().take_while(|x| **x == '`').count();
            let stop = close_run(&chars, i + run, run).unwrap_or(i + run);
            out.extend(&chars[i..stop]);
            i = stop;
            continue;
        }
        match c {
            '<' => out.push_str("&lt;"),
            '!' if chars.get(i + 1) == Some(&'[') => {
                out.push_str("!&#91;");
                i += 2;
                continue;
            }
            _ => out.push(c),
        }
        i += 1;
    }
    out
}

/// The index just after the first backtick run of exactly `run` backticks at or after `from`, when there is one.
fn close_run(chars: &[char], from: usize, run: usize) -> Option<usize> {
    let mut j = from;
    while j < chars.len() {
        if chars[j] == '`' {
            let n = chars[j..].iter().take_while(|x| **x == '`').count();
            if n == run {
                return Some(j + n);
            }
            j += n;
        } else {
            j += 1;
        }
    }
    None
}

#[cfg(test)]
#[path = "export_md_tests.rs"]
mod tests;
