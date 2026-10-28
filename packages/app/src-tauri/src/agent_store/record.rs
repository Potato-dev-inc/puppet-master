//! The Store record file (`agent-store.json`): what the Store added, installed and checked.
//!
//! The record holds only catalog ids, install method and package, program versions, dates and
//! counts. It never holds a secret, a name, a screen or a path. `mark_*` refuse a value that is not
//! plain text (a path, a `~` home folder, a backslash, a colon, a space), `save` refuses a record
//! that has one, and `load` drops any agent entry that has one. Fields not listed here are dropped
//! on save, so a hand-edited extra field never comes back. Writes go through
//! `secure_file::write_private` (temp file, then rename). A missing or damaged file reads as empty.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::secure_file;
use crate::tui::install::METHODS;

/// The only `v` this code writes.
pub const RECORD_VERSION: u32 = 1;
/// The file name in the app data folder.
pub const FILE_NAME: &str = "agent-store.json";

/// The whole file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StoreRecord {
    pub v: u32,
    /// One entry per catalog id the Store has touched.
    pub agents: BTreeMap<String, AgentRecord>,
}

/// What the Store did for one catalog agent.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentRecord {
    /// `YYYY-MM-DD` when the Store added the agent to Puppet Master; empty = not added.
    pub added: String,
    /// The catalog version (8 hex) the added definition came from; empty when not added.
    pub catalog: String,
    /// How the Store installed the program, if it did.
    pub install: Option<InstallRecord>,
    /// The last "Verify it works" result.
    pub check: Option<CheckRecord>,
}

/// An install the Store ran through a visible, confirmed command.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct InstallRecord {
    /// One of `tui::install::METHODS`.
    pub method: String,
    /// The winget id, npm name or PyPI name the method used; empty for a script install.
    pub package: String,
    /// The program version after the install (number only), or empty.
    pub version: String,
    /// `YYYY-MM-DD`.
    pub date: String,
}

/// A "Verify it works" run: numbers and the program version only.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CheckRecord {
    /// `YYYY-MM-DD`.
    pub date: String,
    /// The program version the check ran against (number only), or empty.
    pub version: String,
    pub passed: u32,
    pub total: u32,
}

impl StoreRecord {
    /// The Store added this agent to Puppet Master, from the catalog version `catalog` (8 hex).
    pub fn mark_added(&mut self, id: &str, date: &str, catalog: &str) -> Result<(), String> {
        check_id(id)?;
        check_date(date)?;
        if !is_hex8(catalog) {
            return Err("catalog: not a catalog version".into());
        }
        let rec = self.agents.entry(id.to_string()).or_default();
        rec.added = date.to_string();
        rec.catalog = catalog.to_string();
        Ok(())
    }

    /// The Store installed the program for this agent.
    pub fn mark_install(&mut self, id: &str, install: InstallRecord) -> Result<(), String> {
        check_id(id)?;
        check_install(&install)?;
        self.agents.entry(id.to_string()).or_default().install = Some(install);
        Ok(())
    }

    /// A "Verify it works" run finished for this agent.
    pub fn mark_check(&mut self, id: &str, check: CheckRecord) -> Result<(), String> {
        check_id(id)?;
        check_check(&check)?;
        self.agents.entry(id.to_string()).or_default().check = Some(check);
        Ok(())
    }

    /// The Store removed the agent from Puppet Master. The install record stays, so an uninstall
    /// still knows how the program was put on this PC. An entry with nothing left is dropped.
    pub fn mark_removed(&mut self, id: &str) {
        let Some(rec) = self.agents.get_mut(id) else {
            return;
        };
        rec.added.clear();
        rec.catalog.clear();
        if rec.install.is_none() && rec.check.is_none() {
            self.agents.remove(id);
        }
    }
}

/// The Store file in the app data folder, next to `agent-approvals.json`.
pub fn default_path() -> PathBuf {
    crate::app_paths::app_data_dir().join(FILE_NAME)
}

/// Reads the record. A missing file or broken JSON reads as empty. An agent entry that holds a value
/// which is not plain text is dropped; the other entries still load.
pub fn load(path: &Path) -> StoreRecord {
    let Ok(text) = fs::read_to_string(path) else {
        return StoreRecord::default();
    };
    let Ok(mut rec) = serde_json::from_str::<StoreRecord>(&text) else {
        return StoreRecord::default();
    };
    rec.agents.retain(|id, agent| agent_ok(id, agent));
    rec
}

/// Writes the record atomically. Refuses, and writes nothing, when an entry holds a value that is
/// not plain text. The file always carries `RECORD_VERSION`.
pub fn save(path: &Path, rec: &StoreRecord) -> Result<(), String> {
    if !rec.agents.iter().all(|(id, agent)| agent_ok(id, agent)) {
        return Err("agent store: an entry holds a value that is not plain text".into());
    }
    let file = StoreRecord {
        v: RECORD_VERSION,
        agents: rec.agents.clone(),
    };
    let text = serde_json::to_string_pretty(&file).map_err(|err| format!("agent store: {err}"))?;
    secure_file::write_private(path, &text)
}

