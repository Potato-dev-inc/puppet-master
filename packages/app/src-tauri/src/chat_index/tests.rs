use std::sync::Arc;
use std::time::{Duration, Instant};

use super::entry::{entries_of, entry_of, folder_name, Entry, MAX_TEXT_CHARS};
use super::query::{marks_of, match_expr};
use super::store::Store;
use super::*;
use crate::session_chat::{ChatItem, ItemObserver, ToolDetail};

fn user(id: &str, at: Option<&str>, text: &str) -> ChatItem {
    ChatItem::User { id: id.into(), at: at.map(String::from), text: text.into() }
}

fn assistant(id: &str, at: Option<&str>, text: &str) -> ChatItem {
    ChatItem::Assistant { id: id.into(), at: at.map(String::from), text: text.into(), model: None, final_answer: true }
}

fn tool(id: &str, name: &str, summary: &str) -> ChatItem {
    ChatItem::ToolCall { id: id.into(), at: None, call_id: "c".into(), name: name.into(), summary: summary.into(), detail: ToolDetail::default() }
}

fn ctx() -> PaneContext {
    PaneContext::of("agent-x", "C:\\Users\\you\\projects\\shop", Some("sess-1"))
}

fn entry(item: ChatItem) -> Entry {
    entry_of("pane-1", &ctx(), &item, 1_000).expect("kept")
}

struct Fixed(PaneContext);

impl ContextLookup for Fixed {
    fn context(&self, _pane_id: &str) -> Option<PaneContext> {
        Some(self.0.clone())
    }
}

fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(5);
    while !done() {
        assert!(Instant::now() < end, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn row(id: &str, text: &str, at_ms: i64) -> Entry {
    Entry {
        pane_id: "pane-1".into(),
        session: "s".into(),
        item_id: id.into(),
        kind: "assistant",
        agent: "agent-x".into(),
        folder: "shop".into(),
        at_ms,
        text: text.into(),
    }
}

#[test]
fn the_bundled_sqlite_has_fts5() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE VIRTUAL TABLE t USING fts5(x); INSERT INTO t(x) VALUES ('hello world');").unwrap();
    let hits: i64 = conn.query_row("SELECT COUNT(*) FROM t WHERE t MATCH 'hel*'", [], |r| r.get(0)).unwrap();
    assert_eq!(hits, 1);
}

#[test]
fn a_folder_is_kept_as_its_name_not_its_path() {
    assert_eq!(folder_name("C:\\Users\\you\\projects\\shop"), "shop");
    assert_eq!(folder_name("/home/you/projects/shop/"), "shop");
    assert_eq!(folder_name(""), "");
    let e = entry(user("1:0", None, "hi"));
    assert_eq!(e.folder, "shop");
    assert!(!format!("{e:?}").contains("Users"));
}

#[test]
fn user_assistant_and_tool_titles_are_kept_and_nothing_else() {
    let result = ChatItem::ToolResult { id: "4:0".into(), at: None, call_id: "c".into(), ok: true, output: "RAW-OUTPUT-WORD".into() };
    let note = ChatItem::Notice { id: "5:0".into(), at: None, notice_kind: "pr".into(), text: "Pull request".into(), url: None };
    let items = vec![user("1:0", None, "question"), assistant("2:0", None, "answer"), tool("3:0", "Bash", "git status"), result, note];
    let entries = entries_of("p", &ctx(), &items, 5);
    let kinds: Vec<&str> = entries.iter().map(|e| e.kind).collect();
    assert_eq!(kinds, vec!["user", "assistant", "tool"]);
    assert_eq!(entries[2].text, "Bash git status");
    assert!(entries.iter().all(|e| !e.text.contains("RAW-OUTPUT-WORD")));
    // An empty message has nothing to find.
    assert!(entry_of("p", &ctx(), &user("9:0", None, "  \n "), 5).is_none());
}

#[test]
fn the_time_is_the_items_own_or_the_time_it_was_read() {
    assert_eq!(entry(user("1:0", Some("2026-10-27T10:00:00Z"), "a")).at_ms, 1_793_095_200_000);
    assert_eq!(entry(user("1:0", Some("not a time"), "a")).at_ms, 1_000);
    assert_eq!(entry(user("1:0", None, "a")).at_ms, 1_000);
}

#[test]
fn a_token_is_masked_before_it_is_stored() {
    let token = "sk-ant-api03-FakeFakeFakeFake0123456789abcdef";
    let e = entry(user("1:0", None, &format!("my key is {token} please keep it")));
    assert!(!e.text.contains("FakeFake"), "{}", e.text);
    let titled = entry(tool("2:0", "Bash", &format!("curl -H 'x-api-key: {token}'")));
    assert!(!titled.text.contains("FakeFake"), "{}", titled.text);
}

#[test]
fn a_long_text_is_cut() {
    let e = entry(assistant("1:0", None, &"word ".repeat(2000)));
    assert_eq!(e.text.chars().count(), MAX_TEXT_CHARS + 1);
    assert!(e.text.ends_with('…'));
}

#[test]
fn typed_text_becomes_a_safe_prefix_query() {
    assert_eq!(match_expr("  deploy   script "), Some("\"deploy\"* \"script\"*".to_string()));
    assert_eq!(match_expr("say \"hi\" OR NOT *"), Some("\"say\"* \"hi\"* \"OR\"* \"NOT\"*".to_string()));
    assert_eq!(match_expr("   "), None);
    assert_eq!(match_expr("--- ***"), None);
    assert_eq!(match_expr(&"a ".repeat(40)).unwrap().matches('*').count(), 12);
}

#[test]
fn marks_are_counted_in_utf16_units_and_the_snippet_is_one_line() {
    let (text, marks) = marks_of("😀 the \u{1}fix\u{2} is\nin \u{1}fix\u{2}es");
    assert_eq!(text, "😀 the fix is in fixes");
    // The emoji is two UTF-16 units.
    assert_eq!(marks, vec![[7, 10], [17, 20]]);
    let units: Vec<u16> = text.encode_utf16().collect();
    assert_eq!(String::from_utf16(&units[7..10]).unwrap(), "fix");
}

#[test]
fn search_finds_words_as_prefixes_newest_first_with_marked_snippets() {
    let mut store = Store::open_memory(100).unwrap();
    store
        .insert(&[
            row("1:0", "We moved the deployment script to scripts/deploy.sh", 10),
            row("2:0", "Unrelated talk about colours", 20),
            row("3:0", "The Deploy step needs a new token", 30),
        ])
        .unwrap();
    let hits = store.search("deploy", 10).unwrap();
    let ids: Vec<&str> = hits.iter().map(|h| h.item_id.as_str()).collect();
    assert_eq!(ids, vec!["3:0", "1:0"]);
    let top = &hits[0];
    assert_eq!((top.agent.as_str(), top.folder.as_str(), top.pane_id.as_str()), ("agent-x", "shop", "pane-1"));
    let [from, to] = top.marks[0];
    assert_eq!(&top.snippet[from as usize..to as usize], "Deploy");
    // Every typed word must be there.
    assert_eq!(store.search("deploy token", 10).unwrap().len(), 1);
    assert!(store.search("deploy zebra", 10).unwrap().is_empty());
    assert!(store.search("", 10).unwrap().is_empty());
    // Operators typed by the person are plain words, never a syntax error.
    assert!(store.search("\"deploy AND (", 10).is_ok());
    assert_eq!(store.search("deploy", 1).unwrap().len(), 1);
}

#[test]
fn an_item_read_again_is_replaced_not_doubled_and_keeps_its_time() {
    let mut store = Store::open_memory(100).unwrap();
    store.insert(&[row("1:0", "first draft of the plan", 10)]).unwrap();
    store.insert(&[row("1:0", "final version of the plan", 99)]).unwrap();
    assert_eq!(store.count().unwrap(), 1);
    assert!(store.search("draft", 5).unwrap().is_empty());
    let hit = store.search("final", 5).unwrap().remove(0);
    assert_eq!(hit.at_ms, 10);
}

#[test]
fn the_oldest_items_go_past_the_cap() {
    let mut store = Store::open_memory(3).unwrap();
    for n in 0..5 {
        store.insert(&[row(&format!("{n}:0"), &format!("note number{n}"), n)]).unwrap();
    }
    assert_eq!(store.count().unwrap(), 3);
    assert!(store.search("number0", 5).unwrap().is_empty());
    assert!(store.search("number1", 5).unwrap().is_empty());
    assert_eq!(store.search("number4", 5).unwrap().len(), 1);
    // The text index forgot the dropped items too.
    assert_eq!(store.search("note", 10).unwrap().len(), 3);
}

#[test]
fn items_handed_to_the_index_are_redacted_labelled_and_found() {
    let index = ChatIndex::start(Store::open_memory(1000).unwrap(), Arc::new(Fixed(ctx())));
    let token = "sk-ant-api03-FakeFakeFakeFake0123456789abcdef";
    let result = ChatItem::ToolResult { id: "3:0".into(), at: None, call_id: "c".into(), ok: true, output: "OUTPUTONLYWORD".into() };
    index.observe(
        "pane-7",
        &[
            user("1:0", Some("2026-10-27T10:00:00Z"), &format!("remember the license {token}")),
            assistant("2:0", None, "The license check lives in checks.rs"),
            result,
        ],
    );
    wait_for("the writer", || index.count() == 2);
    let hits = index.search("license", 10).unwrap();
    assert_eq!(hits.len(), 2);
    assert!(hits.iter().all(|h| h.pane_id == "pane-7" && h.folder == "shop" && h.agent == "agent-x" && h.session == "sess-1"));
    assert!(hits.iter().all(|h| !h.snippet.contains("FakeFake")), "{hits:?}");
    assert!(index.search("FakeFake", 10).unwrap().is_empty());
    assert!(index.search("OUTPUTONLYWORD", 10).unwrap().is_empty());
}

#[test]
fn a_batch_after_its_pane_closed_keeps_the_labels_seen_before() {
    struct Closing(std::sync::atomic::AtomicBool);
    impl ContextLookup for Closing {
        fn context(&self, _pane: &str) -> Option<PaneContext> {
            (!self.0.swap(true, std::sync::atomic::Ordering::SeqCst)).then(ctx)
        }
    }
    let index = ChatIndex::start(Store::open_memory(100).unwrap(), Arc::new(Closing(Default::default())));
    index.observe("p", &[user("1:0", None, "first words")]);
    wait_for("the first", || index.count() == 1);
    index.observe("p", &[user("2:0", None, "later words")]);
    wait_for("the second", || index.count() == 2);
    assert!(index.search("words", 10).unwrap().iter().all(|h| h.folder == "shop"));
}

#[test]
fn a_damaged_file_is_moved_aside_and_a_new_index_is_made() {
    let dir = std::env::temp_dir().join(format!("pm-chat-index-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(FILE_NAME);
    std::fs::write(&path, b"this is not a database file at all, just text that is long enough to be read as a header, so sqlite refuses it").unwrap();
    let mut store = open_store(&path, 10).expect("a new index");
    store.insert(&[row("1:0", "hello there", 1)]).unwrap();
    assert_eq!(store.search("hello", 5).unwrap().len(), 1);
    let aside = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).any(|e| e.file_name().to_string_lossy().contains(".bad-"));
    assert!(aside, "the damaged file stays next to the new one");
}
