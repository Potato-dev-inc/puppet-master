//! Tests for the read-only viewer (`open_chat`) and the continue plan (`resume_plan`). A fake store stands in for an
//! agent folder, and the temp folders are made by the test itself. No test reads a real home folder.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::{open_chat, resume_plan, HistoryEnv, ListLimits, Listing, ReadError, ReadLimits, ReadOutcome, SessionHistory};
use crate::chat_index::ImportedChatRow;
use crate::redact::EMAIL_MASK;
use crate::session_chat::ChatItem;

const SESSION: &str = "3f2a9c10-7b4e-4d21-9a6c-1e5f0b7d8a01";
const OTHER: &str = "5c8e1d22-0a9b-4f3e-b7c6-2d4a8e9f0b02";
const PLANTED_KEY: &str = "sk-ant-api03-FAKEFAKEFAKE123456";

/// A store with fixed items. It honours the `max_items` and `chunk` limits the way a real store does.
struct FakeHistory {
    items: Vec<ChatItem>,
    error: Option<ReadError>,
}

impl SessionHistory for FakeHistory {
    fn list(&self, _env: &HistoryEnv, _limits: &ListLimits) -> Listing {
        Listing::default()
    }

    fn read(
        &self,
        _env: &HistoryEnv,
        _session: &str,
        limits: &ReadLimits,
        sink: &mut dyn FnMut(Vec<ChatItem>) -> bool,
    ) -> Result<ReadOutcome, ReadError> {
        if let Some(error) = self.error {
            return Err(error);
        }
        let mut sent = 0;
        for chunk in self.items.chunks(limits.chunk.max(1)) {
            let take = limits.max_items.saturating_sub(sent).min(chunk.len());
            sent += take;
            if !sink(chunk[..take].to_vec()) {
                return Ok(ReadOutcome { items: sent, cut_short: false });
            }
            if take < chunk.len() {
                return Ok(ReadOutcome { items: sent, cut_short: true });
            }
        }
        Ok(ReadOutcome { items: sent, cut_short: false })
    }

    fn resume_args(&self, _session: &str) -> Option<Vec<String>> {
        None
    }

    fn places(&self, _env: &HistoryEnv) -> Vec<String> {
        Vec::new()
    }
}

fn store(items: Vec<ChatItem>) -> FakeHistory {
    FakeHistory { items, error: None }
}

fn failing(error: ReadError) -> FakeHistory {
    FakeHistory { items: Vec::new(), error: Some(error) }
}

/// A user message with a planted key, an address and a path under the home folder.
fn secret_user_item() -> ChatItem {
    ChatItem::User {
        id: "1:0".into(),
        at: None,
        text: format!("use {PLANTED_KEY} and mail you@example.com from C:\\Users\\you\\proj\\.env"),
    }
}

fn assistant_line(n: usize) -> ChatItem {
    ChatItem::Assistant { id: format!("{n}:0"), at: None, text: format!("line {n}"), model: None, final_answer: false }
}

/// An index row for `session`, kept under the folder `cwd` (with `~` for the home folder).
fn row(session: &str, cwd: &str, folder: &str) -> ImportedChatRow {
    ImportedChatRow {
        agent: "fake".into(),
        session: session.into(),
        folder: folder.into(),
        cwd: cwd.into(),
        title: "first question".into(),
        updated_ms: 1_786_000_000_000,
        stamp: "10:1786000000000".into(),
        items: 2,
        cut_short: false,
        imported_ms: 1_786_000_100_000,
    }
}

fn env_at(home: &str) -> HistoryEnv {
    HistoryEnv::fixed(Some(PathBuf::from(home)), &[])
}

