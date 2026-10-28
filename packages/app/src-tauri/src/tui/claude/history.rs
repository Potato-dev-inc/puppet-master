//! Claude Code's old chats: `<config dir>/projects/<project folder>/<session id>.jsonl`, where the config dir is
//! `~/.claude` or `CLAUDE_CONFIG_DIR`. The generic `FileHistory` walks the folders and streams the lines; this file
//! only says where Claude keeps them, how a chat is named and how to resume one. Sidechain files (`agent-*.jsonl`)
//! and anything below a project folder are left out.

use crate::session_chat::history::{
    FileHistory, FileHistorySpec, HistoryEnv, ListLimits, Listing, ReadError, ReadLimits, ReadOutcome, SessionHistory,
    SessionKey,
};
use crate::session_chat::ChatItem;

use super::session_file::ClaudeSessionFile;

static FORMAT: ClaudeSessionFile = ClaudeSessionFile;

/// Where Claude Code keeps its chats: `{home}/.claude/projects`, and `<CLAUDE_CONFIG_DIR>/projects` when it is set.
/// The session id is the file name; the folder is the first top-level `cwd` in the file's first lines.
const SPEC: FileHistorySpec = FileHistorySpec {
    roots: &["{home}/.claude/projects"],
    extra_roots_env: &[("CLAUDE_CONFIG_DIR", "projects")],
    glob: "*.jsonl",
    skip: &["agent-*.jsonl"],
    depth: 1,
    id: SessionKey::FileStem,
    cwd_pointer: Some("/cwd"),
    format: &FORMAT,
    resume: &["--resume", "{session}"],
};

/// Claude Code's old-chat store: the generic file store with Claude's spec.
pub struct ClaudeHistory(FileHistory);

/// The store `Claude::session_history()` returns.
pub static HISTORY: ClaudeHistory = ClaudeHistory(FileHistory::new(SPEC));

impl SessionHistory for ClaudeHistory {
    fn list(&self, env: &HistoryEnv, limits: &ListLimits) -> Listing {
        self.0.list(env, limits)
    }

    fn read(
        &self,
        env: &HistoryEnv,
        session: &str,
        limits: &ReadLimits,
        sink: &mut dyn FnMut(Vec<ChatItem>) -> bool,
    ) -> Result<ReadOutcome, ReadError> {
        self.0.read(env, session, limits, sink)
    }

    fn resume_args(&self, session: &str) -> Option<Vec<String>> {
        self.0.resume_args(session)
    }

    fn places(&self, env: &HistoryEnv) -> Vec<String> {
        self.0.places(env)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::HISTORY;
    use crate::session_chat::history::{HistoryEnv, ListLimits, ReadError, ReadLimits, SessionHistory};
    use crate::session_chat::{ChatItem, SessionFormat};
    use crate::tui::claude::session_file::ClaudeSessionFile;

    /// The synthetic fixtures (BC-4). Tests copy them into a temp home; they never read a real agent folder.
    const PROJECTS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src/tui/claude/fixtures/history/projects");
    const FOLDER: &str = "C--Users-you-proj";
    const CHAT_A: &str = "3f2a9c10-7b4e-4d21-9a6c-1e5f0b7d8a01"; // has a `cwd` on line 3
    const CHAT_B: &str = "5c8e1d22-0a9b-4f3e-b7c6-2d4a8e9f0b02"; // no `cwd` anywhere

    /// A temp home with the fixtures under `.claude/projects`, and the env that points at it.
    fn fixture_home(tag: &str) -> (PathBuf, HistoryEnv) {
        let home = temp_home(tag);
        copy_dir(Path::new(PROJECTS), &home.join(".claude").join("projects"));
        let env = HistoryEnv::fixed(Some(home.clone()), &[]);
        (home, env)
    }

    fn temp_home(tag: &str) -> PathBuf {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |time| time.as_nanos());
        std::env::temp_dir().join(format!("pm-bc14-{tag}-{}-{nanos}", std::process::id()))
    }

    fn copy_dir(from: &Path, to: &Path) {
        fs::create_dir_all(to).expect("copy target");
        for entry in fs::read_dir(from).expect("fixture folder") {
            let entry = entry.expect("fixture entry");
            let target = to.join(entry.file_name());
            if entry.file_type().expect("fixture type").is_dir() {
                copy_dir(&entry.path(), &target);
            } else {
                fs::copy(entry.path(), &target).expect("fixture copy");
            }
        }
    }

    /// The items `ClaudeSessionFile` gives for each line of one fixture file, with the same offsets the store uses.
    fn expected_items(path: &Path) -> Vec<ChatItem> {
        let bytes = fs::read(path).expect("fixture chat");
        let mut items = Vec::new();
        let mut offset = 0u64;
        for line in bytes.split_inclusive(|byte| *byte == b'\n') {
            let text = String::from_utf8_lossy(line);
            items.extend(ClaudeSessionFile.parse_line(text.trim_end_matches(['\n', '\r']), offset));
            offset += line.len() as u64;
        }
        items
    }

