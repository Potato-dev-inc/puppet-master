//! What the index keeps of a chat item, already redacted.

use crate::session_chat::{cut_to, home_to_tilde, ChatItem};

/// Longest text kept per item, in characters (a longer one is cut with a trailing "…").
pub const MAX_TEXT_CHARS: usize = 4000;
/// Longest text kept per item of an old chat that is imported (live chats keep `MAX_TEXT_CHARS`).
pub const MAX_IMPORTED_TEXT_CHARS: usize = 2000;
/// Longest tool title kept (name and one-line summary).
const MAX_TITLE_CHARS: usize = 300;

/// How hard a text is cleaned before it is kept. `Secrets` is for live chats (known secrets and token shapes).
/// `Strict` is for old chats that are imported: also long opaque runs, the person's folder name and email addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strength {
    Secrets,
    Strict,
}

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
    /// True for an item of an old chat that was imported (`chat_items.imported = 1`).
    pub imported: bool,
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

/// The time an item says it happened, when it says one (`None` for an item without a readable time). The importer
/// uses the latest of these as the chat's own last-update time, instead of the file's modified time.
pub fn item_time_ms(item: &ChatItem) -> Option<i64> {
    let at = match item {
        ChatItem::User { at, .. } | ChatItem::Assistant { at, .. } | ChatItem::ToolCall { at, .. } => at,
        _ => return None,
    };
    at.as_deref().and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok()).map(|at| at.timestamp_millis())
}

/// Whether the index keeps this item at all (so the feeder copies only these).
pub fn is_kept(item: &ChatItem) -> bool {
    matches!(item, ChatItem::User { .. } | ChatItem::Assistant { .. } | ChatItem::ToolCall { .. })
}

/// The row for one item, or `None` for an item the index does not keep (tool output, usage, notices...) or one with
/// no text. A live item: `Secrets` strength, `MAX_TEXT_CHARS`.
pub fn entry_of(pane_id: &str, ctx: &PaneContext, item: &ChatItem, now_ms: i64) -> Option<Entry> {
    entry_of_with(pane_id, ctx, item, now_ms, Strength::Secrets, false, MAX_TEXT_CHARS)
}

/// Cleans one string at a given strength: the redaction first (`Strict` also masks email addresses), then the home
/// folder becomes `~`, then the snippet marks are removed. The one function every kept string goes through (item
/// text, folder label; the importer's labels too), so no path can skip it.
pub fn scrub_text(raw: &str, strength: Strength) -> String {
    let redact_ctx = crate::redact::known::context();
    let redacted = match strength {
        Strength::Secrets => crate::redact::text_with(raw, &redact_ctx).into_inner(),
        Strength::Strict => crate::redact::rules::mask_emails(crate::redact::strict_with(raw, &redact_ctx).as_str()),
    };
    clean(&home_to_tilde(&redacted, redact_ctx.home.as_deref()))
}

/// The row for one item at a given strength. Every string (text, folder) goes through the redaction first, then the
/// home folder becomes `~`, then the text is cut to `max_chars` (tool titles to their own, shorter limit).
/// `Strict` also masks email addresses. Imported rows are built with `imported = true`.
pub fn entry_of_with(
    pane_id: &str,
    ctx: &PaneContext,
    item: &ChatItem,
    now_ms: i64,
    strength: Strength,
    imported: bool,
    max_chars: usize,
) -> Option<Entry> {
    let (kind, at, raw, limit) = match item {
        ChatItem::User { at, text, .. } => ("user", at, text.clone(), max_chars),
        ChatItem::Assistant { at, text, .. } => ("assistant", at, text.clone(), max_chars),
        ChatItem::ToolCall { at, name, summary, .. } => ("tool", at, format!("{name} {summary}"), MAX_TITLE_CHARS.min(max_chars)),
        _ => return None,
    };
    let scrub = |raw: &str| scrub_text(raw, strength);
    let text = cut_to(scrub(raw.trim()).trim(), limit);
    if text.is_empty() {
        return None;
    }
    Some(Entry {
        pane_id: pane_id.to_string(),
        session: ctx.session.clone(),
        item_id: item.id().to_string(),
        kind,
        agent: ctx.agent.clone(),
        folder: scrub(ctx.folder.as_str()),
        at_ms: at_ms(at, now_ms),
        text,
        imported,
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
