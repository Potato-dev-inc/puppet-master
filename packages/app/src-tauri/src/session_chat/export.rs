//! "Save chat as page": one self-contained HTML file of a pane's chat, to keep or send. The page has inline CSS
//! and no script, and links to nothing (no `href`, `src`, font or image). Every string goes through `redact` first,
//! folder paths shrink to the folder name and text is escaped (`export_text`). Generic: every agent's items are
//! drawn the same way; only the agent's display name is shown.

use serde_json::Value;

use super::detail::{PlanItem, PlanStatus};
use super::export_style::CSS;
use super::export_text::{clean, escape, lines, MAX_LINES};
use super::model::{ChatItem, TurnOutcome};
use crate::redact::RedactContext;

pub use super::export_text::folder_of;

const FOOTER: &str = "Saved from Puppet Master";

/// What the page header says about the chat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Meta {
    /// The agent's display name (for example a label; the agent id when it has none).
    pub agent: String,
    /// The folder the agent works in, by name only.
    pub folder: String,
    /// The day the page was made, `YYYY-MM-DD`.
    pub date: String,
}

/// Turns to keep, inclusive. Turns are numbered from 1 by each of the person's messages; 0 is what the agent said
/// before the first one. `None` on a side means no limit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TurnRange {
    pub from: Option<u32>,
    pub to: Option<u32>,
}

impl TurnRange {
    /// Refuses a range whose first turn comes after its last.
    pub fn check(&self) -> Result<(), String> {
        match (self.from, self.to) {
            (Some(from), Some(to)) if from > to => Err("The first turn comes after the last turn.".into()),
            _ => Ok(()),
        }
    }

    pub fn holds(&self, turn: u32) -> bool {
        self.from.map_or(true, |from| turn >= from) && self.to.map_or(true, |to| turn <= to)
    }
}

/// The turn each item belongs to, in order (see `TurnRange`).
pub fn turn_numbers(items: &[ChatItem]) -> Vec<u32> {
    let mut turn = 0;
    items
        .iter()
        .map(|item| {
            if matches!(item, ChatItem::User { .. }) {
                turn += 1;
            }
            turn
        })
        .collect()
}

/// The whole page: the items in `range`, under a header and a footer.
pub fn page(items: &[ChatItem], meta: &Meta, range: TurnRange, ctx: &RedactContext) -> String {
    let turns = turn_numbers(items);
    let mut body = String::new();
    for (item, turn) in items.iter().zip(&turns) {
        if range.holds(*turn) {
            item_html(&mut body, item, meta, ctx);
        }
    }
    if body.is_empty() {
        body.push_str("<p class=\"empty\">Nothing to show.</p>\n");
    }
    let agent = clean(&meta.agent, ctx);
    let folder = clean(&meta.folder, ctx);
    let range_line = if range == TurnRange::default() {
        String::new()
    } else {
        let last = turns.last().copied().unwrap_or(0);
        format!("<p class=\"range\">Turns {} to {}</p>\n", range.from.unwrap_or(0), range.to.unwrap_or(last))
    };
    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<meta name=\"color-scheme\" content=\"light dark\">\n<title>{agent} · {folder}</title>\n<style>\n{css}\n</style>\n</head>\n<body>\n<main class=\"page\">\n<header class=\"head\">\n<p class=\"eyebrow\">Chat</p>\n<h1>{agent}</h1>\n<p class=\"meta\"><span>{folder}</span><span>{date}</span></p>\n{range_line}</header>\n<div class=\"chat\">\n{body}</div>\n<footer class=\"foot\">{footer}</footer>\n</main>\n</body>\n</html>\n",
        css = CSS,
        date = escape(&meta.date),
        footer = FOOTER,
    )
}

fn item_html(out: &mut String, item: &ChatItem, meta: &Meta, ctx: &RedactContext) {
    match item {
        ChatItem::User { text, .. } => message(out, "you", "You", text, ctx),
        ChatItem::Assistant { text, .. } => message(out, "agent", &clean(&meta.agent, ctx), text, ctx),
        ChatItem::ToolCall { name, summary, detail, .. } => tool_html(out, name, summary, detail.input.as_ref(), ctx),
        ChatItem::ToolResult { ok, output, .. } => result_html(out, *ok, output, ctx),
        ChatItem::TurnEnd { outcome, duration_ms, .. } => turn_end_html(out, outcome, *duration_ms),
        ChatItem::Plan { items, .. } => plan_html(out, items, ctx),
        ChatItem::Notice { text, .. } => {
            if !text.trim().is_empty() {
                out.push_str(&format!("<p class=\"notice\">{}</p>\n", clean(text, ctx)));
            }
        }
        // Token counts and the session header are not part of the conversation.
        ChatItem::Usage { .. } | ChatItem::Session { .. } => {}
    }
}