/// A folder under the system temp folder, unique per test. Like the other history tests, it is never deleted.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |time| time.as_nanos());
        let dir = std::env::temp_dir().join(format!("pm-bc18-{tag}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&dir).expect("temp folder");
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

#[test]
fn open_redacts_items_masks_emails_and_shows_home_as_tilde() {
    let history = store(vec![secret_user_item()]);
    let env = env_at("C:\\Users\\you");
    let opened = open_chat(&history, &env, &row(SESSION, "~\\proj", "proj"), SESSION).expect("opens");
    assert_eq!(opened.items.len(), 1);
    let text = opened.items[0]["text"].as_str().expect("text");
    assert!(!text.contains("FAKEFAKEFAKE"), "the planted key is masked: {text}");
    assert!(!text.contains("you@example.com"), "the address is masked: {text}");
    assert!(text.contains(EMAIL_MASK), "the address leaves its mask: {text}");
    assert!(!text.contains("C:\\Users\\you"), "the home folder is not shown: {text}");
    assert!(text.contains("~\\proj\\.env"), "the home folder is shown as ~: {text}");
    assert_eq!(opened.items[0]["kind"], "user", "the row keeps its kind");
}

#[test]
fn open_home_as_tilde_works_for_an_injected_home_that_is_not_a_users_folder() {
    let history = store(vec![ChatItem::Assistant {
        id: "1:0".into(),
        at: None,
        text: "saved at D:\\fixture-home\\proj\\a.rs".into(),
        model: None,
        final_answer: false,
    }]);
    let env = env_at("D:\\fixture-home");
    let opened = open_chat(&history, &env, &row(SESSION, "~\\proj", "proj"), SESSION).expect("opens");
    assert_eq!(opened.items[0]["text"], "saved at ~\\proj\\a.rs");
}

#[test]
fn open_redacts_the_header_too() {
    let history = store(vec![assistant_line(1)]);
    let env = env_at("C:\\Users\\you");
    let mut chat = row(SESSION, "~\\proj", "C:\\Users\\you\\proj");
    chat.title = format!("key {PLANTED_KEY}");
    chat.cut_short = true;
    let opened = open_chat(&history, &env, &chat, SESSION).expect("opens");
    assert!(!opened.meta.title.contains("FAKEFAKEFAKE"), "title: {}", opened.meta.title);
    assert_eq!(opened.meta.folder, "~\\proj", "the folder is shown with ~, not the account path");
    assert_eq!(opened.meta.agent, "fake");
    assert_eq!(opened.meta.updated_ms, 1_786_000_000_000);
    assert!(opened.meta.cut_short, "a chat cut at import stays marked");
}

#[test]
fn open_caps_at_view_max_items_and_marks_cut_short() {
    let items: Vec<ChatItem> = (0..6500).map(assistant_line).collect();
    let opened = open_chat(&store(items), &env_at("C:\\Users\\you"), &row(SESSION, "~\\proj", "proj"), SESSION)
        .expect("opens");
    assert_eq!(opened.items.len(), super::VIEW_MAX_ITEMS);
    assert_eq!(opened.items.last().map(|item| item["id"].clone()), Some("5999:0".into()));
    assert!(opened.meta.cut_short);
}

#[test]
fn open_a_chat_of_exactly_the_cap_is_not_cut() {
    let items: Vec<ChatItem> = (0..super::VIEW_MAX_ITEMS).map(assistant_line).collect();
    let opened = open_chat(&store(items), &env_at("C:\\Users\\you"), &row(SESSION, "~\\proj", "proj"), SESSION)
        .expect("opens");
    assert_eq!(opened.items.len(), super::VIEW_MAX_ITEMS);
    assert!(!opened.meta.cut_short);
}

#[test]
fn open_missing_source_gives_the_plain_sentence() {
    let err = open_chat(&failing(ReadError::Missing), &env_at("C:\\Users\\you"), &row(SESSION, "~\\proj", "proj"), SESSION)
        .expect_err("refused");
    assert_eq!(err, "The original chat file is gone. Search still has its text.");
}

#[test]
fn open_other_read_errors_give_plain_sentences_and_no_path() {
    let env = env_at("C:\\Users\\you");
    let chat = row(SESSION, "~\\proj", "proj");
    let too_big = open_chat(&failing(ReadError::TooBig), &env, &chat, SESSION).expect_err("refused");
    let unreadable = open_chat(&failing(ReadError::Unreadable), &env, &chat, SESSION).expect_err("refused");
    let format = open_chat(&failing(ReadError::Format), &env, &chat, SESSION).expect_err("refused");
    assert_eq!(too_big, "This chat file is too big to open.");
    assert_eq!(unreadable, "The chat file could not be read.");
    assert_eq!(format, "This chat is in a format Puppet Master cannot read yet.");
    for text in [&too_big, &unreadable, &format] {
        assert!(!text.contains('\\') && !text.contains('/'), "no path in: {text}");
    }
}

#[test]
fn open_refuses_a_session_that_is_not_the_index_row() {
    let history = store(vec![assistant_line(1)]);
    let err = open_chat(&history, &env_at("C:\\Users\\you"), &row(SESSION, "~\\proj", "proj"), OTHER).expect_err("refused");
    assert_eq!(err, "This chat is not in the search index.");
}

#[test]
fn resume_plan_refuses_an_agent_with_no_resume_option() {
    let home = TempDir::new("noresume");
    let plan = resume_plan(None, &row(SESSION, "~\\proj", "proj"), Some(home.path()), "fake");
    assert!(!plan.ok);
    assert_eq!(plan.reason.as_deref(), Some("This agent has no way to continue an old chat."));
    assert_eq!(plan.agent_type, "fake");
    assert!(plan.cwd.is_empty() && plan.extra_args.is_empty());
}

#[test]
fn resume_plan_refuses_an_unknown_folder() {
    let home = TempDir::new("unknown");
    let args = vec!["--resume".to_string(), SESSION.to_string()];
    let none_kept = resume_plan(Some(args), &row(SESSION, "", "proj"), Some(home.path()), "fake");
    assert_eq!(none_kept.reason.as_deref(), Some("The folder of this chat is not known."));
    let no_home = resume_plan(Some(vec![]), &row(SESSION, "~\\proj", "proj"), None, "fake");
    assert!(!no_home.ok);
    assert_eq!(no_home.reason.as_deref(), Some("The folder of this chat is not known."), "~ with no home cannot expand");
    let climbs = resume_plan(Some(vec![]), &row(SESSION, "~\\..\\other", "other"), Some(home.path()), "fake");
    assert_eq!(climbs.reason.as_deref(), Some("The folder of this chat is not known."), "a .. is refused");
}

#[test]
fn resume_plan_refuses_a_folder_that_is_gone() {
    let home = TempDir::new("gone");
    let args = vec!["--resume".to_string(), SESSION.to_string()];
    let plan = resume_plan(Some(args), &row(SESSION, "~\\gone-proj", "gone-proj"), Some(home.path()), "fake");
    assert!(!plan.ok);
    assert_eq!(plan.reason.as_deref(), Some("The folder is gone: gone-proj."));
    assert!(plan.cwd.is_empty() && plan.extra_args.is_empty(), "nothing to start with");
    let nameless = resume_plan(Some(vec![]), &row(SESSION, "~\\gone", ""), Some(home.path()), "fake");
    assert_eq!(nameless.reason.as_deref(), Some("The folder is gone."));
}

#[test]
fn resume_plan_refuses_an_invalid_session_id_first() {
    let home = TempDir::new("badid");
    let args = vec!["--resume".to_string(), "../evil".to_string()];
    let plan = resume_plan(Some(args), &row("../evil", "~\\proj", "proj"), Some(home.path()), "fake");
    assert!(!plan.ok);
    assert_eq!(plan.reason.as_deref(), Some("This chat's id is not valid."));
}

#[test]
fn resume_plan_ok_expands_the_home_folder_and_keeps_the_args() {
    let home = TempDir::new("ok");
    fs::create_dir_all(home.path().join("proj")).expect("project folder");
    let args = vec!["--resume".to_string(), SESSION.to_string()];
    let plan = resume_plan(Some(args.clone()), &row(SESSION, "~\\proj", "proj"), Some(home.path()), "fake");
    assert!(plan.ok, "{plan:?}");
    assert_eq!(plan.reason, None);
    assert_eq!(plan.agent_type, "fake");
    assert_eq!(plan.cwd, home.path().join("proj").to_string_lossy());
    assert_eq!(plan.extra_args, args);
}

#[test]
fn resume_plan_ok_takes_a_folder_that_is_already_absolute() {
    let home = TempDir::new("absolute");
    let project = home.path().join("proj");
    fs::create_dir_all(&project).expect("project folder");
    let absolute = project.to_string_lossy().into_owned();
    let plan = resume_plan(Some(vec!["--resume".into(), SESSION.into()]), &row(SESSION, &absolute, "proj"), None, "fake");
    assert!(plan.ok, "{plan:?}");
    assert_eq!(plan.cwd, absolute);
}
