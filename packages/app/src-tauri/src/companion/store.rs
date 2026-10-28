//! Companion files in the app data folder (spec 3.1): `companions.json`, each companion's
//! `runtime.json`, its `feed.jsonl` and its `approved/` copies of the three memory files.
//!
//! Every function takes the root folder (the app data dir in the app, a temp dir in tests). Writes
//! go through `secure_file::write_private` (temp file, then rename). A missing file reads as empty;
//! a damaged file is an error, so nothing is silently overwritten.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::model::{valid_id, CompanionError, CompanionProfile, CompanionResult, FeedItem, MemoryFile};
use crate::secure_file;

pub const STORE_VERSION: u32 = 1;
pub const FEED_CAP: usize = 500;

const STORE_INVALID: &str = "COMPANION_STORE_INVALID";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CompanionsFile {
    pub v: u32,
    pub companions: Vec<CompanionProfile>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SentToday {
    /// Local date, `YYYY-MM-DD`.
    pub date: String,
    pub count: u32,
}

/// Per-companion state the keeper needs across restarts (`runtime.json`).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct Runtime {
    pub sleeping: bool,
    /// Times the pane was restarted by the keeper (epoch ms).
    pub restarts_ms: Vec<i64>,
    /// Last send time per check-in id (epoch ms).
    pub last_runs: BTreeMap<String, i64>,
    pub sent_today: SentToday,
    pub last_nap_ms: Option<i64>,
}

/// `<root>/companions.json`.
pub fn companions_path(root: &Path) -> PathBuf {
    root.join("companions.json")
}

/// `<root>/companions/<id>`, refused for an id that is not a slug (so no path can escape).
pub fn companion_dir(root: &Path, id: &str) -> CompanionResult<PathBuf> {
    if !valid_id(id) {
        return Err(CompanionError::new("COMPANION_NOT_FOUND", "No companion has that id."));
    }
    Ok(root.join("companions").join(id))
}

/// `<root>/companions/<id>/approved/<FILE>`: the last copy the person approved.
pub fn approved_path(root: &Path, id: &str, file: MemoryFile) -> CompanionResult<PathBuf> {
    Ok(companion_dir(root, id)?.join("approved").join(file.file_name()))
}

/// Every saved companion, in file order. A missing file is an empty list.
pub fn load(root: &Path) -> CompanionResult<Vec<CompanionProfile>> {
    let path = companions_path(root);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(CompanionError::new(STORE_INVALID, format!("Could not read companions.json: {e}."))),
    };
    let file: CompanionsFile = serde_json::from_str(&text)
        .map_err(|e| CompanionError::new(STORE_INVALID, format!("companions.json is damaged: {e}.")))?;
    if file.v > STORE_VERSION {
        return Err(CompanionError::new(
            STORE_INVALID,
            "companions.json was written by a newer Puppet Master.",
        ));
    }
    Ok(file.companions)
}

/// Writes the whole list (version 1), atomically.
pub fn save(root: &Path, companions: &[CompanionProfile]) -> CompanionResult<()> {
    let file = CompanionsFile { v: STORE_VERSION, companions: companions.to_vec() };
    let text = serde_json::to_string_pretty(&file)
        .map_err(|e| CompanionError::new(STORE_INVALID, format!("Could not save companions: {e}.")))?;
    secure_file::write_private(&companions_path(root), &text)
        .map_err(|e| CompanionError::new(STORE_INVALID, e))
}

/// A companion's runtime state. A missing file reads as the default.
pub fn runtime_load(root: &Path, id: &str) -> CompanionResult<Runtime> {
    let path = companion_dir(root, id)?.join("runtime.json");
    match fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text)
            .map_err(|e| CompanionError::new(STORE_INVALID, format!("runtime.json is damaged: {e}."))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Runtime::default()),
        Err(e) => Err(CompanionError::new(STORE_INVALID, format!("Could not read runtime.json: {e}."))),
    }
}

pub fn runtime_save(root: &Path, id: &str, runtime: &Runtime) -> CompanionResult<()> {
    let path = companion_dir(root, id)?.join("runtime.json");
    let text = serde_json::to_string_pretty(runtime)
        .map_err(|e| CompanionError::new(STORE_INVALID, format!("Could not save runtime: {e}.")))?;
    secure_file::write_private(&path, &text).map_err(|e| CompanionError::new(STORE_INVALID, e))
}

/// Appends one feed line, dropping the oldest lines beyond `FEED_CAP`.
pub fn feed_append(root: &Path, id: &str, item: &FeedItem) -> CompanionResult<()> {
    let path = companion_dir(root, id)?.join("feed.jsonl");
    let line = serde_json::to_string(item)
        .map_err(|e| CompanionError::new(STORE_INVALID, format!("Could not save feed item: {e}.")))?;
    let mut lines: Vec<String> = read_lines(&path)?;
    lines.push(line);
    if lines.len() > FEED_CAP {
        let drop = lines.len() - FEED_CAP;
        lines.drain(..drop);
    }
    let mut text = lines.join("\n");
    text.push('\n');
    secure_file::write_private(&path, &text).map_err(|e| CompanionError::new(STORE_INVALID, e))
}

/// Every readable feed line, oldest first. A damaged line is skipped, not an error.
pub fn feed_read(root: &Path, id: &str) -> CompanionResult<Vec<FeedItem>> {
    let path = companion_dir(root, id)?.join("feed.jsonl");
    Ok(read_lines(&path)?
        .iter()
        .filter_map(|line| serde_json::from_str::<FeedItem>(line).ok())
        .collect())
}

fn read_lines(path: &Path) -> CompanionResult<Vec<String>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text.lines().filter(|l| !l.trim().is_empty()).map(str::to_string).collect()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(CompanionError::new(STORE_INVALID, format!("Could not read {}: {e}.", path.display()))),
    }
}

