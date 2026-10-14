//! "Screens I didn't recognise": a screen that stayed unreadable on a live pane, saved so it can
//! become a fixture and a rule. Everything is redacted BEFORE it touches disk, with the same
//! helper the session recorder uses (`agent_definitions_recorder::redact`), so no secret, token
//! or user-folder name reaches the file.
//!
//! Layout: `<app data>/captures/<agent>/<version>/<stamp>_<hash>.txt` (the screen, plain text, the
//! shape the fixtures use) with a `.json` beside it (agent, version, when, how long it stayed
//! unreadable). Nothing is ever uploaded.

use crate::agent_definitions_recorder::redact::{redact, RedactContext};
use once_cell::sync::Lazy;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

/// At most this many screens are kept per agent and version.
pub const MAX_PER_VERSION: usize = 25;
/// A screen is cut to this many bytes (a terminal grid is far smaller; this is a guard).
const MAX_BYTES: usize = 64 * 1024;

static CTX: Lazy<RedactContext> = Lazy::new(RedactContext::from_process);

/// The redaction every kept screen goes through.
pub fn redact_screen(text: &str) -> String {
    redact(text, &CTX)
}

pub fn root() -> PathBuf {
    crate::app_paths::app_data_dir().join("captures")
}

fn safe(part: &str) -> String {
    let clean: String = part
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '_' })
        .collect();
    let clean = clean.trim_matches('.').to_string();
    if clean.is_empty() { "unknown".into() } else { clean }
}

fn hash8(text: &str) -> String {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    format!("{:08x}", hasher.finish() as u32)
}

pub fn dir_for(root: &Path, agent: &str, version: Option<&str>) -> PathBuf {
    root.join(safe(agent)).join(safe(version.unwrap_or("unknown")))
}

/// Save one screen under `root`. Returns the path written, or `None` when the screen is blank,
/// already saved, or the cap for this agent and version is reached.
pub fn save(
    root: &Path,
    agent: &str,
    version: Option<&str>,
    screen: &str,
    unreadable_for_ms: u64,
    at_ms: i64,
) -> Option<PathBuf> {
    if screen.trim().is_empty() {
        return None;
    }
    let mut kept = redact_screen(screen);
    if kept.len() > MAX_BYTES {
        let mut cut = MAX_BYTES;
        while !kept.is_char_boundary(cut) {
            cut -= 1;
        }
        kept.truncate(cut);
    }
    let dir = dir_for(root, agent, version);
    std::fs::create_dir_all(&dir).ok()?;
    let id = hash8(&kept);
    let existing: Vec<_> = std::fs::read_dir(&dir)
        .ok()?
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.ends_with(".txt"))
        .collect();
    if existing.iter().any(|n| n.contains(&id)) || existing.len() >= MAX_PER_VERSION {
        return None;
    }
    let path = dir.join(format!("{at_ms}_{id}.txt"));
    std::fs::write(&path, &kept).ok()?;
    let meta = serde_json::json!({
        "agent": agent,
        "version": version,
        "at_ms": at_ms,
        "unreadable_for_ms": unreadable_for_ms,
        "note": "Redacted. Saved because the screen stayed unreadable on a live pane.",
    });
    let _ = std::fs::write(path.with_extension("json"), serde_json::to_vec_pretty(&meta).unwrap_or_default());
    Some(path)
}

/// Screens saved for one agent (all versions) when `agent` is given, else all.
pub fn count(root: &Path, agent: Option<&str>) -> usize {
    let agents: Vec<PathBuf> = match agent {
        Some(agent) => vec![root.join(safe(agent))],
        None => std::fs::read_dir(root)
            .map(|rd| rd.flatten().map(|e| e.path()).collect())
            .unwrap_or_default(),
    };
    agents
        .iter()
        .filter_map(|dir| std::fs::read_dir(dir).ok())
        .flat_map(|rd| rd.flatten())
        .filter_map(|version| std::fs::read_dir(version.path()).ok())
        .flat_map(|rd| rd.flatten())
        .filter(|e| e.path().extension().is_some_and(|x| x == "txt"))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pm-captures-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_saved_screen_is_redacted_before_it_reaches_the_disk() {
        let root = temp("redact");
        let screen = "Welcome\nexport OPENAI_API_KEY=sk-live-abcdefghijklmnopqrstuvwxyz\nFolder: C:\\Users\\SomeoneElse\\project\nghp_abcdefghijklmnop1234\n";
        let path = save(&root, "codex", Some("0.162.1"), screen, 7000, 100).expect("saved");
        let on_disk = std::fs::read_to_string(&path).unwrap();
        assert!(!on_disk.contains("sk-live"), "{on_disk}");
        assert!(!on_disk.contains("ghp_"), "{on_disk}");
        assert!(!on_disk.contains("SomeoneElse"), "{on_disk}");
        assert!(on_disk.contains("[REDACTED]"), "{on_disk}");
        assert!(on_disk.contains("Welcome"));
        // Nothing unredacted sits beside it either.
        let meta = std::fs::read_to_string(path.with_extension("json")).unwrap();
        assert!(!meta.contains("sk-live"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_same_screen_is_saved_once_and_a_blank_one_never() {
        let root = temp("dedupe");
        assert!(save(&root, "codex", Some("0.162.1"), "menu A", 6000, 1).is_some());
        assert!(save(&root, "codex", Some("0.162.1"), "menu A", 6000, 2).is_none());
        assert!(save(&root, "codex", Some("0.162.1"), "  \n ", 6000, 3).is_none());
        assert!(save(&root, "codex", Some("0.162.1"), "menu B", 6000, 4).is_some());
        assert_eq!(count(&root, Some("codex")), 2);
        assert_eq!(count(&root, None), 2);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_version_stops_at_the_cap() {
        let root = temp("cap");
        for i in 0..(MAX_PER_VERSION + 5) {
            save(&root, "codex", Some("0.162.1"), &format!("screen number {i}"), 6000, i as i64);
        }
        assert_eq!(count(&root, Some("codex")), MAX_PER_VERSION);
        // Another version has its own allowance.
        assert!(save(&root, "codex", Some("0.163.0"), "screen number 0", 6000, 99).is_some());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn odd_agent_and_version_text_cannot_leave_the_folder() {
        let root = temp("path");
        let path = save(&root, "..\\evil/agent", Some("../../1.0"), "screen", 6000, 1).unwrap();
        assert!(path.starts_with(&root), "{path:?}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
