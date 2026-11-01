//! Recently deleted (UX-2): a deleted memory note, routine or whole bot moves aside into `potato/trash/<id>/` and is
//! kept for 7 days, then purged. Each item has an `item.json` record (what it was, when it was deleted, where its
//! files were) and a `files/` folder with the moved files. Nothing secret is kept here: bot keys live in the OS
//! credential store only.
//!
//! - `trash_note`, `trash_routine`, `trash_bot`: move the item aside (the delete commands call these).
//! - `list`: the items, newest first. `restore(id)`: puts an item back as it was (refused, and kept, on a clash).
//! - `purge_expired(now)`: removes items deleted 7 days ago or more. `maybe_purge` runs it at most once a day
//!   (the routines tick calls it, so it also runs at app start).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use super::bot_settings;
use super::memory_notes;
use super::routines::{self, Routine, RoutinesFile};
use super::store::{self, Bot, StoreError, StoreResult};
use crate::secure_file;

pub const DAY_MS: i64 = 24 * 60 * 60 * 1000;
/// How long a deleted item stays in the trash.
pub const KEEP_MS: i64 = 7 * DAY_MS;
const DIR: &str = "trash";
const ITEM_FILE: &str = "item.json";
const FILES: &str = "files";
const RECORD_VERSION: u32 = 1;
const NOT_FOUND: &str = "POTATO_TRASH_NOT_FOUND";
const CONFLICT: &str = "POTATO_TRASH_CONFLICT";

static SEQ: AtomicU64 = AtomicU64::new(0);
static LAST_PURGE_MS: AtomicI64 = AtomicI64::new(i64::MIN);

/// What the Recently deleted list shows.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub id: String,
    /// `note`, `routine` or `bot`.
    pub kind: String,
    pub bot: String,
    /// The note's name, the routine's title or the bot's name.
    pub title: String,
    /// When it was deleted (epoch ms).
    pub deleted_at: i64,
}

/// The record on disk: the item, plus what restoring needs.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
struct Record {
    v: u32,
    #[serde(flatten)]
    item: Item,
    /// A note: its path in the vault, `journal/2026/10/2026-10-28.md`.
    #[serde(default)]
    note_path: Option<String>,
    /// A routine: the routine as it was.
    #[serde(default)]
    routine: Option<Routine>,
    /// A bot: its list entry, its routines, its read mark, and which moved files it had (see `FILE_NAMES`).
    #[serde(default)]
    bot_record: Option<Bot>,
    #[serde(default)]
    bot_routines: Vec<Routine>,
    #[serde(default)]
    read_mark: Option<i64>,
    #[serde(default)]
    files: Vec<String>,
}

/// The files a bot's folder can hold, by their name in the trash: the chat, its old chat, its settings, its brain.
const FILE_NAMES: [&str; 4] = ["chat.jsonl", "chat.1.jsonl", "settings.json", "brain"];

fn new_item(kind: &str, bot: &str, title: &str, now_ms: i64) -> Item {
    let n = SEQ.fetch_add(1, Ordering::SeqCst);
    Item { id: format!("{kind}-{now_ms}-{n}"), kind: kind.into(), bot: bot.into(), title: title.into(), deleted_at: now_ms }
}

fn trash_root(root: &Path) -> PathBuf {
    root.join(DIR)
}

fn item_dir(root: &Path, id: &str) -> PathBuf {
    trash_root(root).join(id)
}

fn io(what: &str, e: impl std::fmt::Display) -> StoreError {
    StoreError::new("POTATO_STORE_INVALID", format!("Could not {what}: {e}."))
}

/// Makes the item's folder and its `files/` folder.
fn make_dir(root: &Path, id: &str) -> StoreResult<PathBuf> {
    secure_file::create_private_dir(&trash_root(root)).map_err(|e| io("make the trash folder", e))?;
    let dir = item_dir(root, id);
    secure_file::create_private_dir(&dir).map_err(|e| io("make the trash folder", e))?;
    secure_file::create_private_dir(&dir.join(FILES)).map_err(|e| io("make the trash folder", e))?;
    Ok(dir)
}

fn write_record(dir: &Path, record: &Record) -> StoreResult<()> {
    store::write_json(&dir.join(ITEM_FILE), record, "the trash record")
}

fn read_record(dir: &Path) -> Option<Record> {
    store::read_json::<Record>(&dir.join(ITEM_FILE), "the trash record").ok().flatten()
}

fn load_routines(root: &Path) -> StoreResult<Vec<Routine>> {
    let file: Option<RoutinesFile> = store::read_json(&routines::routines_path(root), "routines.json")?;
    Ok(file.map(|f| f.routines).unwrap_or_default())
}

fn save_routines(root: &Path, routines: &[Routine]) -> StoreResult<()> {
    store::write_json(&routines::routines_path(root), &RoutinesFile { v: routines::FILE_VERSION, routines: routines.to_vec() }, "routines.json")
}

fn not_found() -> StoreError {
    StoreError::new(NOT_FOUND, "That item is no longer in Recently deleted.")
}