/// Reads a text file; a missing file is None.
pub fn read_optional(path: &Path) -> CompanionResult<Option<String>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(CompanionError::new(STORE_INVALID, format!("Could not read {}: {e}.", path.display()))),
    }
}

/// Writes an app-data file (private permissions, atomic).
pub fn write_private_text(path: &Path, text: &str) -> CompanionResult<()> {
    secure_file::write_private(path, text).map_err(|e| CompanionError::new(STORE_INVALID, e))
}

#[cfg(test)]
mod tests {
    use super::super::model::{FeedKind, HhMm, When};
    use super::*;

    fn root() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pm-companion-store-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn profile(id: &str) -> CompanionProfile {
        CompanionProfile {
            id: id.into(),
            name: "Mochi".into(),
            avatar: "mochi".into(),
            persona: "calm".into(),
            agent_type: "shell".into(),
            launch_args: vec![],
            account: None,
            folder: "C:\\Users\\you\\PuppetMaster\\companions\\mochi".into(),
            auto_start: true,
            memory_review: true,
            push_replies: true,
            share_today: false,
            nap_at: Some(HhMm { h: 4, m: 0 }),
            checkins: vec![],
            created_ms: 1,
            archived: false,
        }
    }

    #[test]
    fn missing_store_loads_empty_and_round_trips() {
        let dir = root();
        assert!(load(&dir).unwrap().is_empty());
        save(&dir, &[profile("mochi")]).unwrap();
        assert_eq!(load(&dir).unwrap(), vec![profile("mochi")]);
        assert!(!dir.join("companions.json.tmp").exists());
    }

    #[test]
    fn unknown_fields_are_tolerated_and_defaults_apply() {
        let dir = root();
        let text = r#"{
          "v": 1,
          "futureTopLevel": true,
          "companions": [{
            "id": "bit", "name": "Bit", "avatar": "bit", "persona": "plain",
            "agentType": "shell", "folder": "C:\\x\\bit", "createdMs": 5,
            "someNewField": {"a": 1}
          }]
        }"#;
        fs::write(companions_path(&dir), text).unwrap();
        let list = load(&dir).unwrap();
        assert_eq!(list.len(), 1);
        assert!(list[0].auto_start && list[0].memory_review && !list[0].archived);
        assert_eq!(list[0].nap_at, Some(HhMm { h: 4, m: 0 }), "nap defaults to 04:00");
        assert!(list[0].checkins.is_empty());
    }

    #[test]
    fn newer_store_version_and_damaged_file_are_refused() {
        let dir = root();
        fs::write(companions_path(&dir), r#"{"v": 2, "companions": []}"#).unwrap();
        assert_eq!(load(&dir).unwrap_err().code, "COMPANION_STORE_INVALID");
        fs::write(companions_path(&dir), "{ not json").unwrap();
        assert_eq!(load(&dir).unwrap_err().code, "COMPANION_STORE_INVALID");
    }

    #[test]
    fn checkins_survive_a_round_trip_in_camel_case() {
        let dir = root();
        let mut p = profile("mochi");
        p.checkins.push(super::super::model::CheckIn {
            id: "0a1b2c3d".into(),
            label: "Brief".into(),
            text: "Morning news?".into(),
            when: When::Once { at_ms: 99 },
            enabled: true,
        });
        save(&dir, std::slice::from_ref(&p)).unwrap();
        let text = fs::read_to_string(companions_path(&dir)).unwrap();
        assert!(text.contains("\"atMs\": 99"), "{text}");
        assert!(text.contains("\"agentType\""), "{text}");
        assert_eq!(load(&dir).unwrap(), vec![p]);
    }

    #[test]
    fn runtime_defaults_and_round_trip() {
        let dir = root();
        assert_eq!(runtime_load(&dir, "mochi").unwrap(), Runtime::default());
        let mut rt = Runtime::default();
        rt.sleeping = true;
        rt.restarts_ms = vec![1, 2];
        rt.last_runs.insert("0a1b2c3d".into(), 7);
        rt.sent_today = SentToday { date: "2026-10-28".into(), count: 3 };
        runtime_save(&dir, "mochi", &rt).unwrap();
        assert_eq!(runtime_load(&dir, "mochi").unwrap(), rt);
    }

    #[test]
    fn feed_keeps_the_newest_500_lines() {
        let dir = root();
        for i in 0..(FEED_CAP as i64 + 5) {
            let item = FeedItem::new(FeedKind::Checkin, i, &format!("ask {i}"));
            feed_append(&dir, "mochi", &item).unwrap();
        }
        let items = feed_read(&dir, "mochi").unwrap();
        assert_eq!(items.len(), FEED_CAP);
        assert_eq!(items[0].at_ms, 5, "the five oldest were dropped");
        assert_eq!(items.last().unwrap().at_ms, FEED_CAP as i64 + 4);
    }

    #[test]
    fn damaged_feed_line_is_skipped() {
        let dir = root();
        let path = companion_dir(&dir, "mochi").unwrap().join("feed.jsonl");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "garbage\n").unwrap();
        feed_append(&dir, "mochi", &FeedItem::new(FeedKind::Woke, 1, "Awake")).unwrap();
        let items = feed_read(&dir, "mochi").unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, FeedKind::Woke);
    }

    #[test]
    fn ids_that_are_not_slugs_never_become_paths() {
        let dir = root();
        assert!(companion_dir(&dir, "../etc").is_err());
        assert!(approved_path(&dir, "Mochi", MemoryFile::Soul).is_err());
        assert!(runtime_load(&dir, "a/b").is_err());
    }
}
