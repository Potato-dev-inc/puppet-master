//! Potato Bot files in `<app data>/potato/` (spec section 3). Every function takes the root folder (the
//! app data folder in the app, a temp folder in the tests). Writes go through `secure_file::write_private`
//! (temp file, then rename); a private conversation grows by appended lines instead, so a message costs
//! the same at line 5000 as at line 5. A missing file reads as empty; a damaged file is an error and is never
//! overwritten (the rule of `companion/store.rs`).
//!
//! - `bots.json`: `{ v: 1, bots: [{ id, talkPersona, createdMs }] }`. A bot IS a companion profile: `id`
//!   is the companion id, and `companions.json` does not change.
//! - `settings.json`: the choices of Settings (brain, default persona, daily limit). Written only by
//!   Tauri commands, never reachable through the bridge or the phone.
//! - `conversations/<surface>/<conversation id>.jsonl`: one message per line. The id's colons are written
//!   as `%3A` so the name is a valid file name on every system. Text is redacted on write and again on
//!   display, at most 4000 characters a message, 2 MB or 5000 lines a file, then it rotates to `.1.jsonl`
//!   (one old file is kept). A thread of a shared channel keeps only its newest 500 lines.
//! - `reads.json`: when the person last looked at each bot's thread (for the unread dot).
//! - `brain/<bot>/vault/`, `brain/<bot>/persona/`: the brain's memory and its persona files. The persona
//!   folder is seeded from the app's resources and updated only where the person never edited a file
//!   (`persona.sha256` holds the hash of what was seeded).
//! - "Forget talk memory" moves `brain/<bot>` into `potato/.removed/`, it never deletes it.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::secure_file;

pub const STORE_VERSION: u32 = 1;
pub const MAX_MESSAGE_CHARS: usize = 4000;
pub const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
pub const MAX_FILE_LINES: usize = 5000;
pub const CHANNEL_KEEP: usize = 500;
pub const MAX_BOTS: usize = 3;

pub type StoreResult<T> = Result<T, StoreError>;

static FILES: once_cell::sync::Lazy<parking_lot::ReentrantMutex<()>> = once_cell::sync::Lazy::new(|| parking_lot::ReentrantMutex::new(()));

/// One lock for every whole-file write of Potato Bot (`bots.json`, `settings.json`, `reads.json`, the usage
/// files, the persona manifest, a channel thread) and for every read-then-write of them. Two commands or
/// threads can write at once in the real app; `write_private` names its temp file after the target, so two
/// writers of one file would collide on it, and a read-modify-write would lose a change. Reentrant, so a
/// function that holds it may call another that takes it.
pub(super) fn files_lock() -> parking_lot::ReentrantMutexGuard<'static, ()> {
    FILES.lock()
}

/// `code` is a short wire word for the UI; `message` is plain English.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreError {
    pub code: &'static str,
    pub message: String,
}

impl StoreError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

const INVALID: &str = "POTATO_STORE_INVALID";

fn io_error(what: &str, e: impl std::fmt::Display) -> StoreError {
    StoreError::new(INVALID, format!("Could not {what}: {e}."))
}

// ---- bots ---------------------------------------------------------------------------------------

/// Which voice a bot talks in. The names are the persona packs of the brain.
pub const PACKS: [&str; 2] = ["fish", "potato"];

pub fn valid_pack(pack: &str) -> bool {
    PACKS.contains(&pack)
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Bot {
    /// The companion id.
    pub id: String,
    /// `fish` or `potato`. (Named so because a companion profile's `persona` already means cheerful, calm ...)
    pub talk_persona: String,
    pub created_ms: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BotsFile {
    pub v: u32,
    pub bots: Vec<Bot>,
}

pub fn bots_path(root: &Path) -> PathBuf {
    root.join("bots.json")
}

pub(super) fn read_json<T: for<'de> Deserialize<'de>>(path: &Path, what: &str) -> StoreResult<Option<T>> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io_error(&format!("read {what}"), e)),
    };
    serde_json::from_str(&text).map(Some).map_err(|e| StoreError::new(INVALID, format!("{what} is damaged: {e}.")))
}

pub(super) fn write_json<T: Serialize>(path: &Path, value: &T, what: &str) -> StoreResult<()> {
    let _files = files_lock();
    let text = serde_json::to_string_pretty(value).map_err(|e| io_error(&format!("save {what}"), e))?;
    secure_file::write_private(path, &text).map_err(|e| StoreError::new(INVALID, e))
}