// ---- moving aside ------------------------------------------------------------------------------

/// Moves one memory note into the trash. The note must be one the memory list would show.
pub fn trash_note(root: &Path, bot: &str, rel: &str, now_ms: i64) -> StoreResult<Item> {
    let _files = store::files_lock();
    let path = memory_notes::checked_path(root, bot, rel)?;
    let title = rel.rsplit('/').next().unwrap_or(rel).trim_end_matches(".md").to_string();
    let item = new_item("note", bot, &title, now_ms);
    let dir = make_dir(root, &item.id)?;
    write_record(&dir, &Record { v: RECORD_VERSION, item: item.clone(), note_path: Some(rel.into()), routine: None, bot_record: None, bot_routines: vec![], read_mark: None, files: vec![] })?;
    fs::rename(&path, dir.join(FILES).join("note.md")).map_err(|e| io("move the note aside", e))?;
    Ok(item)
}

/// Takes one routine off the schedule and into the trash.
pub fn trash_routine(root: &Path, routine_id: &str, now_ms: i64) -> StoreResult<Item> {
    let _files = store::files_lock();
    let mut all = load_routines(root)?;
    let at = all.iter().position(|r| r.id == routine_id).ok_or_else(|| StoreError::new("POTATO_ROUTINE_NOT_FOUND", "No routine has that name."))?;
    let routine = all[at].clone();
    let item = new_item("routine", &routine.bot, &routine.title, now_ms);
    let dir = make_dir(root, &item.id)?;
    write_record(&dir, &Record { v: RECORD_VERSION, item: item.clone(), note_path: None, routine: Some(routine), bot_record: None, bot_routines: vec![], read_mark: None, files: vec![] })?;
    all.remove(at);
    save_routines(root, &all)?;
    Ok(item)
}

/// Takes a whole bot off the list. Its chat, settings and brain folder move into the trash with it, and so do its
/// routines and its read mark. Its keys are not here: they live in the OS store and stay there.
pub fn trash_bot(root: &Path, bot: &str, name: &str, now_ms: i64) -> StoreResult<Item> {
    let _files = store::files_lock();
    let record = store::load_bots(root)?.into_iter().find(|b| b.id == bot).ok_or_else(|| StoreError::new("POTATO_BOT_NOT_FOUND", "No bot has that name."))?;
    let chat = store::conversation_path(root, &store::app_conversation(bot))?;
    let sources = [
        ("chat.jsonl", chat.clone()),
        ("chat.1.jsonl", chat.with_extension("1.jsonl")),
        ("settings.json", bot_settings::path(root, bot)?),
        ("brain", store::brain_dir(root, bot)?),
    ];
    let present: Vec<String> = sources.iter().filter(|(_, from)| from.exists()).map(|(name, _)| name.to_string()).collect();

    let all = load_routines(root)?;
    let (mine, others): (Vec<Routine>, Vec<Routine>) = all.into_iter().partition(|r| r.bot == bot);
    let mut reads = store::load_reads(root)?;
    let read_mark = reads.remove(bot);

    let item = new_item("bot", bot, name, now_ms);
    let dir = make_dir(root, &item.id)?;
    // The record first: if a move fails halfway, the record still names what is in the trash.
    write_record(
        &dir,
        &Record { v: RECORD_VERSION, item: item.clone(), note_path: None, routine: None, bot_record: Some(record), bot_routines: mine, read_mark, files: present.clone() },
    )?;
    for (name, from) in sources {
        if !present.iter().any(|p| p == name) {
            continue;
        }
        fs::rename(&from, dir.join(FILES).join(name)).map_err(|e| io("move the bot's files aside", e))?;
    }
    store::save_bots(root, &store::load_bots(root)?.into_iter().filter(|b| b.id != bot).collect::<Vec<_>>())?;
    save_routines(root, &others)?;
    store::write_json(&store::reads_path(root), &reads, "reads.json")?;
    Ok(item)
}

// ---- reading -----------------------------------------------------------------------------------

