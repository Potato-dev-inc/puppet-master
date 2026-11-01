//! Files a person attaches to a message in a bot's thread (pictures and text files).
//!
//! The brain's protocol carries text only, so an attachment travels as text: the file is saved in the bot's own
//! folder (`<app data>/potato/brain/<bot>/attachments/`) and the message says so ("[Attached: notes.txt (2 KB), saved
//! as attachments/..]"). A text file that fits is also quoted in the message, so the bot can read it at once. File
//! contents go nowhere but that folder and that message (which only the bot's own brain receives). Nothing here
//! opens a network connection or a path the caller names: the folder is fixed, the file name is cleaned, and what
//! kind of file it is comes from its first bytes, never from the name the page claims.

use std::path::{Path, PathBuf};

use base64::Engine as _;
use serde::Deserialize;

use super::store::{self, StoreError, MAX_MESSAGE_CHARS};

/// Files in one message.
pub const MAX_FILES: usize = 4;
/// One file, in bytes (the same 2 MiB the store uses for its own files).
pub const MAX_FILE_BYTES: usize = 2 * 1024 * 1024;
/// All files of one message together.
pub const MAX_TOTAL_BYTES: usize = 4 * 1024 * 1024;
/// The folder inside the bot's brain folder.
pub const FOLDER: &str = "attachments";
/// Below this many characters of room left, a text file is not quoted (the saved file is still named).
const MIN_QUOTE: usize = 60;

/// One file as the page sends it. `data` is base64. `kind` is only a hint; the bytes decide.
#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Incoming {
    pub name: String,
    #[serde(default)]
    pub kind: String,
    pub data: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Image,
    Text,
}

/// A file after it is checked and saved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Saved {
    /// The cleaned name the person sees.
    pub name: String,
    /// Where it is, relative to the bot's folder: `attachments/<stamp>-<n>-<name>`.
    pub rel: String,
    pub kind: Kind,
    pub bytes: usize,
    /// The text of a text file.
    pub text: Option<String>,
}

fn bad(message: impl Into<String>) -> StoreError {
    StoreError::new("POTATO_ATTACHMENT", message)
}

/// The last part of a path, with anything but letters, digits, `.`, `-`, `_` and spaces turned into `_`; never
/// empty, never starting with a dot, at most 60 characters (the end is kept, so the extension stays).
pub fn safe_name(raw: &str) -> String {
    let last = raw.rsplit(['/', '\\']).next().unwrap_or("");
    let cleaned: String = last
        .chars()
        .map(|c| if c.is_alphanumeric() || matches!(c, '.' | '-' | '_' | ' ') { c } else { '_' })
        .collect();
    let trimmed = cleaned.trim().trim_start_matches('.').to_string();
    if trimmed.is_empty() {
        return "file".to_string();
    }
    let count = trimmed.chars().count();
    if count <= 60 {
        trimmed
    } else {
        trimmed.chars().skip(count - 60).collect()
    }
}

/// `png`, `jpg`, `gif` or `webp` from the first bytes of a file, or `None`.
pub fn image_ext(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some("png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("gif")
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("webp")
    } else {
        None
    }
}

/// `1.5 MB`, `12 KB`, `300 B`.
pub fn size_word(bytes: usize) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{} KB", (bytes + 512) / 1024)
    } else {
        format!("{bytes} B")
    }
}

/// One file's kind and cleaned name from its bytes. A picture must start like a picture; anything else must be
/// plain text (valid UTF-8, no zero bytes). The saved name keeps the person's extension for text and takes the real
/// one for a picture.
fn inspect(file: &Incoming, bytes: &[u8]) -> Result<(Kind, String, Option<String>), StoreError> {
    let name = safe_name(&file.name);
    if bytes.is_empty() {
        return Err(bad(format!("{name} is empty.")));
    }
    if bytes.len() > MAX_FILE_BYTES {
        return Err(bad(format!("{name} is bigger than {}.", size_word(MAX_FILE_BYTES))));
    }
    if let Some(ext) = image_ext(bytes) {
        let stem = name.rsplit_once('.').map(|(s, _)| s).filter(|s| !s.is_empty()).unwrap_or(&name);
        return Ok((Kind::Image, format!("{stem}.{ext}"), None));
    }
    if file.kind == "image" {
        return Err(bad(format!("{name} is not a picture Potato Bot can keep (png, jpg, gif or webp).")));
    }
    if bytes.contains(&0) {
        return Err(bad(format!("{name} is not a text file.")));
    }
    match String::from_utf8(bytes.to_vec()) {
        Ok(text) => Ok((Kind::Text, name, Some(text))),
        Err(_) => Err(bad(format!("{name} is not a text file."))),
    }
}