pub fn load_bots(root: &Path) -> StoreResult<Vec<Bot>> {
    let file: Option<BotsFile> = read_json(&bots_path(root), "bots.json")?;
    match file {
        None => Ok(Vec::new()),
        Some(file) if file.v > STORE_VERSION => Err(StoreError::new(INVALID, "bots.json was written by a newer Puppet Master.")),
        Some(file) => Ok(file.bots),
    }
}

pub fn save_bots(root: &Path, bots: &[Bot]) -> StoreResult<()> {
    write_json(&bots_path(root), &BotsFile { v: STORE_VERSION, bots: bots.to_vec() }, "bots.json")
}

/// Adds a bot for a companion. The companion must exist (the caller checks); at most 3 bots.
pub fn add_bot(root: &Path, id: &str, talk_persona: &str, now_ms: i64) -> StoreResult<Bot> {
    let _files = files_lock();
    if !crate::companion::model::valid_id(id) {
        return Err(StoreError::new("POTATO_BOT_INVALID", "That is not a bot name Potato Bot can use."));
    }
    if !valid_pack(talk_persona) {
        return Err(StoreError::new("POTATO_PERSONA_INVALID", "Pick Fish or Potato."));
    }
    let mut bots = load_bots(root)?;
    if bots.iter().any(|b| b.id == id) {
        return Err(StoreError::new("POTATO_BOT_EXISTS", "That bot is already here."));
    }
    if bots.len() >= MAX_BOTS {
        return Err(StoreError::new("POTATO_BOT_LIMIT", format!("You can have at most {MAX_BOTS} bots.")));
    }
    let bot = Bot { id: id.to_string(), talk_persona: talk_persona.to_string(), created_ms: now_ms };
    bots.push(bot.clone());
    save_bots(root, &bots)?;
    Ok(bot)
}

pub fn set_talk_persona(root: &Path, id: &str, talk_persona: &str) -> StoreResult<Bot> {
    let _files = files_lock();
    if !valid_pack(talk_persona) {
        return Err(StoreError::new("POTATO_PERSONA_INVALID", "Pick Fish or Potato."));
    }
    let mut bots = load_bots(root)?;
    let bot = bots.iter_mut().find(|b| b.id == id).ok_or_else(|| StoreError::new("POTATO_BOT_NOT_FOUND", "No bot has that name."))?;
    bot.talk_persona = talk_persona.to_string();
    let copy = bot.clone();
    save_bots(root, &bots)?;
    Ok(copy)
}

// ---- settings -----------------------------------------------------------------------------------

/// What the brain talks to.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BrainKind {
    /// DeepSeek with the person's key (the default).
    Deepseek,
    /// The person's Go plan sign-in.
    Go,
    /// Through the person's own agents: no brain process at all.
    Agents,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub v: u32,
    pub brain: BrainKind,
    /// The persona a new bot starts with.
    pub default_persona: String,
    pub daily_limit_cents: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { v: STORE_VERSION, brain: BrainKind::Deepseek, default_persona: "fish".into(), daily_limit_cents: super::cost::DEFAULT_LIMIT_CENTS }
    }
}

pub fn settings_path(root: &Path) -> PathBuf {
    root.join("settings.json")
}

pub fn load_settings(root: &Path) -> StoreResult<Settings> {
    let settings: Option<Settings> = read_json(&settings_path(root), "settings.json")?;
    let mut settings = settings.unwrap_or_default();
    settings.daily_limit_cents = super::cost::clamp_limit_cents(settings.daily_limit_cents);
    if !valid_pack(&settings.default_persona) {
        settings.default_persona = "fish".into();
    }
    Ok(settings)
}

pub fn save_settings(root: &Path, settings: &Settings) -> StoreResult<()> {
    let mut settings = settings.clone();
    settings.v = STORE_VERSION;
    settings.daily_limit_cents = super::cost::clamp_limit_cents(settings.daily_limit_cents);
    if !valid_pack(&settings.default_persona) {
        return Err(StoreError::new("POTATO_PERSONA_INVALID", "Pick Fish or Potato."));
    }
    write_json(&settings_path(root), &settings, "settings.json")
}

// ---- conversations ------------------------------------------------------------------------------

/// `app:dm:<bot>`.
pub fn app_conversation(bot: &str) -> String {
    format!("app:dm:{bot}")
}

