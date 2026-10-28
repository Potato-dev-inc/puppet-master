//! What the index keeps of a chat item, already redacted.

use crate::session_chat::{cut_to, home_to_tilde, ChatItem};

/// Longest text kept per item, in characters (a longer one is cut with a trailing "…").
pub const MAX_TEXT_CHARS: usize = 4000;
/// Longest tool title kept (name and one-line summary).
const MAX_TITLE_CHARS: usize = 300;

/// What the app knows about the pane an item came from.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PaneContext {
    /// The pane's agent, by the id the app lists it under; the screen turns it into a label.
    pub agent: String,
    /// The folder the pane works in, as a name (not the path).
    pub folder: String,
    /// The agent's own session id when known, else empty.
    pub session: String,
}

/// One row of the index.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub pane_id: String,
    pub session: String,
    pub item_id: String,
    /// `user`, `assistant` or `tool`.
    pub kind: &'static str,
    pub agent: String,
    pub folder: String,
    /// When the item happened, in milliseconds since the epoch (the time it was read when the item has none).
    pub at_ms: i64,
    pub text: String,
}

/// The last part of a folder path, for either kind of separator; empty for a root.
pub fn folder_name(cwd: &str) -> String {
    const SEPARATORS: [char; 2] = ['/', '\\'];
    cwd.trim_end_matches(SEPARATORS).rsplit(SEPARATORS).next().unwrap_or_default().to_string()
}

/// The marks the snippet uses must not occur in the text itself.
fn clean(text: &str) -> String {
    text.chars().map(|c| if c == '\u{1}' || c == '\u{2}' { ' ' } else { c }).collect()
}

fn at_ms(at: &Option<String>, now_ms: i64) -> i64 {
    at.as_deref()
        .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
        .map(|at| at.timestamp_millis())
        .unwrap_or(now_ms)
}

/// Whether the index keeps this item at all (so the feeder copies only these).
pub fn is_kept(item: &ChatItem) -> bool {
    matches!(item, ChatItem::User { .. } | ChatItem::Assistant { .. } | ChatItem::ToolCall { .. })
}

/// The row for one item, or `None` for an item the index does not keep (tool output, usage, notices...) or one with
/// no text. Every string goes through `redact::text` first.
pub fn entry_of(pane_id: &str, ctx: &PaneContext, item: &ChatItem, now_ms: i64) -> Option<Entry> {
    let (kind, at, raw, limit) = match item {
        ChatItem::User { at, text, .. } => ("user", at, text.clone(), MAX_TEXT_CHARS),
        ChatItem::Assistant { at, text, .. } => ("assistant", at, text.clone(), MAX_TEXT_CHARS),
        ChatItem::ToolCall { at, name, summary, .. } => ("tool", at, format!("{name} {summary}"), MAX_TITLE_CHARS),
        _ => return None,
    };
    let redact_ctx = crate::redact::known::context();
    let home = redact_ctx.home.as_deref();
    // Redacted first, then the home folder becomes `~` (the account name is not kept), then cut to size.
    let text = clean(&home_to_tilde(crate::redact::text_with(raw.trim(), &redact_ctx).as_str(), home));
    let text = cut_to(text.trim(), limit);
    if text.is_empty() {
        return None;
    }
    Some(Entry {
        pane_id: pane_id.to_string(),
        session: ctx.session.clone(),
        item_id: item.id().to_string(),
        kind,
        agent: ctx.agent.clone(),
        folder: clean(&home_to_tilde(crate::redact::text_with(&ctx.folder, &redact_ctx).as_str(), home)),
        at_ms: at_ms(at, now_ms),
        text,
    })
}

impl PaneContext {
    /// The labels for a pane: its agent, the name of its folder and its session id (empty when not known).
    pub fn of(agent: &str, cwd: &str, session: Option<&str>) -> Self {
        // A pane in the home folder shows as `~`, never as the account name.
        let home = crate::redact::known::context().home.clone();
        let folder = folder_name(&home_to_tilde(cwd, home.as_deref()));
        PaneContext { agent: agent.to_string(), folder, session: session.unwrap_or_default().to_string() }
    }
}

/// The rows for a batch of items.
pub fn entries_of(pane_id: &str, ctx: &PaneContext, items: &[ChatItem], now_ms: i64) -> Vec<Entry> {
    items.iter().filter_map(|item| entry_of(pane_id, ctx, item, now_ms)).collect()
}
