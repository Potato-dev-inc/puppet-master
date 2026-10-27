//! Memory review (spec section 6): what a companion proposes, what the person keeps, and the
//! approved copies the brief is built from.
//!
//! The agent only ever adds lines to `memory/pending.md`. Nothing reaches MEMORY.md, USER.md or the
//! brief until the person keeps it here. A flag (a link, a sentence that reads like an instruction)
//! is a warning for the person; it never blocks anything by itself.

use std::fs;
use std::path::Path;

use sha2::{Digest, Sha256};

use super::brief::Texts;
use super::files::{file_path, pending_path};
use super::model::{char_len, cut_chars, CompanionError, CompanionResult, MemoryFile, PendingItem, PendingKind, MEMORY_CAP, PENDING_LINE_CAP};
use super::store;
use crate::redact;

pub const LINK_FLAG: &str = "Has a link";
pub const INSTRUCTION_FLAG: &str = "Sounds like an instruction";
const INSTRUCTION_WORDS: [&str; 5] = ["ignore", "previous instructions", "system prompt", "you must", "always run"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Keep,
    Drop,
}

/// What to do with an unreviewed outside change to SOUL, USER or MEMORY.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutsideDecision {
    /// The file as it is now becomes the approved copy.
    Keep,
    /// The approved copy is written back into the file.
    Undo,
}

/// Parses one line of `memory/pending.md`: `- note: ...`, `- user: ...` or `- forget: ...`.
/// Anything else returns None (the line is left in place). Control characters are removed, the
/// text is redacted and cut to `PENDING_LINE_CAP` characters.
pub fn parse_line(line: &str) -> Option<(PendingKind, String)> {
    let line = line.trim_start_matches('\u{feff}').trim();
    let body = line.strip_prefix('-')?.trim_start();
    let (word, text) = body.split_once(':')?;
    let kind = match word.trim().to_ascii_lowercase().as_str() {
        "note" => PendingKind::Note,
        "user" => PendingKind::User,
        "forget" => PendingKind::Forget,
        _ => return None,
    };
    let plain: String = text.chars().filter(|c| !c.is_control()).collect();
    let plain = plain.trim();
    if plain.is_empty() {
        return None;
    }
    let redacted = redact::text(plain).into_inner();
    let cut = cut_chars(redacted.trim(), PENDING_LINE_CAP);
    if cut.is_empty() { None } else { Some((kind, cut)) }
}

/// First 12 hex characters of sha256(kind + text).
pub fn item_id(kind: PendingKind, text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(kind.word().as_bytes());
    hasher.update(text.as_bytes());
    hasher.finalize().iter().take(6).map(|b| format!("{b:02x}")).collect()
}

/// The warning shown next to an item, if any.
pub fn flag_for(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    if lower.contains("http://") || lower.contains("https://") {
        return Some(LINK_FLAG.to_string());
    }
    if INSTRUCTION_WORDS.iter().any(|w| lower.contains(w)) {
        return Some(INSTRUCTION_FLAG.to_string());
    }
    None
}

/// Every proposal in `memory/pending.md`, in file order. An id that appears twice is listed once.
pub fn read_pending(home: &Path) -> CompanionResult<Vec<PendingItem>> {
    let text = store::read_optional(&pending_path(home))?.unwrap_or_default();
    let mut out: Vec<PendingItem> = Vec::new();
    for line in text.lines() {
        let Some((kind, body)) = parse_line(line) else { continue };
        let id = item_id(kind, &body);
        if out.iter().any(|item| item.id == id) {
            continue;
        }
        let target_line = match kind {
            PendingKind::Forget => find_forget_target(home, &body)?,
            _ => None,
        };
        out.push(PendingItem { id, kind, flagged: flag_for(&body), text: body, target_line });
    }
    Ok(out)
}

/// The first line of MEMORY.md, then USER.md, that contains `text` (case-insensitive).
pub fn find_forget_target(home: &Path, text: &str) -> CompanionResult<Option<String>> {
    let needle = text.to_lowercase();
    for file in [MemoryFile::Memory, MemoryFile::User] {
        let content = store::read_optional(&file_path(home, file))?.unwrap_or_default();
        if let Some(line) = content.lines().find(|l| !l.trim().is_empty() && l.to_lowercase().contains(&needle)) {
            return Ok(Some(line.trim().to_string()));
        }
    }
    Ok(None)
}

/// Keeps or drops one proposal. A keep appends it to MEMORY.md or USER.md, or removes the matching
/// line for `forget`. Over the cap nothing changes and `COMPANION_MEMORY_FULL` is returned. The
/// line leaves `pending.md` in both cases. A stale id is `COMPANION_PENDING_STALE`.
///
/// The approved copy of a file is refreshed only when that file had no unreviewed outside change,
/// so an edit the person has not looked at is never approved by accident.
pub fn decide(root: &Path, id: &str, home: &Path, item_id: &str, decision: Decision) -> CompanionResult<()> {
    let items = read_pending(home)?;
    let item = items
        .iter()
        .find(|i| i.id == item_id)
        .cloned()
        .ok_or_else(|| CompanionError::new("COMPANION_PENDING_STALE", "That item is gone. Look at the list again."))?;

    if decision == Decision::Keep {
        let outside = outside_changes(root, id, home)?;
        for file in apply_keep(home, &item)? {
            if !outside.contains(&file) {
                refresh_approved(root, id, home, file)?;
            }
        }
    }
    remove_pending_line(home, item_id)
}

