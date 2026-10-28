//! The home folder of a companion (spec 3.2): the three memory files, the pending list, the journal
//! folder and the marker that says "this folder is a companion home".
//!
//! `ensure_home` only creates what is missing. An existing file is never overwritten, so the
//! person's own edits to SOUL.md and friends survive every start.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use super::model::{CompanionError, CompanionProfile, CompanionResult, MemoryFile};
use super::{personas, store};

pub const PENDING_REL: &str = "memory/pending.md";
pub const JOURNAL_REL: &str = "memory/journal";
pub const MARKER_REL: &str = ".puppet-master/companion.json";

/// The default home for a companion: `<user home>\PuppetMaster\companions\<id>`.
pub fn default_folder(user_home: &Path, id: &str) -> PathBuf {
    user_home.join("PuppetMaster").join("companions").join(id)
}

/// The person's home folder from the environment (`USERPROFILE`, then `HOME`).
pub fn user_home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

/// Path of one of the three memory files inside a home folder.
pub fn file_path(home: &Path, file: MemoryFile) -> PathBuf {
    home.join(file.file_name())
}

pub fn pending_path(home: &Path) -> PathBuf {
    home.join(PENDING_REL)
}

/// Creates whatever is missing in the home folder and the app-data approved copies.
///
/// Refuses a folder that is already the home of another companion, and a folder that is not an
/// absolute path. Never overwrites an existing file.
pub fn ensure_home(root: &Path, profile: &CompanionProfile) -> CompanionResult<()> {
    let home = Path::new(&profile.folder);
    if !home.is_absolute() {
        return Err(folder_error("The folder must be a full path."));
    }
    crate::path_limit::check(home).map_err(|_| folder_error("That folder path is too long for Windows."))?;
    fs::create_dir_all(home).map_err(|e| folder_error(&format!("Could not create the folder: {e}.")))?;

    check_marker(home, &profile.id)?;
    create_if_missing(&home.join(".puppet-master").join("companion.json"), &marker_text(&profile.id))?;
    fs::create_dir_all(home.join(JOURNAL_REL)).map_err(|e| folder_error(&format!("Could not create memory/journal: {e}.")))?;

    let soul = personas::soul_template(&profile.persona).replace("\r\n", "\n");
    create_if_missing(&file_path(home, MemoryFile::Soul), &soul)?;
    create_if_missing(&file_path(home, MemoryFile::User), "")?;
    create_if_missing(&file_path(home, MemoryFile::Memory), "")?;
    create_if_missing(&pending_path(home), "")?;

    for file in MemoryFile::ALL {
        let approved = store::approved_path(root, &profile.id, file)?;
        if store::read_optional(&approved)?.is_none() {
            let current = fs::read_to_string(file_path(home, file)).unwrap_or_default();
            store::write_private_text(&approved, &current)?;
        }
    }
    Ok(())
}

fn marker_text(id: &str) -> String {
    format!("{{\n  \"id\": \"{id}\"\n}}\n")
}

/// The marker file must be absent (then it is written) or name this companion.
fn check_marker(home: &Path, id: &str) -> CompanionResult<()> {
    let path = home.join(MARKER_REL);
    let Some(text) = store::read_optional(&path)? else {
        return Ok(());
    };
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|_| folder_error("This folder has a damaged companion marker. Pick another folder."))?;
    match value.get("id").and_then(serde_json::Value::as_str) {
        Some(found) if found == id => Ok(()),
        _ => Err(folder_error("Another companion already lives in this folder. Pick another folder.")),
    }
}

/// Creates `path` with `text` unless it exists. An existing file is left exactly as it is.
fn create_if_missing(path: &Path, text: &str) -> CompanionResult<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| folder_error(&format!("Could not create {}: {e}.", dir.display())))?;
    }
    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut file) => file
            .write_all(text.as_bytes())
            .map_err(|e| folder_error(&format!("Could not write {}: {e}.", path.display()))),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(folder_error(&format!("Could not create {}: {e}.", path.display()))),
    }
}