/// The surface in front of a conversation id (`app`, or an outside surface), when the id is well formed:
/// `<surface>:<kind>:<rest...>`, every part made of `[A-Za-z0-9._-]`, at most 200 characters.
pub fn surface_of(conversation_id: &str) -> Option<&str> {
    if conversation_id.is_empty() || conversation_id.len() > 200 {
        return None;
    }
    let mut parts = conversation_id.split(':');
    let surface = parts.next()?;
    let kind = parts.next()?;
    let rest: Vec<&str> = parts.collect();
    let ok_part = |p: &str| !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    let surface_ok = !surface.is_empty() && surface.len() <= 16 && surface.bytes().all(|b| b.is_ascii_lowercase());
    (surface_ok && ok_part(kind) && !rest.is_empty() && rest.iter().all(|p| ok_part(p))).then_some(surface)
}

/// The bot an app conversation belongs to: `app:dm:<bot>` gives `<bot>`.
pub fn app_bot_of(conversation_id: &str) -> Option<&str> {
    let bot = conversation_id.strip_prefix("app:dm:")?;
    (surface_of(conversation_id).is_some() && crate::companion::model::valid_id(bot)).then_some(bot)
}

/// A conversation id as a file name: every character that is not a letter, digit, `.`, `_` or `-` is `%XX`.
pub fn file_stem(conversation_id: &str) -> String {
    let mut out = String::new();
    for b in conversation_id.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `<root>/conversations/<surface>/<stem>.jsonl`; refused for an id that is not well formed.
pub fn conversation_path(root: &Path, conversation_id: &str) -> StoreResult<PathBuf> {
    let surface = surface_of(conversation_id).ok_or_else(|| StoreError::new("POTATO_CONVERSATION_INVALID", "That is not a conversation Potato Bot knows."))?;
    Ok(root.join("conversations").join(surface).join(format!("{}.jsonl", file_stem(conversation_id))))
}

fn old_path(path: &Path) -> PathBuf {
    path.with_extension("1.jsonl")
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FromKind {
    You,
    Bot,
    Person,
    Note,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Speaker {
    pub kind: FromKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MsgKind {
    Text,
    HelperCard,
    HelperResult,
    Note,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub at: i64,
    pub from: Speaker,
    pub kind: MsgKind,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mood: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    /// A helper card: the folder name the bot asked for (a name only, never a path).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// A helper result row: the pane the helper worked in, for its "Open" button.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane_id: Option<String>,
}

impl Message {
    pub fn new(at: i64, from: FromKind, name: Option<&str>, kind: MsgKind, text: &str) -> Message {
        Message {
            id: format!("m-{}", &uuid::Uuid::new_v4().simple().to_string()[..12]),
            at,
            from: Speaker { kind: from, name: name.map(str::to_string) },
            kind,
            text: text.to_string(),
            mood: None,
            request_id: None,
            project: None,
            pane_id: None,
        }
    }
}

/// The text that is kept and shown: secrets and the home folder taken out, cut to 4000 characters.
pub fn clean_text(text: &str, home: Option<&str>) -> String {
    let cleaned = crate::companion::view::scrub(text, home);
    cleaned.chars().take(MAX_MESSAGE_CHARS).collect()
}

fn read_lines(path: &Path) -> StoreResult<Vec<Message>> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(io_error("read a conversation", e)),
    };
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str::<Message>(l).map_err(|e| StoreError::new(INVALID, format!("A conversation file is damaged: {e}."))))
        .collect()
}

/// The most recent messages (oldest first), at most `limit`, from the old and the current file. The
/// text is redacted again on its way out.
pub fn read_messages(root: &Path, conversation_id: &str, limit: usize, home: Option<&str>) -> StoreResult<Vec<Message>> {
    let path = conversation_path(root, conversation_id)?;
    let mut all = read_lines(&old_path(&path))?;
    all.extend(read_lines(&path)?);
    let skip = all.len().saturating_sub(limit.max(1));
    Ok(all
        .into_iter()
        .skip(skip)
        .map(|mut m| {
            m.text = clean_text(&m.text, home);
            m
        })
        .collect())
}

/// Lines in a conversation file, remembered with the file size they were counted at, so a message is
/// appended without reading the whole file again. A file changed behind our back (a different size) is
/// counted again.
static LINE_COUNTS: once_cell::sync::Lazy<parking_lot::Mutex<std::collections::HashMap<PathBuf, (u64, usize)>>> =
    once_cell::sync::Lazy::new(Default::default);

fn count_lines(path: &Path) -> StoreResult<usize> {
    match fs::read(path) {
        Ok(bytes) => Ok(bytes.iter().filter(|b| **b == b'\n').count()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(e) => Err(io_error("read a conversation", e)),
    }
}

/// The last line of the file must be a message: a file that ends in something else is damaged, and is
/// left exactly as it is.
fn check_tail(path: &Path, len: u64) -> StoreResult<()> {
    use std::io::{Read, Seek, SeekFrom};
    if len == 0 {
        return Ok(());
    }
    let mut file = fs::File::open(path).map_err(|e| io_error("read a conversation", e))?;
    let back = len.min(MAX_MESSAGE_CHARS as u64 * 8);
    file.seek(SeekFrom::Start(len - back)).map_err(|e| io_error("read a conversation", e))?;
    let mut tail = Vec::new();
    file.read_to_end(&mut tail).map_err(|e| io_error("read a conversation", e))?;
    let text = String::from_utf8_lossy(&tail);
    let last = text.trim_end().rsplit('\n').next().unwrap_or("");
    if last.is_empty() || serde_json::from_str::<Message>(last).is_err() {
        return Err(StoreError::new(INVALID, "A conversation file is damaged."));
    }
    Ok(())
}

fn append_line(path: &Path, line: &str) -> StoreResult<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        secure_file::create_private_dir(parent).map_err(|e| io_error("make the conversation folder", e))?;
    }
    let mut options = fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|e| io_error("write a conversation", e))?;
    file.write_all(line.as_bytes()).map_err(|e| io_error("write a conversation", e))
}

