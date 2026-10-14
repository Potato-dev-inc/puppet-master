//! Projection (§4.3): a vendor hook payload becomes a [`HookEvent`] through the agent's `[hooks]`
//! field selectors. Only the declared fields are read; everything else in the payload is dropped
//! here, in memory. Generic: no vendor field name appears in this file except through the table.

use serde_json::Value;
use sha2::{Digest, Sha256};

use super::model::{HookDetail, HookEvent, HookKind};
use crate::tui::declarative::hooks::HooksTable;
use crate::tui::declarative::select::walk;

pub const MAX_ID: usize = 128;
pub const MAX_SHORT: usize = 200;
pub const MAX_COMMAND: usize = 2 * 1024;
pub const MAX_FINAL_TEXT: usize = 64 * 1024;
pub const MAX_PATH: usize = 512;

/// Why a payload produced no event (counted, never an error to the emitter).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dropped {
    /// The emitter could not parse the vendor's stdin.
    Unparsed,
    /// No `event_field`, or an event the table does not subscribe.
    UnknownEvent,
}

/// Project one vendor payload. `seq` and `at_ms` come from the caller (the ledger).
pub fn project(
    scheme: &HooksTable,
    payload: &Value,
    pane_id: &str,
    agent: &str,
    project_dir: &str,
    seq: u64,
    at_ms: u64,
) -> Result<HookEvent, Dropped> {
    if payload.get("unparsed").and_then(Value::as_bool) == Some(true) {
        return Err(Dropped::Unparsed);
    }
    let vendor_event = walk(payload, &scheme.event_field).and_then(Value::as_str).ok_or(Dropped::UnknownEvent)?;
    let row = scheme.row_for(vendor_event).ok_or(Dropped::UnknownEvent)?;
    let kind = HookKind::parse(&row.pm).ok_or(Dropped::UnknownEvent)?;
    let field = |name: &str| row.fields.get(name).and_then(|path| walk(payload, path));
    let text = |name: &str| field(name).and_then(scalar_text);
    let short = |name: &str, max: usize| text(name).map(|s| cut_chars(&sanitize(&s), max));

    let mut detail = HookDetail {
        source: short("source", MAX_SHORT),
        cwd: short("cwd", MAX_PATH),
        model: short("model", MAX_SHORT),
        tool: short("tool", MAX_SHORT),
        tool_use: short("tool_use", MAX_ID),
        error: short("error", MAX_SHORT),
        reason: short("reason", MAX_SHORT),
        kind: short("kind", MAX_SHORT),
        continuing: field("continuing").and_then(Value::as_bool),
        ..HookDetail::default()
    };
    let mut paths: Vec<String> = text("path").into_iter().collect();
    if let Some(Value::Array(items)) = field("paths") {
        paths.extend(items.iter().filter_map(scalar_text));
    }
    detail.paths = paths.iter().take(64).map(|p| project_path(p, project_dir)).collect();
    if let Some(prompt) = text("prompt") {
        detail.prompt_len = Some(prompt.chars().count());
        detail.prompt_sha = Some(prompt_sha(&prompt));
    }
    if let Some(reply) = text("final_text") {
        detail.final_text_len = Some(reply.chars().count());
        detail.final_text = Some(crate::redact::text(&cut_bytes(&sanitize(&reply), MAX_FINAL_TEXT)));
    }
    if let Some(command) = text("command").or_else(|| text("description")) {
        detail.command_len = Some(command.chars().count());
        detail.command = Some(crate::redact::text(&cut_bytes(&sanitize(&command), MAX_COMMAND)));
    }
    Ok(HookEvent {
        seq,
        at_ms,
        pane_id: pane_id.to_string(),
        agent: agent.to_string(),
        kind,
        vendor_event: cut_chars(vendor_event, 64),
        session: text("session").map(|s| cut_chars(&s, MAX_ID)),
        turn_key: text("turn").map(|s| cut_chars(&s, MAX_ID)),
        subagent: text("subagent").filter(|s| !s.is_empty()).map(|s| cut_chars(&s, MAX_ID)),
        foreign: false,
        applied: false,
        detail,
    })
}

fn scalar_text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// Control and invisible/bidi formatting characters out (the push notifier's rule).
pub fn sanitize(text: &str) -> String {
    crate::push::triggers::sanitize(text)
}

pub fn cut_chars(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

fn cut_bytes(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

/// A path inside the pane's project stays as it is; one outside is reduced to its file name, so
/// a hook can't put the shape of the user's disk on a receipt.
pub fn project_path(path: &str, project_dir: &str) -> String {
    let norm = |p: &str| p.replace('\\', "/").trim_end_matches('/').to_ascii_lowercase();
    let (p, root) = (norm(path), norm(project_dir));
    let path = cut_chars(&sanitize(path), MAX_PATH);
    if !root.is_empty() && (p == root || p.starts_with(&format!("{root}/"))) {
        return path;
    }
    let relative = !p.starts_with('/') && !p.starts_with('~') && p.get(1..2) != Some(":") && !p.split('/').any(|s| s == "..");
    if relative {
        return path;
    }
    let name = path.replace('\\', "/").rsplit('/').next().unwrap_or_default().to_string();
    format!("{name} (outside the project)")
}

/// `normalize(prompt)` (§6.4): trim, CRLF to LF, Claude's `<pasted_content ...>` wrapper lines out,
/// whitespace runs collapsed.
pub fn normalize_prompt(prompt: &str) -> String {
    prompt
        .replace("\r\n", "\n")
        .lines()
        .filter(|line| {
            let t = line.trim();
            !(t.starts_with("<pasted_content") || t.starts_with("</pasted_content"))
        })
        .collect::<Vec<_>>()
        .join("\n")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn prompt_sha(prompt: &str) -> String {
    let digest = Sha256::digest(normalize_prompt(prompt).as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}
