//! Codex's old chats: `<codex home>/sessions/YYYY/MM/DD/rollout-<local time>-<thread id>.jsonl`, where the codex home is
//! `~/.codex` or `CODEX_HOME`. A finished session is kept compressed (`.jsonl.zst`); when a chat has both files, the
//! plain one is read. The generic `FileHistory` walks the day folders and streams the lines; this file only says where
//! Codex keeps them, that the id and the folder are on the first line (`session_meta`) and how to resume one.

use std::path::Path;

use crate::session_chat::history::{
    FileHistory, FileHistorySpec, FileMeta, HistoryEnv, ListLimits, Listing, ReadError, ReadLimits, ReadOutcome,
    SessionHistory, SessionKey,
};
use crate::session_chat::ChatItem;

use super::session_file::{rollout_meta, CodexRollout};

static FORMAT: CodexRollout = CodexRollout;

/// Where Codex keeps its chats: `{home}/.codex/sessions`, and `<CODEX_HOME>/sessions` when it is set. The file name
/// is `rollout-*`; the extension check keeps `.jsonl` and `.jsonl.zst`. Days are three folders deep (`YYYY/MM/DD`).
const SPEC: FileHistorySpec = FileHistorySpec {
    roots: &["{home}/.codex/sessions"],
    extra_roots_env: &[("CODEX_HOME", "sessions")],
    glob: "rollout-*",
    skip: &[],
    depth: 3,
    id: SessionKey::Meta(meta_of),
    cwd_pointer: None,
    format: &FORMAT,
    resume: &["resume", "{session}"],
};

/// The chat id and the folder, both from the `session_meta` line. The file name is not used.
fn meta_of(path: &Path) -> Option<FileMeta> {
    let (cwd, id) = rollout_meta(path)?;
    Some(FileMeta { id, cwd: Some(cwd) })
}

/// Codex's old-chat store: the generic file store with Codex's spec.
pub struct CodexHistory(FileHistory);

/// The store `Codex::session_history()` returns.
pub static HISTORY: CodexHistory = CodexHistory(FileHistory::new(SPEC));

impl SessionHistory for CodexHistory {
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
    use crate::tui::codex::session_file::CodexRollout;

    /// The synthetic rollouts (BC-4 and the zst pair of BC-15). Tests copy them into a temp home; they never read a
    /// real agent folder. `shop` = a1 (plain), `notes` = b2 (plain and zst: the plain one wins), `archive` = c3 (zst only).
    const SESSIONS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src/tui/codex/fixtures/history/sessions");
    const SHOP: &str = "0198b000-0000-7000-8000-0000000000a1";
    const NOTES: &str = "0198b100-0000-7000-8000-0000000000b2";
    const ARCHIVE: &str = "0198b300-0000-7000-8000-0000000000c3";
    const SHOP_FILE: &str = "2026/08/14/rollout-2026-08-14T10-00-00-0198b000-0000-7000-8000-0000000000a1.jsonl";
    const NOTES_PLAIN: &str = "2026/08/15/rollout-2026-08-15T09-30-00-0198b100-0000-7000-8000-0000000000b2.jsonl";
    const NOTES_ZST: &str = "2026/08/15/rollout-2026-08-15T09-30-00-0198b100-0000-7000-8000-0000000000b2.jsonl.zst";

    /// A temp home with the fixtures under `.codex/sessions`, and the env that points at it.
    fn fixture_home(tag: &str) -> (PathBuf, HistoryEnv) {
        let home = temp_home(tag);
        copy_dir(Path::new(SESSIONS), &home.join(".codex").join("sessions"));
        let env = HistoryEnv::fixed(Some(home.clone()), &[]);
        (home, env)
    }

    fn temp_home(tag: &str) -> PathBuf {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |time| time.as_nanos());
        std::env::temp_dir().join(format!("pm-bc15-{tag}-{}-{nanos}", std::process::id()))
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

    fn fixture_file(relative: &str) -> PathBuf {
        Path::new(SESSIONS).join(relative)
    }

    /// The items `CodexRollout` gives for each line of one plain fixture file, with the offsets the store uses.
    fn expected_items(path: &Path) -> Vec<ChatItem> {
        let bytes = fs::read(path).expect("fixture chat");
        let mut items = Vec::new();
        let mut offset = 0u64;
        for line in bytes.split_inclusive(|byte| *byte == b'\n') {
            let text = String::from_utf8_lossy(line);
            items.extend(CodexRollout.parse_line(text.trim_end_matches(['\n', '\r']), offset));
            offset += line.len() as u64;
        }
        items
    }

    fn read_all(env: &HistoryEnv, session: &str) -> Result<Vec<ChatItem>, ReadError> {
        let mut got = Vec::new();
        HISTORY.read(env, session, &ReadLimits::default(), &mut |batch| {
            got.extend(batch);
            true
        })?;
        Ok(got)
    }

    #[test]
    fn lists_the_three_chats_once_each_with_the_plain_file_preferred() {
        let (_home, env) = fixture_home("list");
        let listing = HISTORY.list(&env, &ListLimits::default());
        let mut ids: Vec<&str> = listing.chats.iter().map(|chat| chat.session.as_str()).collect();
        ids.sort();
        assert_eq!(ids, vec![SHOP, NOTES, ARCHIVE], "the plain and zst copies of notes count as one chat");
        assert_eq!((listing.too_big, listing.unreadable, listing.cut_short), (0, 0, false));
    }

