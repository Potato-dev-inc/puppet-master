//! The check before a found table is written: the real reader (`session_chat::declared`) runs over the captured
//! lines and must yield the task as the user's message and the reply as the assistant's, and the folder template
//! must lead to the very file again.

use std::path::Path;
use std::time::SystemTime;

use serde::Deserialize;

use super::leaves::norm;
use super::scan::Homes;
use crate::session_chat::{ChatItem, DeclaredSource, Located, Names, PaneFacts, SessionChatTable, SessionFormat, SessionSource};

#[derive(Deserialize)]
struct Wrap {
    session_chat: SessionChatTable,
}

/// What the table has to do with the captured lines.
pub struct Check<'a> {
    pub prompt: &'a str,
    /// The assistant's message holds one of these (`infer::needles`).
    pub reply: &'a [String],
    /// A tool call by this name must come out of the lines.
    pub expect_tool: Option<&'a str>,
    pub cwd: &'a Path,
    pub homes: &'a Homes,
    /// Before the pane started.
    pub since: SystemTime,
    pub file: &'a Path,
}

/// `Ok` when `text` (a `[session_chat]` table) reads `lines` as the check says.
pub fn verify(text: &str, lines: &[String], check: &Check) -> Result<(), String> {
    let wrap: Wrap = toml::from_str(text).map_err(|e| format!("the table does not parse: {e}"))?;
    let source = DeclaredSource::new(wrap.session_chat).map_err(|errors| {
        let first = errors.first().map(|e| format!("{}: {}", e.code, e.message)).unwrap_or_default();
        format!("the table is not valid: {first}")
    })?;
    let mut items: Vec<ChatItem> = Vec::new();
    let mut offset = 0u64;
    for line in lines {
        items.extend(source.format().parse_line(line, offset));
        offset += line.len() as u64 + 1;
    }
    let (prompt, reply): (String, Vec<String>) = (norm(check.prompt), check.reply.iter().map(|r| norm(r)).collect());
    let user = items.iter().any(|i| matches!(i, ChatItem::User { text, .. } if norm(text).contains(&prompt)));
    if !user {
        return Err("the reader does not give the wizard's message as a user message".into());
    }
    let assistant = items.iter().any(|i| matches!(i, ChatItem::Assistant { text, .. } if reply.iter().any(|r| norm(text).contains(r))));
    if !assistant {
        return Err("the reader does not give the reply the wizard read as an assistant message".into());
    }
    if let Some(name) = check.expect_tool {
        if !items.iter().any(|i| matches!(i, ChatItem::ToolCall { name: n, .. } if n == name)) {
            return Err("the reader does not give the tool call".into());
        }
    }
    let names = Names { home: check.homes.home.as_deref(), cwd: check.cwd, env: &|_| None };
    let pane = PaneFacts { cwd: check.cwd.to_path_buf(), started_at: check.since, provider_session_id: None, home: None, launch: Default::default() };
    let real = std::fs::canonicalize(check.file).map(|p| crate::session_chat::plain_path(&p.to_string_lossy())).unwrap_or_default();
    match source.locate_with(&pane, &names) {
        Located::Found(found) if crate::session_chat::plain_path(&found.to_string_lossy()).eq_ignore_ascii_case(&real) => Ok(()),
        _ => Err("the folder the table names does not lead to the transcript file again".into()),
    }
}
