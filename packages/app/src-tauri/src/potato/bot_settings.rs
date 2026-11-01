//! Per-bot settings of Potato Bot (the bot panel's Advanced tab): which talk brain the bot uses (or the
//! default), its extra instructions with their history, and its tool switches. One small file per bot,
//! `<app data>/potato/bot-settings/<bot>.json`, written under the store's lock.
//!
//! - the brain: `None` is "Use the default" (the Settings brain). The engine (`service.rs`) reads it when a
//!   brain starts for that bot;
//! - the instructions: every save is a version (newest last, at most 50). Restore adds the old text as a new
//!   version, so nothing is ever lost. The text in use is written to `brain/<bot>/persona/instructions.md` when
//!   a brain starts, next to the persona files, and the brain reads it like them;
//! - the tools: three switches, all on by default. `helpers` = local helper agents (`ask_helper`); `computer` =
//!   the sandbox screen tools; `web` = the sandbox browser tools. The engine checks them before a tool call runs.
//!
//! Deleting a bot moves its chat, its settings and its brain folder into `potato/.removed/`, never deletes them.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::TimeZone;
use serde::{Deserialize, Serialize};

use super::store::{self, BrainKind, Message, StoreError, StoreResult};
use crate::secure_file;

/// The most characters one instruction version may hold (the brain reads it as persona text).
pub const MAX_INSTRUCTION_CHARS: usize = 4000;
/// Versions kept per bot; the oldest go first.
pub const MAX_VERSIONS: usize = 50;
/// The most characters a bot's name may have (the companion name cap).
pub const MAX_NAME_CHARS: usize = 40;
/// The file the brain reads for a bot's extra instructions, in its persona folder.
pub const INSTRUCTIONS_FILE: &str = "instructions.md";
const SETTINGS_VERSION: u32 = 1;
/// The most messages a chat export reads (the old file and the current one together).
pub const EXPORT_MESSAGES: usize = 10_000;

/// The three tool switches of a bot. All on by default; a switch that is off refuses its tools at the engine.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct ToolSwitches {
    /// Local helper agents: `ask_helper`.
    pub helpers: bool,
    /// The sandbox screen: `computer_observe`, `computer_act`, `ask_person_for_screen`.
    pub computer: bool,
    /// Reading the web in the sandbox browser: `browser_navigate`, `browser_snapshot`, `browser_act`.
    pub web: bool,
}

