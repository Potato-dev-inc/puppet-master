//! What the bot remembers (spec P7): the notes in its vault, `brain/<bot>/vault/`. The list is read-only, and the one
//! change is Delete (the panel asks first). The notes are the folders the brain's own memory search reads (`journal`,
//! `diary`, `nodes`), minus the open-work file. `state` and `sessions` are the brain's own bookkeeping, not notes.
//!
//! Every path comes from the list and is checked again before a read or a delete: a `.md` file under one of those
//! folders, with no `..`, no drive or absolute part, no dot names, and no link anywhere on the way.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::Serialize;

use super::open_work;
use super::store::{self, files_lock, StoreError, StoreResult};

/// The vault folders that hold what the bot remembers.
pub const FOLDERS: [&str; 3] = ["journal", "diary", "nodes"];
/// Notes listed at most (newest first).
pub const MAX_NOTES: usize = 500;
/// Bytes shown for one note.
pub const MAX_VIEW_BYTES: u64 = 256 * 1024;
const MAX_DEPTH: usize = 6;
const NOT_FOUND: &str = "POTATO_NOTE_NOT_FOUND";

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    /// Relative to the vault, with `/`: `journal/2026/10/2026-10-28.md`.
    pub path: String,
    pub bytes: u64,
    pub modified_ms: i64,
}

fn not_found() -> StoreError {
    StoreError::new(NOT_FOUND, "That note is not in this bot's memory.")
}

/// The notes of one bot, newest first.
pub fn list(root: &Path, bot: &str) -> StoreResult<Vec<Note>> {
    store::require_bot(root, bot)?;
    let vault = store::vault_dir(root, bot)?;
    let mut notes = Vec::new();
    for folder in FOLDERS {
        walk(&vault.join(folder), folder, 0, &mut notes);
    }
    notes.retain(|note| note.path != open_work::REL_PATH);
    notes.sort_by(|a, b| b.modified_ms.cmp(&a.modified_ms).then_with(|| a.path.cmp(&b.path)));
    notes.truncate(MAX_NOTES);
    Ok(notes)
}

fn walk(dir: &Path, rel: &str, depth: usize, out: &mut Vec<Note>) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_symlink() {
            continue;
        }
        let child = format!("{rel}/{name}");
        if kind.is_dir() {
            walk(&entry.path(), &child, depth + 1, out);
        } else if kind.is_file() && name.ends_with(".md") {
            let Ok(meta) = entry.metadata() else { continue };
            let modified_ms = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as i64);
            out.push(Note { path: child, bytes: meta.len(), modified_ms });
        }
    }
}

/// The file a note path names, checked. Refused (as "not found") unless it is a `.md` file under a note folder,
/// every part is a plain name, and no part on the way is a link.
pub fn checked_path(root: &Path, bot: &str, rel: &str) -> StoreResult<PathBuf> {
    let parts: Vec<&str> = rel.split('/').collect();
    let plain = |p: &&str| !p.is_empty() && *p != "." && *p != ".." && !p.starts_with('.') && !p.contains(['\\', ':', '\0']);
    let shaped = rel.len() <= 300 && parts.len() >= 2 && FOLDERS.contains(&parts[0]) && parts.iter().all(plain) && rel.ends_with(".md") && rel != open_work::REL_PATH;
    if !shaped {
        return Err(not_found());
    }
    store::require_bot(root, bot)?;
    let mut at = store::vault_dir(root, bot)?;
    let last = parts.len() - 1;
    for (i, part) in parts.iter().enumerate() {
        at.push(*part);
        let meta = fs::symlink_metadata(&at).map_err(|_| not_found())?;
        if meta.file_type().is_symlink() {
            return Err(not_found());
        }
        let want_file = i == last;
        if (want_file && !meta.is_file()) || (!want_file && !meta.is_dir()) {
            return Err(not_found());
        }
    }
    Ok(at)
}

/// The text of one note, redacted like every other text the panel shows. A long note is cut at 256 KB, with a line
/// that says so.
pub fn read(root: &Path, bot: &str, rel: &str) -> StoreResult<String> {
    let path = checked_path(root, bot, rel)?;
    let file = fs::File::open(&path).map_err(|_| not_found())?;
    let total = file.metadata().map(|m| m.len()).unwrap_or(0);
    let mut buf = Vec::new();
    file.take(MAX_VIEW_BYTES).read_to_end(&mut buf).map_err(|e| StoreError::new("POTATO_NOTE_UNREADABLE", format!("Could not read the note: {e}.")))?;
    let mut text = String::from_utf8_lossy(&buf).replace("\r\n", "\n");
    if total > MAX_VIEW_BYTES {
        text.push_str("\n\n[Cut short: the note is longer than 256 KB.]");
    }
    Ok(crate::companion::view::scrub(&text, None))
}

/// Deletes one note. The person asked for it (the panel confirms first); nothing else is touched.
pub fn delete(root: &Path, bot: &str, rel: &str) -> StoreResult<()> {
    let _files = files_lock();
    let path = checked_path(root, bot, rel)?;
    fs::remove_file(&path).map_err(|e| StoreError::new("POTATO_NOTE_UNREADABLE", format!("Could not delete the note: {e}.")))
}