    fn fixture_chat(session: &str) -> PathBuf {
        Path::new(PROJECTS).join(FOLDER).join(format!("{session}.jsonl"))
    }

    #[test]
    fn lists_two_chats_and_skips_sidechains_and_subagent_files() {
        let (_home, env) = fixture_home("list");
        let listing = HISTORY.list(&env, &ListLimits::default());
        let mut ids: Vec<&str> = listing.chats.iter().map(|chat| chat.session.as_str()).collect();
        ids.sort();
        assert_eq!(ids, vec![CHAT_A, CHAT_B]);
        assert_eq!((listing.too_big, listing.unreadable, listing.cut_short), (0, 0, false));
    }

    #[test]
    fn the_folder_is_read_for_one_chat_and_absent_for_the_other() {
        let (_home, env) = fixture_home("cwd");
        let listing = HISTORY.list(&env, &ListLimits::default());
        let a = listing.chats.iter().find(|chat| chat.session == CHAT_A).expect("chat a");
        assert_eq!(a.cwd.as_deref(), Some("~\\proj"), "home is shown as ~, the rest as written");
        let b = listing.chats.iter().find(|chat| chat.session == CHAT_B).expect("chat b");
        assert_eq!(b.cwd, None);
    }

    #[test]
    fn items_are_the_ones_the_claude_session_format_gives() {
        let (_home, env) = fixture_home("items");
        let mut got = Vec::new();
        let outcome = HISTORY.read(&env, CHAT_B, &ReadLimits::default(), &mut |batch| {
            got.extend(batch);
            true
        });
        assert!(outcome.is_ok(), "{outcome:?}");
        assert!(!got.is_empty());
        assert_eq!(got, expected_items(&fixture_chat(CHAT_B)));
        let missing = HISTORY.read(&env, "00000000-0000-0000-0000-000000000000", &ReadLimits::default(), &mut |_| true);
        assert_eq!(missing, Err(ReadError::Missing));
    }

    #[test]
    fn config_dir_variable_adds_a_root_and_is_listed_too() {
        let (home, _env) = fixture_home("config-dir");
        let alt = home.join("alt").join("projects").join("proj");
        fs::create_dir_all(&alt).expect("alt folder");
        fs::write(alt.join("9d1f0c33-1111-4222-8333-944455556666.jsonl"), "{\"type\":\"user\"}\n").expect("alt chat");
        let alt_dir = home.join("alt");
        let env = HistoryEnv::fixed(Some(home.clone()), &[("CLAUDE_CONFIG_DIR", alt_dir.to_str().expect("utf-8 temp path"))]);
        let listing = HISTORY.list(&env, &ListLimits::default());
        assert!(listing.chats.iter().any(|chat| chat.session == "9d1f0c33-1111-4222-8333-944455556666"));
        assert_eq!(listing.chats.len(), 3, "the default root's two chats and the variable root's one");
    }

    #[test]
    fn resume_args_are_the_documented_flag_and_the_id() {
        assert_eq!(HISTORY.resume_args(CHAT_A), Some(vec!["--resume".to_string(), CHAT_A.to_string()]));
        assert_eq!(HISTORY.resume_args("../etc/passwd"), None);
        assert_eq!(HISTORY.resume_args("abc; del /q"), None);
    }

    #[test]
    fn places_show_home_as_tilde_with_one_separator_on_windows_and_unix() {
        // Windows-style home: the default root, and a variable root written with forward slashes (mixed on Windows).
        let windows = HistoryEnv::fixed(Some(PathBuf::from(r"C:\Users\you")), &[]);
        assert_eq!(HISTORY.places(&windows), vec!["~/.claude/projects".to_string()]);
        let mixed = HistoryEnv::fixed(Some(PathBuf::from(r"C:\Users\you")), &[("CLAUDE_CONFIG_DIR", r"C:\Users\you\cfg")]);
        assert_eq!(HISTORY.places(&mixed), vec!["~/.claude/projects".to_string(), "~/cfg/projects".to_string()]);
        let slashes = HistoryEnv::fixed(Some(PathBuf::from(r"C:\Users\you")), &[("CLAUDE_CONFIG_DIR", "D:/alt")]);
        assert_eq!(HISTORY.places(&slashes), vec!["~/.claude/projects".to_string(), "D:/alt/projects".to_string()]);

        // Unix-style home and a Unix variable root.
        let unix = HistoryEnv::fixed(Some(PathBuf::from("/home/you")), &[]);
        assert_eq!(HISTORY.places(&unix), vec!["~/.claude/projects".to_string()]);
        let unix_var = HistoryEnv::fixed(Some(PathBuf::from("/home/you")), &[("CLAUDE_CONFIG_DIR", "/srv/claude")]);
        assert_eq!(HISTORY.places(&unix_var), vec!["~/.claude/projects".to_string(), "/srv/claude/projects".to_string()]);
        assert!(HISTORY.places(&HistoryEnv::isolated()).is_empty(), "no home, nothing to show");
    }
}
