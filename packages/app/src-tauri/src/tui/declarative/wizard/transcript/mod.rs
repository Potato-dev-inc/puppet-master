//! "The wizard finds the transcript": after the visible test's turn in the wizard's temporary folder, look for a
//! JSONL file the program created or grew during that turn, work out which JSON fields hold the roles and the
//! text, check the guess with the real reader, and offer a `[session_chat]` table for the draft.
//!
//! What may be looked at is fixed (`scan`): the pane's own temporary folder, and under the person's home only
//! `~/.{name}`, `~/.config/{name}`, `%APPDATA%/{name}` and `%LOCALAPPDATA%/{name}`, depth-limited, with a cap on the
//! entries listed and on the bytes read, and only the lines that were appended during the turn. Nothing leaves this
//! module but the table (pointers, a folder template, the role words): never a line of the transcript.
//! Generic: no program is named here.

mod infer;
mod leaves;
mod place;
mod render;
mod scan;
mod verify;

#[cfg(test)]
mod tests;

pub use render::MARK;
pub use scan::{clean_name, Homes, Roots, Snapshot};

use std::path::Path;
use std::time::SystemTime;

use infer::{infer, Missing};
use scan::Appended;
use verify::Check;

/// A table that passed the check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// The `[session_chat]` tables as TOML, with the `found by the wizard` comment on top.
    pub toml: String,
    /// The `dir` template (`{home}/.x/{cwd_slug}`): safe to show, it holds no real folder name.
    pub dir: String,
    /// Tool calls and results are mapped too.
    pub tool: bool,
}

/// What the wizard keeps of its search for one draft: the table, or why there is none.
#[derive(Debug, Clone)]
pub struct Finding {
    pub draft_sha256: String,
    pub result: Result<Found, String>,
}

impl Finding {
    /// The record for `<id>.wizard.json` and the visible test's reply.
    pub fn to_json(&self) -> serde_json::Value {
        match &self.result {
            Ok(found) => serde_json::json!({"found": true, "dir": found.dir, "tool_lines": found.tool}),
            Err(why) => serde_json::json!({"found": false, "why": why}),
        }
    }
}

/// What a search is given.
pub struct Search<'a> {
    pub roots: &'a Roots,
    /// The files before the turn.
    pub before: &'a Snapshot,
    /// When the pane started.
    pub since: SystemTime,
    /// The task the wizard typed, the reply it read off the screen and the random word the task asked for.
    pub prompt: &'a str,
    pub reply: &'a str,
    pub word: &'a str,
}

/// Why nothing was found. `retry`: a file did change, so its last lines may simply not be written yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotFound {
    pub why: String,
    pub retry: bool,
}

/// Looks for the transcript of the turn that has just ended.
pub fn find(search: &Search) -> Result<Found, NotFound> {
    let after = Snapshot::take(search.roots);
    let appended = search.before.appended(&after);
    if appended.is_empty() {
        let why = if after.cut_short {
            "too many files in the folders the wizard may look in, so it stopped looking before it found one that changed"
        } else {
            "no .jsonl file was created or grew during the test turn in the folders the wizard may look in"
        };
        return Err(NotFound { why: why.to_string(), retry: false });
    }
    let mut best: Option<(Missing, String)> = None;
    for file in &appended {
        match from_file(search, file) {
            Ok(found) => return Ok(found),
            Err(failure) => {
                if best.as_ref().is_none_or(|(rank, _)| failure.0 > *rank) {
                    best = Some(failure);
                }
            }
        }
    }
    let why = best.map(|(_, why)| why).unwrap_or_default();
    Err(NotFound { why: format!("{} .jsonl file(s) changed during the test turn, but {why}", appended.len()), retry: true })
}

/// `find`, asked again a few times while a file has changed but does not yet show the whole turn (a program may write its
/// last lines a moment after the screen shows the reply).
pub fn find_with_retries(search: &Search) -> Result<Found, String> {
    let mut tries = 0;
    loop {
        match find(search) {
            Ok(found) => return Ok(found),
            Err(not_found) if not_found.retry && tries < RETRIES => {
                tries += 1;
                std::thread::sleep(RETRY_AFTER);
            }
            Err(not_found) => return Err(not_found.why),
        }
    }
}

const RETRIES: u32 = 3;
const RETRY_AFTER: std::time::Duration = std::time::Duration::from_millis(250);

/// The table for one changed file, or how far the search got with it.
fn from_file(search: &Search, file: &Appended) -> Result<Found, (Missing, String)> {
    let cwd = search.roots.cwd.as_path();
    let reply = infer::needles(search.reply, search.word);
    let mapping = infer(&file.lines, search.prompt, &reply).map_err(|missing| (missing, missing.why().to_string()))?;
    let reached = Missing::Role;
    let dir = file
        .file
        .parent()
        .and_then(|dir| place::dir_template(dir, cwd, search.roots.homes.home.as_deref()))
        .ok_or((reached, "the file is not under the temporary folder or the home folder".to_string()))?;
    let try_with = |with_tool: bool| -> Result<String, String> {
        let text = render::render(&dir, &mapping, with_tool);
        let expect_tool = mapping.tool.as_ref().filter(|_| with_tool).map(|t| t.sample_name.as_str());
        let check = Check {
            prompt: search.prompt,
            reply: &reply,
            expect_tool,
            cwd,
            homes: &search.roots.homes,
            since: search.since,
            file: &file.file,
        };
        verify::verify(&text, &file.lines, &check).map(|()| text)
    };
    if mapping.tool.is_some() {
        if let Ok(toml) = try_with(true) {
            return Ok(Found { toml, dir, tool: true });
        }
    }
    try_with(false).map(|toml| Found { toml, dir, tool: false }).map_err(|why| (reached, why))
}

/// `text` (a draft definition) with `found` added at the end, or `None` when the draft already declares a
/// `[session_chat]` (the drafter's own table is never replaced) or the result is not a valid definition.
pub fn add_to_draft(text: &str, found: &Found) -> Option<String> {
    let table: toml::Table = text.parse().ok()?;
    if table.contains_key("session_chat") {
        return None;
    }
    let mut out = text.trim_end().to_string();
    out.push_str("\n\n");
    out.push_str(&found.toml);
    crate::tui::declarative::schema::parse_definition(&out).is_ok().then_some(out)
}

/// The names the home patterns use: the typed command, the program's file and the definition's id.
pub fn names_for(command: &str, program: &Path, id: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in [command, &program.to_string_lossy(), id] {
        if let Some(name) = clean_name(raw) {
            if !out.contains(&name) {
                out.push(name);
            }
        }
    }
    out
}
