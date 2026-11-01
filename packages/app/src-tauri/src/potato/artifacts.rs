//! Potato Bot artifacts (the Artifacts page): files a bot made or received, kept under
//! `<app data>/potato/artifacts/<bot>/`. Each bot has one folder with `index.json` (the metadata, one record per
//! version) and one file per version, named `<id>.<ext>`. A new file with the name of an old one is a new version
//! (the old versions stay). The name is only a label; the file on disk is named by its id, never by the name, so a
//! name can never point outside the folder.
//!
//! - `index.json`: `{ v: 1, items: [{ id, name, bot, kind, size, created, version, file }] }`.
//! - `kind` is `image`, `text`, `markdown`, `html`, `pdf` or `other`, from the file extension. Nothing is ever run:
//!   HTML is only shown later in a sandboxed frame with no scripts.
//! - Sources: a message from the chat saved as a file ("Save as file"). Files the person attaches in chat are not
//!   here yet (no attachments module on this base).
//! - "Delete" removes one version (its record and its file). "Download" copies the file where the person picks.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use base64::Engine;
use serde::{Deserialize, Serialize};

use super::store::{self, Message, MsgKind, StoreError, StoreResult};
use crate::secure_file;

pub const ARTIFACT_VERSION: u32 = 1;
/// Largest file the app keeps (a saved message is at most 4000 characters, so this is for later sources).
pub const MAX_SAVE_BYTES: usize = 10 * 1024 * 1024;
/// Text, Markdown and HTML shown in the preview: the first megabyte.
pub const MAX_TEXT_PREVIEW_BYTES: usize = 1024 * 1024;
/// Images shown in the preview; a bigger image is listed but not drawn.
pub const MAX_IMAGE_PREVIEW_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_NAME_CHARS: usize = 120;
const INDEX: &str = "index.json";
const INVALID: &str = "POTATO_ARTIFACT_INVALID";
const NOT_FOUND: &str = "POTATO_ARTIFACT_NOT_FOUND";

/// What the page shows and filters by.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Image,
    Text,
    Markdown,
    Html,
    Pdf,
    Other,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    /// The record's id (also the file's base name). `a-` and twelve hex digits.
    pub id: String,
    /// The name the bot or the person gave it, cleaned (no folders, no control characters).
    pub name: String,
    /// The bot's id (a companion id).
    pub bot: String,
    pub kind: Kind,
    /// Bytes.
    pub size: u64,
    /// Epoch milliseconds.
    pub created: i64,
    /// 1 for the first file with this name, then 2, 3 ...
    pub version: u32,
    /// The file's name inside the bot's folder.
    pub file: String,
}

#[derive(Serialize, Deserialize)]
struct IndexFile {
    v: u32,
    items: Vec<Artifact>,
}

/// The Artifacts page's filters. Absent fields match everything.
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Filter {
    pub bot: Option<String>,
    pub kind: Option<Kind>,
    /// Matches part of the name, without regard to case.
    pub search: Option<String>,
}

/// One row of the list: the newest version of a name, and how many versions the name has.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ListItem {
    #[serde(flatten)]
    pub artifact: Artifact,
    pub versions: u32,
}

/// The preview of one version: its metadata, every version of its name (newest first), and the content when the kind
/// can be shown (text, Markdown and HTML as text; images as a data address). PDF and other files have no content here:
/// the page offers Download and Show in folder.
#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub artifact: Artifact,
    pub versions: Vec<Artifact>,
    pub text: Option<String>,
    /// The text was longer than the preview and was cut.
    pub truncated: bool,
    pub image: Option<String>,
}

fn io(what: &str, e: impl std::fmt::Display) -> StoreError {
    StoreError::new(INVALID, format!("Could not {what}: {e}."))
}

fn not_found() -> StoreError {
    StoreError::new(NOT_FOUND, "That file is gone. It may have been deleted.")
}