fn agent_ok(id: &str, agent: &AgentRecord) -> bool {
    check_id(id).is_ok()
        && (agent.added.is_empty() || check_date(&agent.added).is_ok())
        && (agent.catalog.is_empty() || is_hex8(&agent.catalog))
        && agent
            .install
            .as_ref()
            .map_or(true, |install| check_install(install).is_ok())
        && agent
            .check
            .as_ref()
            .map_or(true, |check| check_check(check).is_ok())
}

fn check_id(id: &str) -> Result<(), String> {
    let starts_ok = id
        .as_bytes()
        .first()
        .is_some_and(|b| b.is_ascii_lowercase());
    let ok = starts_ok
        && id.len() <= 40
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if ok {
        Ok(())
    } else {
        Err("agent id: not a catalog id".into())
    }
}

/// `YYYY-MM-DD`, shape only.
fn check_date(date: &str) -> Result<(), String> {
    let ok = date.len() == 10
        && date.bytes().enumerate().all(|(i, c)| match i {
            4 | 7 => c == b'-',
            _ => c.is_ascii_digit(),
        });
    if ok {
        Ok(())
    } else {
        Err("date: must be YYYY-MM-DD".into())
    }
}

fn check_install(install: &InstallRecord) -> Result<(), String> {
    if !METHODS.contains(&install.method.as_str()) {
        return Err("install.method: not a known install method".into());
    }
    if !install.package.is_empty() && !is_plain(&install.package, 100) {
        return Err("install.package: not a package name".into());
    }
    if !install.version.is_empty() && !is_plain(&install.version, 40) {
        return Err("install.version: not a version number".into());
    }
    check_date(&install.date)
}

fn check_check(check: &CheckRecord) -> Result<(), String> {
    if !check.version.is_empty() && !is_plain(&check.version, 40) {
        return Err("check.version: not a version number".into());
    }
    if check.passed > check.total {
        return Err("check: passed is more than total".into());
    }
    check_date(&check.date)
}

/// Letters, digits and `._-+`, plus one `/` only as an npm scope (`@scope/name`). Refuses a path
/// (`C:\`, `~/`, `/home`), a space, a control character and a value with no letter or digit.
fn is_plain(text: &str, max: usize) -> bool {
    let slashes = text.matches('/').count();
    !text.is_empty()
        && text.len() <= max
        && text.chars().any(|c| c.is_ascii_alphanumeric())
        && (slashes == 0 || (slashes == 1 && text.starts_with('@')))
        && text.chars().all(|c| {
            c == '/' || c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+' | '@')
        })
}