impl Default for ToolSwitches {
    fn default() -> Self {
        ToolSwitches { helpers: true, computer: true, web: true }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InstructionVersion {
    /// When it was saved (epoch ms). Unique within a bot, and the key Restore uses.
    pub at: i64,
    pub text: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct BotSettings {
    pub v: u32,
    /// The bot's own talk brain. `None`: the default in Settings.
    pub brain: Option<BrainKind>,
    pub tools: ToolSwitches,
    /// Oldest first. The last one is the text in use (empty when none was ever saved).
    pub instructions: Vec<InstructionVersion>,
}

/// What the Advanced tab reads in one call. Versions come newest first.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BotOptions {
    pub brain: Option<BrainKind>,
    pub tools: ToolSwitches,
    /// The sandbox Labs switch is on: the computer switch is shown only then.
    pub sandbox_on: bool,
    pub instructions: String,
    pub versions: Vec<InstructionVersion>,
}

/// A brain's name in a note, as the person reads it. Generic code names no agent or provider (see `guard_tests`), so the
/// Go plan and the person's agents are named by what they are for; the Settings list has the full labels.
pub fn brain_name(kind: BrainKind) -> &'static str {
    match kind {
        BrainKind::Deepseek => "DeepSeek",
        BrainKind::Go => "your Go plan",
        BrainKind::Agents => "your agents",
    }
}

pub fn settings_dir(root: &Path) -> PathBuf {
    root.join("bot-settings")
}

/// `<root>/bot-settings/<bot>.json`; refused for an id that is not well formed.
pub fn path(root: &Path, bot: &str) -> StoreResult<PathBuf> {
    if !crate::companion::model::valid_id(bot) {
        return Err(StoreError::new("POTATO_BOT_NOT_FOUND", "No bot has that name."));
    }
    Ok(settings_dir(root).join(format!("{bot}.json")))
}

/// A missing file reads as the defaults; a damaged one is an error (and is never overwritten).
pub fn load(root: &Path, bot: &str) -> StoreResult<BotSettings> {
    Ok(store::read_json(&path(root, bot)?, "the bot's settings")?.unwrap_or_default())
}

fn save(root: &Path, bot: &str, settings: &BotSettings) -> StoreResult<()> {
    let target = path(root, bot)?;
    secure_file::create_private_dir(&settings_dir(root)).map_err(|e| StoreError::new("POTATO_STORE_INVALID", format!("Could not make the settings folder: {e}.")))?;
    let mut copy = settings.clone();
    copy.v = SETTINGS_VERSION;
    store::write_json(&target, &copy, "the bot's settings")
}

/// Read, change and write under the store's one lock, so two changes cannot lose each other.
pub fn update(root: &Path, bot: &str, change: impl FnOnce(&mut BotSettings) -> StoreResult<()>) -> StoreResult<BotSettings> {
    let _files = store::files_lock();
    let mut settings = load(root, bot)?;
    change(&mut settings)?;
    save(root, bot, &settings)?;
    Ok(settings)
}

/// The brain a bot starts with: its own, or the default.
pub fn effective_brain(default: BrainKind, settings: &BotSettings) -> BrainKind {
    settings.brain.unwrap_or(default)
}

// ---- instructions -------------------------------------------------------------------------------

/// The text in use: the newest version, or nothing.
pub fn current_instructions(settings: &BotSettings) -> &str {
    settings.instructions.last().map(|v| v.text.as_str()).unwrap_or("")
}

/// Trims the text and checks its length. The check is on what the person typed, before any cleaning.
pub fn clean_instructions(raw: &str) -> StoreResult<String> {
    let trimmed = raw.trim();
    if trimmed.chars().count() > MAX_INSTRUCTION_CHARS {
        return Err(StoreError::new("POTATO_INSTRUCTIONS_TOO_LONG", format!("Instructions can be {MAX_INSTRUCTION_CHARS} characters at most.")));
    }
    Ok(store::clean_text(trimmed, None))
}

/// Adds a version with `text`, unless it is the text in use already. Returns whether a version was added.
pub fn save_instructions(settings: &mut BotSettings, raw: &str, now: i64) -> StoreResult<bool> {
    let text = clean_instructions(raw)?;
    if text == current_instructions(settings) {
        return Ok(false);
    }
    push_version(settings, text, now);
    Ok(true)
}

/// Makes the version saved at `at` the one in use, by adding its text as a new version. Returns whether that
/// changed anything (restoring the text already in use does nothing).
pub fn restore_instructions(settings: &mut BotSettings, at: i64, now: i64) -> StoreResult<bool> {
    let text = settings
        .instructions
        .iter()
        .find(|v| v.at == at)
        .map(|v| v.text.clone())
        .ok_or_else(|| StoreError::new("POTATO_VERSION_NOT_FOUND", "That version of the instructions is gone."))?;
    if text == current_instructions(settings) {
        return Ok(false);
    }
    push_version(settings, text, now);
    Ok(true)
}

fn push_version(settings: &mut BotSettings, text: String, now: i64) {
    // Two saves in the same millisecond still get two different keys.
    let at = match settings.instructions.last() {
        Some(last) if last.at >= now => last.at + 1,
        _ => now,
    };
    settings.instructions.push(InstructionVersion { at, text });
    let extra = settings.instructions.len().saturating_sub(MAX_VERSIONS);
    settings.instructions.drain(..extra);
}

/// The Advanced tab's view of a bot's settings: versions newest first.
pub fn options_of(settings: &BotSettings, sandbox_on: bool) -> BotOptions {
    BotOptions {
        brain: settings.brain,
        tools: settings.tools,
        sandbox_on,
        instructions: current_instructions(settings).to_string(),
        versions: settings.instructions.iter().rev().cloned().collect(),
    }
}

/// Writes the instructions in use into the bot's persona folder (`instructions.md`), or removes the file when
/// there are none, so a text that was cleared never comes back. Called when a brain starts.
pub fn write_instructions_file(persona_dir: &Path, settings: &BotSettings) -> StoreResult<()> {
    let file = persona_dir.join(INSTRUCTIONS_FILE);
    let text = current_instructions(settings);
    if text.is_empty() {
        return match fs::remove_file(&file) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(StoreError::new("POTATO_STORE_INVALID", format!("Could not remove the instructions file: {e}."))),
        };
    }
    secure_file::write_private(&file, text).map_err(|e| StoreError::new("POTATO_STORE_INVALID", e))
}