/// A bot id or an artifact id as a folder or file part: letters, digits, `.`, `_`, `-`; never `.` or `..`.
pub fn valid_part(part: &str) -> bool {
    !part.is_empty()
        && part.len() <= 200
        && part != "."
        && part != ".."
        && part.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

fn bot_dir(root: &Path, bot: &str) -> StoreResult<PathBuf> {
    if !valid_part(bot) {
        return Err(StoreError::new(INVALID, "That bot is not known."));
    }
    Ok(root.join("artifacts").join(bot))
}

/// The name as a label: the last part of any path, no control characters or characters Windows forbids in names,
/// at most 120 characters. An empty result becomes `file`.
pub fn clean_name(raw: &str) -> String {
    let base = raw.rsplit(['/', '\\']).next().unwrap_or("");
    let kept: String = base.chars().filter(|c| !c.is_control() && !"<>:\"|?*".contains(*c)).collect();
    let trimmed = kept.trim().trim_matches('.').trim();
    let cut: String = trimmed.chars().take(MAX_NAME_CHARS).collect();
    let cut = cut.trim_end();
    if cut.is_empty() {
        "file".to_string()
    } else {
        cut.to_string()
    }
}

/// The extension in lower case, when it is 1 to 8 letters or digits and the name has more than the extension.
pub fn extension(name: &str) -> String {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() && ext.len() <= 8 && ext.chars().all(|c| c.is_ascii_alphanumeric()) => {
            ext.to_ascii_lowercase()
        }
        _ => String::new(),
    }
}

pub fn kind_of(ext: &str) -> Kind {
    match ext {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" => Kind::Image,
        "md" | "markdown" => Kind::Markdown,
        "html" | "htm" => Kind::Html,
        "pdf" => Kind::Pdf,
        "txt" | "log" | "csv" | "tsv" | "json" | "xml" | "yaml" | "yml" | "toml" | "ini" | "sql" | "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "sh" | "css" => {
            Kind::Text
        }
        _ => Kind::Other,
    }
}

fn image_mime(ext: &str) -> &'static str {
    match ext {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        _ => "application/octet-stream",
    }
}

/// Two names are the same version family when they match without regard to case.
fn same_name(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

fn stored_name(id: &str, ext: &str) -> String {
    if ext.is_empty() {
        id.to_string()
    } else {
        format!("{id}.{ext}")
    }
}

fn load_items(dir: &Path) -> StoreResult<Vec<Artifact>> {
    let file: Option<IndexFile> = store::read_json(&dir.join(INDEX), "the artifacts index")?;
    match file {
        None => Ok(Vec::new()),
        Some(file) if file.v > ARTIFACT_VERSION => Err(StoreError::new(INVALID, "The artifacts index was written by a newer Puppet Master.")),
        Some(file) => Ok(file.items),
    }
}

fn save_items(dir: &Path, items: &[Artifact]) -> StoreResult<()> {
    store::write_json(&dir.join(INDEX), &IndexFile { v: ARTIFACT_VERSION, items: items.to_vec() }, "the artifacts index")
}

/// Writes bytes atomically (a temp file, then a rename), with private permissions from creation.
fn write_bytes(path: &Path, bytes: &[u8]) -> StoreResult<()> {
    let parent = path.parent().ok_or_else(|| io("save the file", "no folder"))?;
    secure_file::create_private_dir(parent).map_err(|e| io("create the artifacts folder", e))?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = path.with_file_name(format!("{name}.tmp"));
    let _ = fs::remove_file(&tmp);
    {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&tmp).map_err(|e| io("save the file", e))?;
        file.write_all(bytes).map_err(|e| io("save the file", e))?;
    }
    fs::rename(&tmp, path).map_err(|e| io("save the file", e))
}

/// Saves a file for a bot. The same name (without regard to case) as an old file makes the next version.
pub fn put(root: &Path, bot: &str, name: &str, bytes: &[u8], now_ms: i64) -> StoreResult<Artifact> {
    if bytes.len() > MAX_SAVE_BYTES {
        return Err(StoreError::new("POTATO_ARTIFACT_TOO_BIG", "That file is bigger than 10 MB."));
    }
    let dir = bot_dir(root, bot)?;
    let name = clean_name(name);
    let ext = extension(&name);
    let _files = store::files_lock();
    let mut items = load_items(&dir)?;
    let version = items.iter().filter(|a| same_name(&a.name, &name)).map(|a| a.version).max().unwrap_or(0) + 1;
    let id = format!("a-{}", &uuid::Uuid::new_v4().simple().to_string()[..12]);
    let file = stored_name(&id, &ext);
    write_bytes(&dir.join(&file), bytes)?;
    let artifact = Artifact { id, name, bot: bot.to_string(), kind: kind_of(&ext), size: bytes.len() as u64, created: now_ms, version, file };
    items.push(artifact.clone());
    save_items(&dir, &items)?;
    Ok(artifact)
}