fn is_hex8(text: &str) -> bool {
    text.len() == 8 && text.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("pm-store-record-{}-{name}", std::process::id()))
    }

    fn install(method: &str, package: &str) -> InstallRecord {
        InstallRecord {
            method: method.into(),
            package: package.into(),
            version: "2.1.0".into(),
            date: "2026-10-28".into(),
        }
    }

    fn check(passed: u32, total: u32) -> CheckRecord {
        CheckRecord {
            date: "2026-10-28".into(),
            version: "0.4.1".into(),
            passed,
            total,
        }
    }

    fn full_record() -> StoreRecord {
        let mut rec = StoreRecord::default();
        rec.mark_added("qwen", "2026-10-28", "1a2b3c4d").unwrap();
        rec.mark_install("qwen", install("npm", "@qwen/cli"))
            .unwrap();
        rec.mark_check("qwen", check(6, 7)).unwrap();
        rec
    }

    #[test]
    fn round_trip_keeps_every_field() {
        let dir = scratch("round-trip");
        let path = dir.join(FILE_NAME);
        let rec = full_record();
        save(&path, &rec).unwrap();
        let back = load(&path);
        assert_eq!(back.agents, rec.agents);
        assert_eq!(back.v, RECORD_VERSION);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_missing_file_reads_as_empty() {
        let path = scratch("missing").join(FILE_NAME);
        assert_eq!(load(&path), StoreRecord::default());
    }

    #[test]
    fn broken_json_reads_as_empty() {
        let dir = scratch("broken");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FILE_NAME);
        fs::write(&path, "{ not json, agents: [").unwrap();
        assert_eq!(load(&path), StoreRecord::default());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_damaged_entry_is_dropped_and_the_rest_loads() {
        let dir = scratch("damaged-entry");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FILE_NAME);
        let text = r#"{"v":1,"agents":{
            "qwen":{"added":"2026-10-28","catalog":"1a2b3c4d"},
            "kiro":{"install":{"method":"npm","package":"C:\\Users\\you\\kiro","version":"1.0.0","date":"2026-10-28"}},
            "Bad Id":{"added":"2026-10-28"}}}"#;
        fs::write(&path, text).unwrap();
        let rec = load(&path);
        assert_eq!(rec.agents.keys().collect::<Vec<_>>(), vec!["qwen"]);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_path_is_refused_by_every_helper() {
        let mut rec = StoreRecord::default();
        for bad in [
            "C:\\Users\\you\\.npm",
            "~/.npm",
            "/home/you/x",
            "users/you",
            "..",
            "a:b",
            "x y",
        ] {
            assert!(
                rec.mark_install("qwen", install("npm", bad)).is_err(),
                "package {bad}"
            );
            assert!(
                rec.mark_added("qwen", "2026-10-28", bad).is_err(),
                "catalog {bad}"
            );
            let mut bad_version = install("npm", "qwen");
            bad_version.version = bad.into();
            assert!(
                rec.mark_install("qwen", bad_version).is_err(),
                "version {bad}"
            );
        }
        assert!(
            rec.agents.is_empty(),
            "a refused value leaves no entry behind"
        );
        assert!(rec
            .mark_install("qwen", install("npm", "@qwen/cli"))
            .is_ok());
        assert!(rec.mark_install("qwen", install("script", "")).is_ok());
    }

    #[test]
    fn an_unknown_method_and_a_bad_date_or_id_are_refused() {
        let mut rec = StoreRecord::default();
        assert!(rec.mark_install("qwen", install("sudo", "qwen")).is_err());
        assert!(rec.mark_added("qwen", "28-10-2026", "1a2b3c4d").is_err());
        assert!(rec
            .mark_added("Qwen Code", "2026-10-28", "1a2b3c4d")
            .is_err());
        assert!(rec.mark_added("qwen", "2026-10-28", "not-hex").is_err());
        assert!(
            rec.mark_check("qwen", check(8, 7)).is_err(),
            "passed above total"
        );
    }

    #[test]
    fn save_refuses_a_record_that_holds_a_path_and_writes_nothing() {
        let dir = scratch("refuse-save");
        let path = dir.join(FILE_NAME);
        let mut rec = full_record();
        rec.agents
            .get_mut("qwen")
            .unwrap()
            .install
            .as_mut()
            .unwrap()
            .package = "C:\\Users\\you".into();
        assert!(save(&path, &rec).is_err());
        assert!(!path.exists());
    }

    #[test]
    fn no_field_can_hold_a_path() {
        // The file has a fixed shape: the field names below are the only keys it can ever have,
        // and every text value must be plain (a date, an id, a package, a version).
        let text = serde_json::to_string(&full_record()).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        let keys = |v: &serde_json::Value| -> Vec<String> {
            let mut out: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
            out.sort();
            out
        };
        assert_eq!(keys(&value), ["agents", "v"]);
        let agent = &value["agents"]["qwen"];
        assert_eq!(keys(agent), ["added", "catalog", "check", "install"]);
        assert_eq!(
            keys(&agent["install"]),
            ["date", "method", "package", "version"]
        );
        assert_eq!(
            keys(&agent["check"]),
            ["date", "passed", "total", "version"]
        );
        let texts = [
            &agent["added"],
            &agent["catalog"],
            &agent["install"]["method"],
            &agent["install"]["package"],
            &agent["install"]["version"],
            &agent["install"]["date"],
            &agent["check"]["date"],
            &agent["check"]["version"],
        ];
        for s in texts {
            let s = s.as_str().unwrap();
            assert!(
                is_plain(s, 100) || check_date(s).is_ok() || is_hex8(s),
                "value {s:?} is not plain text"
            );
        }
        assert!(!text.contains("Users") && !text.contains('\\'));
    }

    #[test]
    fn a_removed_agent_keeps_its_install_record() {
        let mut rec = full_record();
        rec.mark_removed("qwen");
        let agent = &rec.agents["qwen"];
        assert!(agent.added.is_empty() && agent.catalog.is_empty());
        assert_eq!(agent.install.as_ref().unwrap().package, "@qwen/cli");

        let mut only_added = StoreRecord::default();
        only_added
            .mark_added("kiro", "2026-10-28", "1a2b3c4d")
            .unwrap();
        only_added.mark_removed("kiro");
        assert!(only_added.agents.is_empty());
    }

    #[test]
    fn an_extra_field_from_a_hand_edit_is_dropped_on_save() {
        let dir = scratch("extra-field");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FILE_NAME);
        fs::write(
            &path,
            r#"{"v":1,"homeDir":"C:\\Users\\you","agents":{"qwen":{"added":"2026-10-28","catalog":"1a2b3c4d","note":"C:\\Users\\you"}}}"#,
        )
        .unwrap();
        let rec = load(&path);
        save(&path, &rec).unwrap();
        let saved = fs::read_to_string(&path).unwrap();
        assert!(!saved.contains("homeDir") && !saved.contains("Users") && !saved.contains("note"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn default_path_is_in_the_app_data_folder() {
        let path = default_path();
        assert!(path.is_absolute());
        assert_eq!(path.file_name().unwrap(), FILE_NAME);
    }
}