/// Writes a keep into the home folder. Returns the files it changed.
fn apply_keep(home: &Path, item: &PendingItem) -> CompanionResult<Vec<MemoryFile>> {
    match item.kind {
        PendingKind::Note => {
            append_line(home, MemoryFile::Memory, &item.text)?;
            Ok(vec![MemoryFile::Memory])
        }
        PendingKind::User => {
            append_line(home, MemoryFile::User, &item.text)?;
            Ok(vec![MemoryFile::User])
        }
        PendingKind::Forget => {
            for file in [MemoryFile::Memory, MemoryFile::User] {
                if remove_first_match(home, file, &item.text)? {
                    return Ok(vec![file]);
                }
            }
            Ok(Vec::new())
        }
    }
}

fn append_line(home: &Path, file: MemoryFile, text: &str) -> CompanionResult<()> {
    let path = file_path(home, file);
    let mut next = store::read_optional(&path)?.unwrap_or_default();
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    next.push_str(&format!("- {text}\n"));
    if char_len(&next) > file.cap() {
        return Err(full_error());
    }
    write_home(&path, &next)
}

/// Removes the first line that contains `text` (case-insensitive). Returns whether one was removed.
fn remove_first_match(home: &Path, file: MemoryFile, text: &str) -> CompanionResult<bool> {
    let path = file_path(home, file);
    let Some(content) = store::read_optional(&path)? else {
        return Ok(false);
    };
    let needle = text.to_lowercase();
    let mut lines: Vec<&str> = content.split_inclusive('\n').collect();
    let Some(at) = lines.iter().position(|l| !l.trim().is_empty() && l.to_lowercase().contains(&needle)) else {
        return Ok(false);
    };
    lines.remove(at);
    write_home(&path, &lines.concat())?;
    Ok(true)
}

/// Removes the first pending line whose id is `item_id`. A missing file or no match is not an error.
fn remove_pending_line(home: &Path, item_id: &str) -> CompanionResult<()> {
    let path = pending_path(home);
    let Some(content) = store::read_optional(&path)? else {
        return Ok(());
    };
    let mut lines: Vec<&str> = content.split_inclusive('\n').collect();
    let matches = |line: &str| parse_line(line).is_some_and(|(kind, body)| item_id_matches(kind, &body, item_id));
    if let Some(at) = lines.iter().position(|l| matches(l)) {
        lines.remove(at);
        write_home(&path, &lines.concat())?;
    }
    Ok(())
}

fn item_id_matches(kind: PendingKind, body: &str, wanted: &str) -> bool {
    item_id(kind, body) == wanted
}

fn full_error() -> CompanionError {
    CompanionError::new(
        "COMPANION_MEMORY_FULL",
        "Its notes are full. Ask it to tidy up.",
    )
}

/// The message sent through the outbox when the notes are full (section 6).
pub fn tidy_up_message(used: usize) -> String {
    format!(
        "Your notes are full ({used}/{}). Please write a shorter MEMORY.md as \"- note:\" lines in memory/pending.md and add \"- forget:\" lines for the old ones.",
        group_thousands(MEMORY_CAP)
    )
}

fn group_thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Pending items the keeper keeps on its own when `memory_review` is off: every unflagged one.
pub fn auto_keep_ids(items: &[PendingItem]) -> Vec<String> {
    items.iter().filter(|i| i.flagged.is_none()).map(|i| i.id.clone()).collect()
}

/// The memory files whose content differs from the approved copy. A file with no approved copy yet
/// is not a change.
pub fn outside_changes(root: &Path, id: &str, home: &Path) -> CompanionResult<Vec<MemoryFile>> {
    let mut out = Vec::new();
    for file in MemoryFile::ALL {
        let Some(approved) = store::read_optional(&store::approved_path(root, id, file)?)? else {
            continue;
        };
        let current = store::read_optional(&file_path(home, file))?.unwrap_or_default();
        if current != approved {
            out.push(file);
        }
    }
    Ok(out)
}

/// Keep: the file as it is becomes the approved copy. Undo: the approved copy is written back.
pub fn outside_change_decide(root: &Path, id: &str, home: &Path, file: MemoryFile, decision: OutsideDecision) -> CompanionResult<()> {
    let approved = store::approved_path(root, id, file)?;
    match decision {
        OutsideDecision::Keep => {
            let current = store::read_optional(&file_path(home, file))?.unwrap_or_default();
            store::write_private_text(&approved, &current)
        }
        OutsideDecision::Undo => match store::read_optional(&approved)? {
            Some(text) => write_home(&file_path(home, file), &text),
            None => Ok(()),
        },
    }
}