/// Everything in the trash, newest first.
pub fn list(root: &Path) -> StoreResult<Vec<Item>> {
    let mut items = Vec::new();
    let Ok(entries) = fs::read_dir(trash_root(root)) else { return Ok(items) };
    for entry in entries.flatten() {
        if let Some(record) = read_record(&entry.path()) {
            items.push(record.item);
        }
    }
    items.sort_by(|a, b| b.deleted_at.cmp(&a.deleted_at).then_with(|| a.id.cmp(&b.id)));
    Ok(items)
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 120 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

fn plain_rel(rel: &str) -> bool {
    !rel.is_empty() && rel.split('/').all(|p| !p.is_empty() && p != "." && p != ".." && !p.contains(['\\', ':', '\0']))
}

// ---- restoring ---------------------------------------------------------------------------------

/// Puts an item back as it was and removes it from the trash. A clash (the note's name is taken, the routine's id
/// or the bot's name is back) is refused and the item stays in the trash.
pub fn restore(root: &Path, id: &str) -> StoreResult<Item> {
    let _files = store::files_lock();
    if !valid_id(id) {
        return Err(not_found());
    }
    let dir = item_dir(root, id);
    let record = read_record(&dir).ok_or_else(not_found)?;
    match record.item.kind.as_str() {
        "note" => restore_note(root, &record, &dir)?,
        "routine" => restore_routine(root, &record)?,
        "bot" => restore_bot(root, &record, &dir)?,
        _ => return Err(not_found()),
    }
    fs::remove_dir_all(&dir).map_err(|e| io("empty the trash item", e))?;
    Ok(record.item)
}

fn restore_note(root: &Path, record: &Record, dir: &Path) -> StoreResult<()> {
    let rel = record.note_path.clone().filter(|r| plain_rel(r)).ok_or_else(not_found)?;
    let target = store::vault_dir(root, &record.item.bot)?.join(rel.split('/').collect::<PathBuf>());
    if target.exists() {
        return Err(StoreError::new(CONFLICT, "A note with that name is back in memory already."));
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|e| io("put the note back", e))?;
    }
    fs::rename(dir.join(FILES).join("note.md"), &target).map_err(|e| io("put the note back", e))
}

fn restore_routine(root: &Path, record: &Record) -> StoreResult<()> {
    let routine = record.routine.clone().ok_or_else(not_found)?;
    store::require_bot(root, &routine.bot).map_err(|_| StoreError::new(CONFLICT, "Bring the bot back first, then this routine."))?;
    let mut all = load_routines(root)?;
    if all.iter().any(|r| r.id == routine.id) {
        return Err(StoreError::new(CONFLICT, "That routine is back already."));
    }
    all.push(routine);
    save_routines(root, &all)
}

fn restore_bot(root: &Path, record: &Record, dir: &Path) -> StoreResult<()> {
    let bot = record.bot_record.clone().ok_or_else(not_found)?;
    let mut bots = store::load_bots(root)?;
    if bots.iter().any(|b| b.id == bot.id) {
        return Err(StoreError::new(CONFLICT, "A bot with that name is back already. Rename or delete it first."));
    }
    if bots.len() >= store::MAX_BOTS {
        return Err(StoreError::new(CONFLICT, format!("You already have {} bots. Delete one first.", store::MAX_BOTS)));
    }
    let chat = store::conversation_path(root, &store::app_conversation(&bot.id))?;
    let targets = |name: &str| -> StoreResult<PathBuf> {
        Ok(match name {
            "chat.jsonl" => chat.clone(),
            "chat.1.jsonl" => chat.with_extension("1.jsonl"),
            "settings.json" => bot_settings::path(root, &bot.id)?,
            _ => store::brain_dir(root, &bot.id)?,
        })
    };
    let mut moves = Vec::new();
    for name in record.files.iter().filter(|n| FILE_NAMES.contains(&n.as_str())) {
        let to = targets(name)?;
        if to.exists() {
            return Err(StoreError::new(CONFLICT, "A file of that bot is in place already."));
        }
        moves.push((dir.join(FILES).join(name), to));
    }
    for (from, to) in moves {
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent).map_err(|e| io("put the bot back", e))?;
        }
        fs::rename(&from, &to).map_err(|e| io("put the bot back", e))?;
    }
    bots.push(bot.clone());
    store::save_bots(root, &bots)?;
    let mut all = load_routines(root)?;
    all.extend(record.bot_routines.iter().cloned());
    save_routines(root, &all)?;
    if let Some(mark) = record.read_mark {
        let mut reads = store::load_reads(root)?;
        reads.insert(bot.id.clone(), mark);
        store::write_json(&store::reads_path(root), &reads, "reads.json")?;
    }
    Ok(())
}

// ---- purging -----------------------------------------------------------------------------------

/// Removes every item deleted at or before `now - 7 days`. Returns how many went. An item whose record cannot be
/// read is left alone.
pub fn purge_expired(root: &Path, now_ms: i64) -> StoreResult<usize> {
    let _files = store::files_lock();
    let Ok(entries) = fs::read_dir(trash_root(root)) else { return Ok(0) };
    let mut gone = 0;
    for entry in entries.flatten() {
        let dir = entry.path();
        let Some(record) = read_record(&dir) else { continue };
        if record.item.deleted_at <= now_ms - KEEP_MS && fs::remove_dir_all(&dir).is_ok() {
            gone += 1;
        }
    }
    Ok(gone)
}

/// The daily purge: runs at most once a day, the first time it is asked (so at app start).
pub fn maybe_purge(root: &Path, now_ms: i64) {
    let last = LAST_PURGE_MS.load(Ordering::SeqCst);
    if last != i64::MIN && now_ms - last < DAY_MS {
        return;
    }
    LAST_PURGE_MS.store(now_ms, Ordering::SeqCst);
    if let Err(e) = purge_expired(root, now_ms) {
        tracing::warn!(%e, "could not purge the recently deleted items; they will be tried again tomorrow");
    }
}