/// The default name of a message saved as a file: its first line (no heading marks), at most 60 characters, `.md`.
pub fn default_message_name(text: &str) -> String {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    let stem: String = line.trim_start_matches('#').trim().chars().take(60).collect();
    let stem = stem.trim();
    if stem.is_empty() {
        "message.md".to_string()
    } else {
        format!("{stem}.md")
    }
}

/// "Save as file" on a chat message: the text becomes a Markdown file. Only a plain chat message can be saved.
pub fn save_message(root: &Path, bot: &str, message: &Message, name: Option<&str>, now_ms: i64) -> StoreResult<Artifact> {
    if message.kind != MsgKind::Text {
        return Err(StoreError::new(INVALID, "Only a chat message can be saved as a file."));
    }
    let name = match name.map(clean_name).filter(|n| n != "file") {
        Some(n) if extension(&n).is_empty() => format!("{n}.md"),
        Some(n) => n,
        None => default_message_name(&message.text),
    };
    put(root, bot, &name, message.text.as_bytes(), now_ms)
}

/// The bot folders to read: one bot, or every folder under `artifacts/`.
fn bots_to_read(root: &Path, bot: Option<&str>) -> StoreResult<Vec<String>> {
    if let Some(bot) = bot {
        bot_dir(root, bot)?;
        return Ok(vec![bot.to_string()]);
    }
    let base = root.join("artifacts");
    let entries = match fs::read_dir(&base) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(io("read the artifacts folder", e)),
    };
    let mut bots: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| valid_part(name))
        .collect();
    bots.sort();
    Ok(bots)
}

/// The list: the newest version of each name, filtered, newest first.
pub fn list(root: &Path, filter: &Filter) -> StoreResult<Vec<ListItem>> {
    let needle = filter.search.as_deref().map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty());
    let mut out = Vec::new();
    for bot in bots_to_read(root, filter.bot.as_deref())? {
        let items = load_items(&bot_dir(root, &bot)?)?;
        let mut families: BTreeMap<String, Vec<Artifact>> = BTreeMap::new();
        for item in items {
            families.entry(item.name.to_lowercase()).or_default().push(item);
        }
        for versions in families.into_values() {
            let versions_count = versions.len() as u32;
            let Some(newest) = versions.into_iter().max_by_key(|a| a.version) else { continue };
            if filter.kind.is_some_and(|k| k != newest.kind) {
                continue;
            }
            if needle.as_deref().is_some_and(|n| !newest.name.to_lowercase().contains(n)) {
                continue;
            }
            out.push(ListItem { artifact: newest, versions: versions_count });
        }
    }
    out.sort_by(|a, b| b.artifact.created.cmp(&a.artifact.created).then_with(|| a.artifact.name.cmp(&b.artifact.name)));
    Ok(out)
}

/// The file on disk for one record. The name in the index is checked, so a damaged index cannot point outside the folder.
pub fn locate(root: &Path, bot: &str, id: &str) -> StoreResult<(Artifact, PathBuf)> {
    if !valid_part(id) {
        return Err(not_found());
    }
    let dir = bot_dir(root, bot)?;
    let artifact = load_items(&dir)?.into_iter().find(|a| a.id == id).ok_or_else(not_found)?;
    if !valid_part(&artifact.file) {
        return Err(StoreError::new(INVALID, "The artifacts index is damaged."));
    }
    let path = dir.join(&artifact.file);
    if !path.is_file() {
        return Err(StoreError::new(NOT_FOUND, "The file is missing from the folder."));
    }
    Ok((artifact, path))
}