// ---- tools --------------------------------------------------------------------------------------

/// The sandbox tools, by the switch that gates them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Area {
    Computer,
    Web,
}

pub fn sandbox_tool_area(tool: &str) -> Option<Area> {
    match tool {
        "computer_observe" | "computer_act" | "ask_person_for_screen" => Some(Area::Computer),
        "browser_navigate" | "browser_snapshot" | "browser_act" => Some(Area::Web),
        _ => None,
    }
}

/// Whether a sandbox tool may run for a bot. A name that is not a sandbox tool is left to the tool layer
/// (it answers "no such tool").
pub fn sandbox_tool_on(tools: &ToolSwitches, tool: &str) -> bool {
    match sandbox_tool_area(tool) {
        Some(Area::Computer) => tools.computer,
        Some(Area::Web) => tools.web,
        None => true,
    }
}

// ---- names --------------------------------------------------------------------------------------

/// A bot's name as typed: trimmed, not empty, at most 40 characters.
pub fn clean_name(raw: &str) -> StoreResult<String> {
    let name = raw.trim();
    if name.is_empty() {
        return Err(StoreError::new("POTATO_NAME_EMPTY", "Give the bot a name."));
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(StoreError::new("POTATO_NAME_TOO_LONG", format!("Names can be {MAX_NAME_CHARS} characters at most.")));
    }
    Ok(name.to_string())
}

// ---- chat export --------------------------------------------------------------------------------

/// The chat as one Markdown file: a title, then one block per message, oldest first. The person's messages and
/// the bot's are labelled; notes are in italics. Times are local.
pub fn chat_markdown(bot_name: &str, messages: &[Message]) -> String {
    let mut out = format!("# Chat with {bot_name}\n\n");
    for m in messages {
        let when = chrono::Local
            .timestamp_millis_opt(m.at)
            .single()
            .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_default();
        let who = match m.from.kind {
            store::FromKind::You => "You".to_string(),
            store::FromKind::Note => String::new(),
            _ => m.from.name.clone().unwrap_or_else(|| bot_name.to_string()),
        };
        match m.from.kind {
            store::FromKind::Note => out.push_str(&format!("*{when} · {}*\n\n", m.text.trim())),
            _ => out.push_str(&format!("**{who}** · {when}\n\n{}\n\n", m.text.trim())),
        }
    }
    out
}

// ---- deleting -----------------------------------------------------------------------------------

/// Moves a bot's own store files (its chat with the old file, its settings, its read mark) into
/// `potato/.removed/<bot>-<stamp>-bot/`. The bot list in `bots.json` and the brain folder are the caller's.
pub fn move_bot_files(root: &Path, bot: &str, stamp: &str) -> StoreResult<PathBuf> {
    let _files = store::files_lock();
    let removed = root.join(".removed");
    secure_file::create_private_dir(&removed).map_err(|e| StoreError::new("POTATO_STORE_INVALID", format!("Could not make the removed folder: {e}.")))?;
    let stamp: String = stamp.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
    let to = removed.join(format!("{bot}-{stamp}-bot"));
    secure_file::create_private_dir(&to).map_err(|e| StoreError::new("POTATO_STORE_INVALID", format!("Could not make the removed folder: {e}.")))?;

    let chat = store::conversation_path(root, &store::app_conversation(bot))?;
    let old_chat = chat.with_extension("1.jsonl");
    let moves = [(chat, to.join("chat.jsonl")), (old_chat, to.join("chat.1.jsonl")), (path(root, bot)?, to.join("settings.json"))];
    for (from, dest) in moves {
        match fs::rename(&from, &dest) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(StoreError::new("POTATO_STORE_INVALID", format!("Could not move a file of the bot: {e}."))),
        }
    }

    let mut reads = store::load_reads(root)?;
    if reads.remove(bot).is_some() {
        store::write_json(&store::reads_path(root), &reads, "reads.json")?;
    }
    Ok(to)
}