fn dir_of(root: &Path, bot: &str) -> Result<PathBuf, StoreError> {
    Ok(store::brain_dir(root, bot)?.join(FOLDER))
}

/// Checks every file, then saves them all (none is saved if one is refused). `stamp` makes the file names unique.
pub fn save_all(root: &Path, bot: &str, files: &[Incoming], stamp: i64) -> Result<Vec<Saved>, StoreError> {
    if files.is_empty() {
        return Ok(Vec::new());
    }
    if files.len() > MAX_FILES {
        return Err(bad(format!("Attach at most {MAX_FILES} files to one message.")));
    }
    let dir = dir_of(root, bot)?;
    let mut checked: Vec<(Vec<u8>, Kind, String, Option<String>)> = Vec::new();
    let mut total = 0usize;
    for file in files {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(file.data.trim())
            .map_err(|_| bad(format!("{} could not be read.", safe_name(&file.name))))?;
        total += bytes.len();
        if total > MAX_TOTAL_BYTES {
            return Err(bad(format!("The files together are bigger than {}.", size_word(MAX_TOTAL_BYTES))));
        }
        let (kind, name, text) = inspect(file, &bytes)?;
        checked.push((bytes, kind, name, text));
    }
    crate::secure_file::create_private_dir(&dir).map_err(|e| bad(format!("Could not make the attachments folder: {e}.")))?;
    let mut saved: Vec<Saved> = Vec::new();
    for (n, (bytes, kind, name, text)) in checked.into_iter().enumerate() {
        let file_name = format!("{stamp}-{}-{name}", n + 1);
        if let Err(e) = std::fs::write(dir.join(&file_name), &bytes) {
            forget(root, bot, &saved);
            return Err(bad(format!("Could not save {name}: {e}.")));
        }
        saved.push(Saved { name, rel: format!("{FOLDER}/{file_name}"), kind, bytes: bytes.len(), text });
    }
    Ok(saved)
}

/// Removes files this module saved (a send that failed). Only names under the bot's attachments folder.
pub fn forget(root: &Path, bot: &str, saved: &[Saved]) {
    let Ok(base) = store::brain_dir(root, bot) else { return };
    for file in saved {
        if file.rel.starts_with(&format!("{FOLDER}/")) && !file.rel.contains("..") {
            let _ = std::fs::remove_file(base.join(&file.rel));
        }
    }
}

/// The longest run of backticks in `text`, so a quote can use a longer fence.
fn longest_ticks(text: &str) -> usize {
    let (mut best, mut run) = (0, 0);
    for c in text.chars() {
        if c == '`' {
            run += 1;
            best = best.max(run);
        } else {
            run = 0;
        }
    }
    best
}

/// The message the brain gets: the person's words, one line per file, then the text of each text file that still
/// fits (cut with a note when it does not). Never longer than the 4000 characters a message may have.
pub fn compose(text: &str, saved: &[Saved]) -> Result<String, StoreError> {
    let head = text.trim();
    let lines: Vec<String> = saved
        .iter()
        .map(|f| {
            let what = if f.kind == Kind::Image { "image" } else { "file" };
            format!("[Attached {what}: {} ({}), saved as {}]", f.name, size_word(f.bytes), f.rel)
        })
        .collect();
    let mut out = String::new();
    if !head.is_empty() {
        out.push_str(head);
        out.push_str("\n\n");
    }
    out.push_str(&lines.join("\n"));
    let used = out.chars().count();
    if used > MAX_MESSAGE_CHARS {
        return Err(bad("That message is too long with the attachments. Shorten it a little."));
    }
    let mut room = MAX_MESSAGE_CHARS - used;
    for file in saved {
        let Some(body) = file.text.as_deref() else { continue };
        let body = body.trim_end();
        if body.is_empty() {
            continue;
        }
        let fence = "`".repeat(longest_ticks(body).max(2) + 1);
        // "\n\n" + fence + "\n" + body + "\n" + fence, and a marker when it is cut.
        let label = format!("{}:", file.name);
        let frame = 2 + label.chars().count() + 1 + fence.len() + 1 + 1 + fence.len();
        let cut_note = "\n(Cut here. The whole file is saved.)";
        if room < frame + MIN_QUOTE {
            continue;
        }
        let full = body.chars().count();
        let (shown, note) = if frame + full <= room {
            (body.to_string(), "")
        } else {
            let keep = room.saturating_sub(frame + cut_note.chars().count());
            (body.chars().take(keep).collect::<String>(), cut_note)
        };
        let block = format!("\n\n{label}\n{fence}\n{shown}\n{fence}{note}");
        room = room.saturating_sub(block.chars().count());
        out.push_str(&block);
    }
    Ok(out)
}