fn folder_error(message: &str) -> CompanionError {
    CompanionError::new("COMPANION_FOLDER_INVALID", message)
}

#[cfg(test)]
mod tests {
    use super::super::model::HhMm;
    use super::*;

    fn temp(prefix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("{prefix}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn profile(id: &str, persona: &str, folder: &Path) -> CompanionProfile {
        CompanionProfile {
            id: id.into(),
            name: "Bit".into(),
            avatar: "bit".into(),
            persona: persona.into(),
            agent_type: "shell".into(),
            launch_args: vec![],
            account: None,
            folder: folder.display().to_string(),
            auto_start: true,
            memory_review: true,
            push_replies: true,
            share_today: false,
            nap_at: Some(HhMm { h: 4, m: 0 }),
            checkins: vec![],
            previous_agent: None,
            until_ms: None,
            created_ms: 1,
            archived: false,
            pane_ids: Vec::new(),
        }
    }

    #[test]
    fn creates_the_full_layout_once() {
        let root = temp("pm-companion-root");
        let home = temp("pm-companion-home").join("bit");
        let p = profile("bit", "coach", &home);
        ensure_home(&root, &p).unwrap();

        assert!(file_path(&home, MemoryFile::Soul).is_file());
        assert!(file_path(&home, MemoryFile::User).is_file());
        assert!(file_path(&home, MemoryFile::Memory).is_file());
        assert!(pending_path(&home).is_file());
        assert!(home.join(JOURNAL_REL).is_dir());
        assert!(home.join(MARKER_REL).is_file());
        let soul = fs::read_to_string(file_path(&home, MemoryFile::Soul)).unwrap();
        assert_eq!(soul, personas::COACH);
        for file in MemoryFile::ALL {
            assert!(store::approved_path(&root, "bit", file).unwrap().is_file());
        }
    }

    #[test]
    fn existing_files_are_never_overwritten() {
        let root = temp("pm-companion-root");
        let home = temp("pm-companion-home");
        fs::write(file_path(&home, MemoryFile::Soul), "My own voice.").unwrap();
        fs::write(file_path(&home, MemoryFile::Memory), "- note: keep me\n").unwrap();
        ensure_home(&root, &profile("bit", "cheerful", &home)).unwrap();
        assert_eq!(fs::read_to_string(file_path(&home, MemoryFile::Soul)).unwrap(), "My own voice.");
        assert_eq!(fs::read_to_string(file_path(&home, MemoryFile::Memory)).unwrap(), "- note: keep me\n");
        // A second start changes nothing either.
        ensure_home(&root, &profile("bit", "cheerful", &home)).unwrap();
        assert_eq!(fs::read_to_string(file_path(&home, MemoryFile::Soul)).unwrap(), "My own voice.");
    }

    #[test]
    fn a_folder_of_another_companion_is_refused() {
        let root = temp("pm-companion-root");
        let home = temp("pm-companion-home");
        ensure_home(&root, &profile("bit", "plain", &home)).unwrap();
        let err = ensure_home(&root, &profile("nib", "plain", &home)).unwrap_err();
        assert_eq!(err.code, "COMPANION_FOLDER_INVALID");
    }

    #[test]
    fn a_relative_folder_is_refused() {
        let root = temp("pm-companion-root");
        let mut p = profile("bit", "plain", Path::new("C:/x"));
        p.folder = "relative/bit".into();
        assert_eq!(ensure_home(&root, &p).unwrap_err().code, "COMPANION_FOLDER_INVALID");
    }

    #[test]
    fn default_folder_sits_under_puppet_master() {
        let f = default_folder(Path::new("C:\\Users\\you"), "bit");
        assert_eq!(f, Path::new("C:\\Users\\you").join("PuppetMaster").join("companions").join("bit"));
    }
}