/// The preview of one version. Reads the file only for the kinds the page can show.
pub fn open(root: &Path, bot: &str, id: &str) -> StoreResult<Preview> {
    let (artifact, path) = locate(root, bot, id)?;
    let mut versions: Vec<Artifact> = load_items(&bot_dir(root, bot)?)?.into_iter().filter(|a| same_name(&a.name, &artifact.name)).collect();
    versions.sort_by(|a, b| b.version.cmp(&a.version));
    let ext = extension(&artifact.name);
    let (mut text, mut truncated, mut image) = (None, false, None);
    match artifact.kind {
        Kind::Text | Kind::Markdown | Kind::Html => {
            let bytes = fs::read(&path).map_err(|e| io("read the file", e))?;
            truncated = bytes.len() > MAX_TEXT_PREVIEW_BYTES;
            let cut = &bytes[..bytes.len().min(MAX_TEXT_PREVIEW_BYTES)];
            text = Some(String::from_utf8_lossy(cut).into_owned());
        }
        Kind::Image if artifact.size as usize <= MAX_IMAGE_PREVIEW_BYTES => {
            let bytes = fs::read(&path).map_err(|e| io("read the file", e))?;
            let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
            image = Some(format!("data:{};base64,{encoded}", image_mime(&ext)));
        }
        _ => {}
    }
    Ok(Preview { artifact, versions, text, truncated, image })
}

/// Copies the file to a place the person picked (Download / Save as). Never moves it.
pub fn export(root: &Path, bot: &str, id: &str, dest: &Path) -> StoreResult<()> {
    let (_, path) = locate(root, bot, id)?;
    fs::copy(&path, dest).map(|_| ()).map_err(|e| io("save the file", e))
}

/// Deletes one version: its record first (so a crash leaves no record without a file), then its file.
pub fn delete(root: &Path, bot: &str, id: &str) -> StoreResult<()> {
    if !valid_part(id) {
        return Err(not_found());
    }
    let dir = bot_dir(root, bot)?;
    let _files = store::files_lock();
    let mut items = load_items(&dir)?;
    let at = items.iter().position(|a| a.id == id).ok_or_else(not_found)?;
    let removed = items.remove(at);
    save_items(&dir, &items)?;
    if valid_part(&removed.file) {
        let _ = fs::remove_file(dir.join(&removed.file));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_labels_and_never_paths() {
        assert_eq!(clean_name("../../secret/plan.md"), "plan.md");
        assert_eq!(clean_name("C:\\Users\\me\\notes.txt"), "notes.txt");
        assert_eq!(clean_name("bad<name>:x?.md"), "badnamex.md");
        assert_eq!(clean_name("   ...   "), "file");
        assert_eq!(clean_name("a\u{0007}b.md"), "ab.md");
        assert_eq!(clean_name(&"x".repeat(300)).chars().count(), MAX_NAME_CHARS);
        assert_eq!(extension("Report.PDF"), "pdf");
        assert_eq!(extension(".bashrc"), "");
        assert_eq!(extension("noext"), "");
        assert_eq!(extension("a.averyveryverylongext"), "");
    }

    #[test]
    fn kinds_come_from_the_extension() {
        assert_eq!(kind_of("png"), Kind::Image);
        assert_eq!(kind_of("md"), Kind::Markdown);
        assert_eq!(kind_of("html"), Kind::Html);
        assert_eq!(kind_of("pdf"), Kind::Pdf);
        assert_eq!(kind_of("csv"), Kind::Text);
        assert_eq!(kind_of("svg"), Kind::Other, "an SVG can carry script: it is only downloaded, never drawn");
        assert_eq!(kind_of("exe"), Kind::Other);
        assert_eq!(kind_of(""), Kind::Other);
    }

    #[test]
    fn default_names_come_from_the_first_line() {
        assert_eq!(default_message_name("## Plan for Friday\nmore"), "Plan for Friday.md");
        assert_eq!(default_message_name("\n\n  \n"), "message.md");
        let long = "a".repeat(200);
        assert_eq!(default_message_name(&long), format!("{}.md", "a".repeat(60)));
    }
}