/// Appends one message, redacting its text first. A damaged file is an error; nothing is overwritten.
///
/// A private thread grows one line at a time (appended, never rewritten, so a message costs the same
/// at line 5000 as at line 5); a shared-channel thread is small and is rewritten whole through
/// `secure_file::write_private`.
pub fn append_message(root: &Path, conversation_id: &str, mut message: Message, home: Option<&str>) -> StoreResult<Message> {
    let _files = files_lock();
    let path = conversation_path(root, conversation_id)?;
    message.text = clean_text(&message.text, home);
    let line = format!("{}\n", serde_json::to_string(&message).map_err(|e| io_error("save a message", e))?);

    if conversation_id.contains(":ch:") {
        let mut current = read_lines(&path)?;
        current.push(message.clone());
        let extra = current.len().saturating_sub(CHANNEL_KEEP);
        current.drain(..extra);
        let mut text = String::new();
        for m in &current {
            text.push_str(&serde_json::to_string(m).map_err(|e| io_error("save a message", e))?);
            text.push('\n');
        }
        secure_file::write_private(&path, &text).map_err(|e| StoreError::new(INVALID, e))?;
        return Ok(message);
    }

    let len = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    check_tail(&path, len)?;
    let lines = match LINE_COUNTS.lock().get(&path) {
        Some((at_len, lines)) if *at_len == len => *lines,
        _ => count_lines(&path)?,
    };
    if len + line.len() as u64 > MAX_FILE_BYTES || lines + 1 > MAX_FILE_LINES {
        // Rotate: the current file becomes the one old file, and the new message starts a new one.
        let old = old_path(&path);
        let _ = fs::remove_file(&old);
        if path.exists() {
            fs::rename(&path, &old).map_err(|e| io_error("rotate a conversation", e))?;
        }
        append_line(&path, &line)?;
        LINE_COUNTS.lock().insert(path, (line.len() as u64, 1));
    } else {
        append_line(&path, &line)?;
        LINE_COUNTS.lock().insert(path, (len + line.len() as u64, lines + 1));
    }
    Ok(message)
}

// ---- reads --------------------------------------------------------------------------------------

pub fn reads_path(root: &Path) -> PathBuf {
    root.join("reads.json")
}

pub fn load_reads(root: &Path) -> StoreResult<BTreeMap<String, i64>> {
    Ok(read_json(&reads_path(root), "reads.json")?.unwrap_or_default())
}

pub fn mark_read(root: &Path, bot: &str, at_ms: i64) -> StoreResult<()> {
    let _files = files_lock();
    let mut reads = load_reads(root)?;
    reads.insert(bot.to_string(), at_ms);
    write_json(&reads_path(root), &reads, "reads.json")
}

// ---- the brain's own folders --------------------------------------------------------------------

pub fn brain_dir(root: &Path, bot: &str) -> StoreResult<PathBuf> {
    if !crate::companion::model::valid_id(bot) {
        return Err(StoreError::new("POTATO_BOT_NOT_FOUND", "No bot has that name."));
    }
    Ok(root.join("brain").join(bot))
}