/// `who` is already safe to show (a label, or the output of `clean`).
fn message(out: &mut String, class: &str, who: &str, text: &str, ctx: &RedactContext) {
    if text.trim().is_empty() {
        return;
    }
    out.push_str(&format!("<section class=\"msg {class}\">\n<h3>{who}</h3>\n<div class=\"text\">{}</div>\n</section>\n", clean(text, ctx)));
}

/// One line: the tool's name, then its target. An edit also shows its change as a small diff.
fn tool_html(out: &mut String, name: &str, summary: &str, input: Option<&Value>, ctx: &RedactContext) {
    let target = if summary.trim().is_empty() { String::new() } else { format!(" <span>{}</span>", clean(summary, ctx)) };
    out.push_str(&format!("<div class=\"tool\"><span class=\"name\">{}</span>{target}</div>\n", clean(name, ctx)));
    if let Some((old, new)) = input.and_then(edit_sides) {
        diff_html(out, old, new, ctx);
    }
}

/// The old and new text of an edit-like call, when its input has both.
fn edit_sides(input: &Value) -> Option<(&str, &str)> {
    Some((input.get("old_string")?.as_str()?, input.get("new_string")?.as_str()?))
}

fn diff_html(out: &mut String, old: &str, new: &str, ctx: &RedactContext) {
    let (removed, removed_more) = lines(old, ctx, MAX_LINES);
    let (added, added_more) = lines(new, ctx, MAX_LINES);
    out.push_str("<pre class=\"diff\">");
    for line in removed {
        out.push_str(&format!("<span class=\"del\">- {line}</span>"));
    }
    if removed_more > 0 {
        out.push_str(&format!("<span class=\"more\">… {removed_more} more lines</span>"));
    }
    for line in added {
        out.push_str(&format!("<span class=\"add\">+ {line}</span>"));
    }
    if added_more > 0 {
        out.push_str(&format!("<span class=\"more\">… {added_more} more lines</span>"));
    }
    out.push_str("</pre>\n");
}

/// A tool's output, at most `MAX_LINES` lines.
fn result_html(out: &mut String, ok: bool, output: &str, ctx: &RedactContext) {
    if output.trim().is_empty() {
        return;
    }
    let (shown, more) = lines(output, ctx, MAX_LINES);
    let class = if ok { "output" } else { "output failed" };
    out.push_str(&format!("<pre class=\"{class}\">{}</pre>\n", shown.join("\n")));
    if more > 0 {
        out.push_str(&format!("<p class=\"more\">… {more} more lines</p>\n"));
    }
}

fn turn_end_html(out: &mut String, outcome: &TurnOutcome, duration_ms: Option<u64>) {
    let mut parts: Vec<String> = Vec::new();
    match outcome {
        TurnOutcome::Interrupted => parts.push("Interrupted".into()),
        TurnOutcome::Failed => parts.push("Failed".into()),
        TurnOutcome::Completed => {}
    }
    if let Some(ms) = duration_ms {
        parts.push(format!("Worked for {}", worked(ms)));
    }
    if parts.is_empty() {
        return;
    }
    out.push_str(&format!("<div class=\"turn-end\"><span>{}</span></div>\n", escape(&parts.join(" · "))));
}

fn worked(ms: u64) -> String {
    let secs = ms / 1000;
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m", secs / 3600, secs % 3600 / 60)
    }
}

fn plan_html(out: &mut String, items: &[PlanItem], ctx: &RedactContext) {
    let rows: String = items
        .iter()
        .filter(|item| !item.text.trim().is_empty())
        .map(|item| {
            let (mark, class) = match item.status {
                Some(PlanStatus::Completed) => ("[x]", "done"),
                Some(PlanStatus::InProgress) => ("[~]", "doing"),
                Some(PlanStatus::Deleted) => ("[-]", "gone"),
                Some(PlanStatus::Pending) | None => ("[ ]", "todo"),
            };
            format!("<li class=\"{class}\"><span class=\"mark\">{mark}</span> {}</li>", clean(&item.text, ctx))
        })
        .collect();
    if !rows.is_empty() {
        out.push_str(&format!("<section class=\"plan\">\n<h3>Task list</h3>\n<ul>{rows}</ul>\n</section>\n"));
    }
}

#[cfg(test)]
#[path = "export_tests.rs"]
mod tests;
