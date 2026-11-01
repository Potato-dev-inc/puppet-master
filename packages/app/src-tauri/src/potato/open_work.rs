//! Open work: the bot's own list of what it promised to do (spec P7). One Markdown file in the bot's vault,
//! `nodes/open-work.md`, so the brain's own memory search can find it:
//!
//! ```text
//! # Open work
//!
//! - [ ] Send the weekly summary to Sam ^ow-3f9a1c2e
//! - [x] Check the build ^ow-0b7d44aa
//! ```
//!
//! A line with an id is an item. Every other line is kept exactly as it is, so a person can edit the file. The
//! helper jobs still running are not stored here: the list reads them from the helpers' book (`helper_rows`).
//! Writes take the store lock and go through `secure_file::write_private`. A damaged (not UTF-8) file is an error
//! and is never overwritten.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::helpers::Running;
use super::store::{self, files_lock, StoreError, StoreResult};
use crate::secure_file;

/// The file, relative to the bot's vault.
pub const REL_PATH: &str = "nodes/open-work.md";
const HEADER: &str = "# Open work";
/// Items per bot, finished ones included.
pub const MAX_ITEMS: usize = 200;
/// Characters per item, after the text is made one line.
pub const MAX_TEXT_CHARS: usize = 200;
const INVALID: &str = "POTATO_OPEN_WORK_INVALID";
const NOT_FOUND: &str = "POTATO_OPEN_WORK_NOT_FOUND";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    /// `ow-` and eight lowercase hex digits.
    pub id: String,
    pub text: String,
    pub done: bool,
}

/// A helper job still running for this bot. Read-only: it ends by itself.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HelperRow {
    pub request_id: String,
    /// The project's name (never a path).
    pub project: String,
    pub started_ms: i64,
}

/// The running helpers that belong to one bot.
pub fn helper_rows(running: &[Running], bot: &str) -> Vec<HelperRow> {
    running
        .iter()
        .filter(|r| r.bot == bot)
        .map(|r| HelperRow { request_id: r.request_id.clone(), project: r.project.clone(), started_ms: r.started_ms })
        .collect()
}

/// The file of one bot (the bot must exist: the caller checks).
pub fn path(root: &Path, bot: &str) -> StoreResult<PathBuf> {
    Ok(store::vault_dir(root, bot)?.join("nodes").join("open-work.md"))
}

/// `ow-` and eight lowercase hex digits, and nothing else.
pub fn is_id(id: &str) -> bool {
    id.len() == 11 && id.starts_with("ow-") && id[3..].bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn new_id() -> String {
    format!("ow-{}", &uuid::Uuid::new_v4().simple().to_string()[..8])
}

/// The text of an item: one line, no secrets, at most `MAX_TEXT_CHARS` characters.
pub fn clean_text(raw: &str) -> String {
    let one_line = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    store::clean_text(&one_line, None).chars().take(MAX_TEXT_CHARS).collect()
}

/// One line as an item, or `None` for any other line (a heading, a note, a line a person wrote).
pub fn parse_line(line: &str) -> Option<Item> {
    let (done, rest) = if let Some(rest) = line.strip_prefix("- [ ] ") {
        (false, rest)
    } else if let Some(rest) = line.strip_prefix("- [x] ").or_else(|| line.strip_prefix("- [X] ")) {
        (true, rest)
    } else {
        return None;
    };
    let at = rest.rfind(" ^ow-")?;
    let id = &rest[at + 2..];
    if !is_id(id) {
        return None;
    }
    Some(Item { id: id.to_string(), text: rest[..at].to_string(), done })
}

fn render(item: &Item) -> String {
    format!("- [{}] {} ^{}", if item.done { "x" } else { " " }, item.text, item.id)
}

fn read(path: &Path) -> StoreResult<String> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(StoreError::new(INVALID, format!("Could not read the open work list: {e}."))),
    }
}

fn write(path: &Path, lines: &[String]) -> StoreResult<()> {
    if let Some(parent) = path.parent() {
        secure_file::create_private_dir(parent).map_err(|e| StoreError::new(INVALID, format!("Could not make the open work folder: {e}.")))?;
    }
    let mut text = String::new();
    for line in lines {
        text.push_str(line);
        text.push('\n');
    }
    secure_file::write_private(path, &text).map_err(|e| StoreError::new(INVALID, e))
}

fn not_found() -> StoreError {
    StoreError::new(NOT_FOUND, "That item is not on the list any more.")
}

/// The items in file order.
pub fn list(root: &Path, bot: &str) -> StoreResult<Vec<Item>> {
    store::require_bot(root, bot)?;
    let text = read(&path(root, bot)?)?;
    Ok(text.lines().filter_map(parse_line).collect())
}

/// Adds an item at the end of the list.
pub fn add(root: &Path, bot: &str, text: &str) -> StoreResult<Item> {
    let _files = files_lock();
    store::require_bot(root, bot)?;
    let clean = clean_text(text);
    if clean.is_empty() {
        return Err(StoreError::new("POTATO_OPEN_WORK_EMPTY", "Write what the bot should do first."));
    }
    let file = path(root, bot)?;
    let mut lines: Vec<String> = read(&file)?.lines().map(str::to_string).collect();
    if lines.iter().filter_map(|l| parse_line(l)).count() >= MAX_ITEMS {
        return Err(StoreError::new("POTATO_OPEN_WORK_FULL", format!("The list is full ({MAX_ITEMS} items). Finish or remove some first.")));
    }
    if lines.is_empty() {
        lines.push(HEADER.to_string());
        lines.push(String::new());
    }
    let item = Item { id: new_id(), text: clean, done: false };
    lines.push(render(&item));
    write(&file, &lines)?;
    Ok(item)
}

/// Marks an item done or open again. The other lines stay as they are.
pub fn set_done(root: &Path, bot: &str, id: &str, done: bool) -> StoreResult<Item> {
    let _files = files_lock();
    store::require_bot(root, bot)?;
    if !is_id(id) {
        return Err(not_found());
    }
    let file = path(root, bot)?;
    let mut lines: Vec<String> = read(&file)?.lines().map(str::to_string).collect();
    let mut changed = None;
    for line in lines.iter_mut() {
        if let Some(mut item) = parse_line(line.as_str()) {
            if item.id == id {
                item.done = done;
                *line = render(&item);
                changed = Some(item);
                break;
            }
        }
    }
    let item = changed.ok_or_else(not_found)?;
    write(&file, &lines)?;
    Ok(item)
}

/// Removes an item's line. The other lines stay as they are.
pub fn remove(root: &Path, bot: &str, id: &str) -> StoreResult<()> {
    let _files = files_lock();
    store::require_bot(root, bot)?;
    if !is_id(id) {
        return Err(not_found());
    }
    let file = path(root, bot)?;
    let mut lines: Vec<String> = read(&file)?.lines().map(str::to_string).collect();
    let before = lines.len();
    lines.retain(|line| parse_line(line).map_or(true, |item| item.id != id));
    if lines.len() == before {
        return Err(not_found());
    }
    write(&file, &lines)
}