/// The brain's memory.
pub fn vault_dir(root: &Path, bot: &str) -> StoreResult<PathBuf> {
    Ok(brain_dir(root, bot)?.join("vault"))
}

pub fn persona_dir(root: &Path, bot: &str) -> StoreResult<PathBuf> {
    Ok(brain_dir(root, bot)?.join("persona"))
}

/// An empty folder the brain is told to share from, so it has nothing to share.
pub fn share_dir(root: &Path, bot: &str) -> StoreResult<PathBuf> {
    Ok(brain_dir(root, bot)?.join("share-empty"))
}

pub fn state_dir(root: &Path, bot: &str) -> StoreResult<PathBuf> {
    Ok(vault_dir(root, bot)?.join("state"))
}

/// Makes the folders a brain needs and returns its working folder.
pub fn ensure_brain_dirs(root: &Path, bot: &str) -> StoreResult<PathBuf> {
    for dir in [vault_dir(root, bot)?, state_dir(root, bot)?, share_dir(root, bot)?, persona_dir(root, bot)?] {
        secure_file::create_private_dir(&dir).map_err(|e| io_error("make the brain folder", e))?;
    }
    brain_dir(root, bot)
}

fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn files_under(dir: &Path, base: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files_under(&path, base, out);
        } else if let Ok(rel) = path.strip_prefix(base) {
            out.push(rel.to_path_buf());
        }
    }
}

/// What `seed_persona` did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Seeded {
    pub copied: Vec<String>,
    pub updated: Vec<String>,
    /// Files the person edited: left alone.
    pub kept_edited: Vec<String>,
}

/// Copies the persona files from the app's resources into the bot's own folder. A file is written when
/// it is missing, and replaced by a newer one only if the person never edited it (the stored sha256 of
/// the seeded copy still matches what is on disk). The folder stays writable for the persona-note tool.
pub fn seed_persona(root: &Path, bot: &str, resource_persona: &Path) -> StoreResult<Seeded> {
    let _files = files_lock();
    let target = persona_dir(root, bot)?;
    secure_file::create_private_dir(&target).map_err(|e| io_error("make the persona folder", e))?;
    let manifest_path = brain_dir(root, bot)?.join("persona.sha256");
    let mut manifest: BTreeMap<String, String> = read_json(&manifest_path, "persona.sha256")?.unwrap_or_default();
    let mut files = Vec::new();
    files_under(resource_persona, resource_persona, &mut files);
    files.sort();
    let mut report = Seeded::default();
    for rel in files {
        let key = rel.to_string_lossy().replace('\\', "/");
        let from = resource_persona.join(&rel);
        let to = target.join(&rel);
        let new_bytes = fs::read(&from).map_err(|e| io_error("read a persona file", e))?;
        let new_hash = sha(&new_bytes);
        match fs::read(&to) {
            Err(_) => {
                if let Some(parent) = to.parent() {
                    secure_file::create_private_dir(parent).map_err(|e| io_error("make a persona folder", e))?;
                }
                fs::write(&to, &new_bytes).map_err(|e| io_error("write a persona file", e))?;
                manifest.insert(key.clone(), new_hash);
                report.copied.push(key);
            }
            Ok(have) => {
                let have_hash = sha(&have);
                if have_hash == new_hash {
                    manifest.insert(key, new_hash);
                } else if manifest.get(&key) == Some(&have_hash) {
                    fs::write(&to, &new_bytes).map_err(|e| io_error("update a persona file", e))?;
                    manifest.insert(key.clone(), new_hash);
                    report.updated.push(key);
                } else {
                    report.kept_edited.push(key);
                }
            }
        }
    }
    write_json(&manifest_path, &manifest, "persona.sha256")?;
    Ok(report)
}

/// "Forget talk memory": moves `brain/<bot>` into `potato/.removed/<bot>-<stamp>`. Nothing is deleted.
pub fn forget_talk_memory(root: &Path, bot: &str, stamp: &str) -> StoreResult<Option<PathBuf>> {
    let from = brain_dir(root, bot)?;
    if !from.exists() {
        return Ok(None);
    }
    let removed = root.join(".removed");
    secure_file::create_private_dir(&removed).map_err(|e| io_error("make the removed folder", e))?;
    let stamp: String = stamp.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
    let to = removed.join(format!("{bot}-{stamp}"));
    if to.exists() {
        return Err(StoreError::new(INVALID, "That backup folder already exists."));
    }
    fs::rename(&from, &to).map_err(|e| io_error("move the brain folder", e))?;
    Ok(Some(to))
}