    #[test]
    fn the_id_and_the_folder_come_from_the_session_meta_line() {
        let (_home, env) = fixture_home("meta");
        let listing = HISTORY.list(&env, &ListLimits::default());
        let shop = listing.chats.iter().find(|chat| chat.session == SHOP).expect("shop chat");
        assert_eq!(shop.cwd.as_deref(), Some("~\\shop"), "home is shown as ~, the rest as written");
        let archive = listing.chats.iter().find(|chat| chat.session == ARCHIVE).expect("zst-only chat");
        assert_eq!(archive.cwd.as_deref(), Some("~\\archive"), "a compressed rollout gives its folder too");
    }

    #[test]
    fn a_plain_rollout_is_read_in_place_of_its_zst_copy() {
        let (home, env) = fixture_home("plain-wins");
        let listing = HISTORY.list(&env, &ListLimits::default());
        let notes = listing.chats.iter().find(|chat| chat.session == NOTES).expect("notes chat");
        let plain_len = fs::metadata(fixture_file(NOTES_PLAIN)).expect("plain").len();
        let zst_len = fs::metadata(fixture_file(NOTES_ZST)).expect("zst").len();
        assert_ne!(plain_len, zst_len, "the two copies differ in size, so the size shows which one was taken");
        assert_eq!(notes.bytes, plain_len);
        assert!(home.join(".codex").join("sessions").join(NOTES_ZST).is_file(), "the zst copy is still there");
        assert_eq!(read_all(&env, NOTES).expect("read notes"), expected_items(&fixture_file(NOTES_PLAIN)));
    }

    #[test]
    fn items_are_the_ones_the_codex_rollout_format_gives_for_plain_and_zst() {
        let (_home, env) = fixture_home("items");
        let shop = read_all(&env, SHOP).expect("read shop");
        assert!(!shop.is_empty());
        assert_eq!(shop, expected_items(&fixture_file(SHOP_FILE)));
        let archive = read_all(&env, ARCHIVE).expect("read zst-only chat");
        assert!(archive.iter().any(|item| matches!(item, ChatItem::User { text, .. } if text.contains("archive-2026"))));
        assert_eq!(read_all(&env, "00000000-0000-0000-0000-000000000000"), Err(ReadError::Missing));
    }

    #[test]
    fn resume_args_are_the_codex_resume_subcommand_and_the_id() {
        assert_eq!(HISTORY.resume_args(SHOP), Some(vec!["resume".to_string(), SHOP.to_string()]));
        assert_eq!(HISTORY.resume_args("../etc/passwd"), None);
        assert_eq!(HISTORY.resume_args("abc; del /q"), None);
    }

    #[test]
    fn config_home_variable_adds_a_root_and_is_listed_too() {
        let (home, _env) = fixture_home("codex-home");
        let alt = home.join("alt");
        let day = alt.join("sessions").join("2026").join("10").join("27");
        fs::create_dir_all(&day).expect("alt day folder");
        // A copy of the shop chat under a new id: the id is in the file (session_meta) and in its name.
        let id = "0198c000-0000-7000-8000-0000000000d4";
        let text = fs::read_to_string(fixture_file(SHOP_FILE)).expect("shop chat").replace(SHOP, id);
        fs::write(day.join(format!("rollout-2026-10-27T09-00-00-{id}.jsonl")), text).expect("alt chat");
        let env = HistoryEnv::fixed(Some(home.clone()), &[("CODEX_HOME", alt.to_str().expect("utf-8 temp path"))]);
        let listing = HISTORY.list(&env, &ListLimits::default());
        assert!(listing.chats.iter().any(|chat| chat.session == id));
        assert_eq!(listing.chats.len(), 4, "the default root's three chats and the variable root's one");
    }

    #[test]
    fn places_show_home_as_tilde_with_one_separator_on_windows_and_unix() {
        // Windows-style home: the default root, and a variable root written with backslashes (mixed on Windows).
        let windows = HistoryEnv::fixed(Some(PathBuf::from(r"C:\Users\you")), &[]);
        assert_eq!(HISTORY.places(&windows), vec!["~/.codex/sessions".to_string()]);
        let mixed = HistoryEnv::fixed(Some(PathBuf::from(r"C:\Users\you")), &[("CODEX_HOME", r"C:\Users\you\cfg")]);
        assert_eq!(HISTORY.places(&mixed), vec!["~/.codex/sessions".to_string(), "~/cfg/sessions".to_string()]);
        let slashes = HistoryEnv::fixed(Some(PathBuf::from(r"C:\Users\you")), &[("CODEX_HOME", "D:/alt")]);
        assert_eq!(HISTORY.places(&slashes), vec!["~/.codex/sessions".to_string(), "D:/alt/sessions".to_string()]);

        // Unix-style home and a Unix variable root.
        let unix = HistoryEnv::fixed(Some(PathBuf::from("/home/you")), &[]);
        assert_eq!(HISTORY.places(&unix), vec!["~/.codex/sessions".to_string()]);
        let unix_var = HistoryEnv::fixed(Some(PathBuf::from("/home/you")), &[("CODEX_HOME", "/srv/codex")]);
        assert_eq!(HISTORY.places(&unix_var), vec!["~/.codex/sessions".to_string(), "/srv/codex/sessions".to_string()]);
        assert!(HISTORY.places(&HistoryEnv::isolated()).is_empty(), "no home, nothing to show");
    }

    #[test]
    fn the_profile_offers_this_store() {
        let profile = crate::tui::profile_for("codex");
        assert!(profile.session_history().is_some(), "codex answers session_history()");
    }
}