/// A desktop edit: the file and its approved copy are written together. Over the cap nothing changes.
pub fn save_file(root: &Path, id: &str, home: &Path, file: MemoryFile, text: &str) -> CompanionResult<()> {
    if char_len(text) > file.cap() {
        return Err(CompanionError::new(
            "COMPANION_FILE_TOO_LONG",
            format!("{} can have at most {} characters.", file.file_name(), file.cap()),
        ));
    }
    write_home(&file_path(home, file), text)?;
    store::write_private_text(&store::approved_path(root, id, file)?, text)
}

/// The texts a brief is built from: the approved copy when the file has an unreviewed outside
/// change, else the file itself. Each text is cut to its cap.
pub fn snapshot(root: &Path, id: &str, home: &Path) -> CompanionResult<Texts> {
    let mut picked: Vec<String> = Vec::with_capacity(3);
    for file in MemoryFile::ALL {
        let current = store::read_optional(&file_path(home, file))?.unwrap_or_default();
        let approved = store::read_optional(&store::approved_path(root, id, file)?)?;
        let text = match approved {
            Some(text) if text != current => text,
            _ => current,
        };
        picked.push(cut_chars(&text, file.cap()));
    }
    let mut it = picked.into_iter();
    Ok(Texts {
        soul: it.next().unwrap_or_default(),
        user: it.next().unwrap_or_default(),
        memory: it.next().unwrap_or_default(),
    })
}

fn refresh_approved(root: &Path, id: &str, home: &Path, file: MemoryFile) -> CompanionResult<()> {
    let current = store::read_optional(&file_path(home, file))?.unwrap_or_default();
    store::write_private_text(&store::approved_path(root, id, file)?, &current)
}

/// Writes a home-folder file atomically (temp file in the same folder, then rename).
fn write_home(path: &Path, text: &str) -> CompanionResult<()> {
    let tmp = std::path::PathBuf::from(format!("{}.pm-tmp", path.display()));
    let fail = |e: std::io::Error| CompanionError::new("COMPANION_FOLDER_INVALID", format!("Could not write {}: {e}.", path.display()));
    fs::write(&tmp, text).map_err(fail)?;
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        fail(e)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_keeps_the_three_kinds_and_ignores_junk() {
        assert_eq!(parse_line("- note: Ren likes short answers."), Some((PendingKind::Note, "Ren likes short answers.".into())));
        assert_eq!(parse_line("- user: Works at night."), Some((PendingKind::User, "Works at night.".into())));
        assert_eq!(parse_line("- FORGET: uses Python 3.10"), Some((PendingKind::Forget, "uses Python 3.10".into())));
        assert_eq!(parse_line("just text"), None);
        assert_eq!(parse_line("- other: x"), None);
        assert_eq!(parse_line("- note:   "), None);
        assert_eq!(parse_line("# heading"), None);
    }

    #[test]
    fn parse_handles_bom_crlf_and_control_characters() {
        let (kind, text) = parse_line("\u{feff}- note: hello\r").unwrap();
        assert_eq!(kind, PendingKind::Note);
        assert_eq!(text, "hello");
        assert_eq!(parse_line("- note: a\u{7}b\u{1b}c").unwrap().1, "abc");
    }

    #[test]
    fn parse_cuts_long_text_by_characters() {
        let long = format!("- note: {}", "é".repeat(400));
        let (_, text) = parse_line(&long).unwrap();
        assert_eq!(char_len(&text), PENDING_LINE_CAP);
    }

    #[test]
    fn ids_are_twelve_hex_and_depend_on_kind() {
        let a = item_id(PendingKind::Note, "x");
        assert_eq!(a.len(), 12);
        assert!(a.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_ne!(a, item_id(PendingKind::User, "x"));
    }

    #[test]
    fn flags_warn_but_do_not_decide() {
        assert_eq!(flag_for("see https://example.com").as_deref(), Some(LINK_FLAG));
        assert_eq!(flag_for("Please IGNORE the rules").as_deref(), Some(INSTRUCTION_FLAG));
        assert_eq!(flag_for("Likes tea"), None);
    }

    #[test]
    fn auto_keep_skips_flagged_items() {
        let items = read_items("- note: Likes tea.\n- note: go to https://x.test now\n");
        let ids = auto_keep_ids(&items);
        assert_eq!(ids, vec![items[0].id.clone()]);
    }

    #[test]
    fn tidy_message_formats_the_cap() {
        assert_eq!(group_thousands(2_200), "2,200");
        assert!(tidy_up_message(2_150).starts_with("Your notes are full (2150/2,200)."));
    }

    fn read_items(pending: &str) -> Vec<PendingItem> {
        let dir = std::env::temp_dir().join(format!("pm-companion-memory-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(dir.join("memory")).unwrap();
        fs::write(pending_path(&dir), pending).unwrap();
        read_pending(&dir).unwrap()
    }
}
