use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::entry::{entries_of, entry_of, folder_name, Entry, MAX_TEXT_CHARS};
use super::feeder::Labels;
use super::query::{marks_of, match_expr};
use super::store::Store;
use super::*;
use crate::session_chat::{ChatItem, ItemObserver, ToolDetail};
use serde_json::json;

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
        imported: false,
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
    let hits = index.search_hits("license", 10).unwrap();
    assert_eq!(hits.len(), 2);
    assert!(hits.iter().all(|h| h.pane_id == "pane-7" && h.folder == "shop" && h.agent == "agent-x" && h.session == "sess-1"));
    assert!(hits.iter().all(|h| !h.snippet.contains("FakeFake")), "{hits:?}");
    assert!(index.search_hits("FakeFake", 10).unwrap().is_empty());
    assert!(index.search_hits("OUTPUTONLYWORD", 10).unwrap().is_empty());
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
    assert!(index.search_hits("words", 10).unwrap().iter().all(|h| h.folder == "shop"));
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

#[test]
fn off_nothing_is_copied_or_written() {
    let on = Arc::new(AtomicBool::new(false));
    let index = ChatIndex::start_switched(Store::open_memory(1000).unwrap(), Arc::new(Fixed(ctx())), on.clone());
    index.observe("pane-1", &[user("1:0", None, "the first secret plan")]);
    on.store(true, Ordering::SeqCst);
    index.observe("pane-1", &[user("2:0", None, "the second plan")]);
    // The writer handles batches in the order they were sent: the first one, if it had been sent, is written too.
    wait_for("the writer", || index.count() == 1);
    assert!(index.search_hits("secret", 10).unwrap().is_empty());
    assert_eq!(index.search_hits("second", 10).unwrap().len(), 1);
}

#[test]
fn off_search_says_it_is_off_with_no_hits_and_on_again_finds_them() {
    let on = Arc::new(AtomicBool::new(true));
    let index = ChatIndex::start_switched(Store::open_memory(1000).unwrap(), Arc::new(Fixed(ctx())), on.clone());
    index.observe("pane-1", &[assistant("2:0", None, "The license check lives in checks.rs")]);
    wait_for("the writer", || index.count() == 1);
    assert!(index.search("license", 10).unwrap().enabled);
    on.store(false, Ordering::SeqCst);
    assert_eq!(index.search("license", 10).unwrap(), Answer { enabled: false, hits: Vec::new() });
    assert!(!index.status().unwrap().enabled);
    // Turning it off keeps what is there.
    assert_eq!(index.status().unwrap().items, 1);
    on.store(true, Ordering::SeqCst);
    assert_eq!(index.search("license", 10).unwrap().hits.len(), 1);
}

#[test]
fn clear_empties_the_index_and_the_file_stays_valid() {
    let dir = std::env::temp_dir().join(format!("pm-chat-index-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(FILE_NAME);
    let index = ChatIndex::start(open_store(&path, 1000).unwrap(), Arc::new(Fixed(ctx())));
    index.observe(
        "pane-1",
        &[assistant("2:0", None, "marker ZEBRAMARKER7731 and more words"), user("3:0", None, "a second line")],
    );
    wait_for("the writer", || index.count() == 2);
    let before = index.status().unwrap();
    assert_eq!((before.items, before.enabled), (2, true));
    assert!(before.bytes > 0);
    // The text is in the write-ahead log until a clear (so the check below can see it going).
    let needle: &[u8] = b"ZEBRAMARKER7731";
    let wal_before = std::fs::read(dir.join(format!("{FILE_NAME}-wal"))).unwrap_or_default();
    assert!(wal_before.windows(needle.len()).any(|w| w == needle));

    index.clear().unwrap();
    let after = index.status().unwrap();
    assert_eq!(after.items, 0);
    assert!(after.enabled, "clearing does not turn the index off");
    assert!(index.search_hits("ZEBRAMARKER7731", 10).unwrap().is_empty());
    // The saved text is gone from the file and from its write-ahead log.
    for file in [path.clone(), dir.join(format!("{FILE_NAME}-wal"))] {
        let bytes = std::fs::read(&file).unwrap_or_default();
        assert!(!bytes.windows(needle.len()).any(|w| w == needle), "{file:?} still has the text");
    }
    // Still a valid index: it passes its own check and takes new items.
    let check = rusqlite::Connection::open(&path).unwrap();
    let verdict: String = check.query_row("PRAGMA integrity_check", [], |row| row.get(0)).unwrap();
    assert_eq!(verdict, "ok");
    drop(check);
    index.observe("pane-1", &[user("4:0", None, "after the clear")]);
    wait_for("a new item", || index.count() == 1);
    assert_eq!(index.search_hits("clear", 10).unwrap().len(), 1);
}

#[test]
fn the_saved_setting_turns_the_switch_off_and_on() {
    apply_setting(&json!({ "chat_index_enabled": false }));
    assert!(!enabled());
    // A settings object without the key changes nothing.
    apply_setting(&json!({ "checkpoints_enabled": true }));
    assert!(!enabled());
    apply_setting(&json!({ "chat_index_enabled": true }));
    assert!(enabled());
}

fn row_in(pane: &str, id: &str, kind: &'static str, text: &str, at_ms: i64) -> Entry {
    Entry { pane_id: pane.into(), kind, ..row(id, text, at_ms) }
}

#[test]
fn a_scoped_search_stays_inside_the_given_panes_and_skips_tool_titles() {
    let mut store = Store::open_memory(100).unwrap();
    store
        .insert(&[
            row_in("companion-mochi", "1:0", "user", "I prefer tea over coffee", 10),
            row_in("companion-mochi", "2:0", "assistant", "Noted: tea in the morning", 20),
            row_in("companion-mochi", "3:0", "tool", "Bash echo tea", 30),
            row_in("companion-bit", "1:0", "user", "My tea is green", 40),
            row_in("pane-9", "1:0", "user", "Tea party at the office", 50),
        ])
        .unwrap();
    let mine = vec!["companion-mochi".to_string()];
    let hits = store.search_in("tea", 10, &mine).unwrap();
    let got: Vec<(&str, &str)> = hits.iter().map(|h| (h.pane_id.as_str(), h.kind.as_str())).collect();
    assert_eq!(got, vec![("companion-mochi", "assistant"), ("companion-mochi", "user")]);
    // Two panes (a current and a past one) are both searched; the rest still is not.
    let two = vec!["companion-mochi".to_string(), "companion-bit".to_string()];
    assert_eq!(store.search_in("tea", 10, &two).unwrap().len(), 3);
    // No pane means no hit, never "everywhere". A pane id is a value, never SQL.
    assert!(store.search_in("tea", 10, &[]).unwrap().is_empty());
    assert!(store.search_in("tea", 10, &["x' OR '1'='1".to_string()]).unwrap().is_empty());
    // The unscoped search is unchanged.
    assert_eq!(store.search("tea", 10).unwrap().len(), 5);
}

#[test]
fn a_scoped_search_says_nothing_when_the_index_is_off() {
    let off = Arc::new(AtomicBool::new(false));
    let index = ChatIndex::start_switched(Store::open_memory(100).unwrap(), Arc::new(Fixed(ctx())), off.clone());
    let panes = vec!["pane-1".to_string()];
    let answer = index.search_in("anything", 5, &panes).unwrap();
    assert!(!answer.enabled && answer.hits.is_empty());
    off.store(true, Ordering::SeqCst);
    assert!(index.search_in("anything", 5, &panes).unwrap().enabled);
}

fn temp_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("pm-chat-index-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn names(dir: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().to_string()).collect();
    names.sort();
    names
}

#[test]
fn the_account_name_in_a_message_a_tool_title_and_the_folder_becomes_a_tilde() {
    let e = entry(user("1:0", None, r"open C:\Users\you two\notes\todo.txt and /home/you/x.rs"));
    assert!(!e.text.contains("you"), "{}", e.text);
    assert!(e.text.contains("~"));
    let t = entry(tool("2:0", "Read", r"C:\Users\you\proj\a.rs"));
    assert!(!t.text.contains("you"), "{}", t.text);
    // A pane that runs in the home folder shows as `~`, not as the account name.
    let home = PaneContext::of("agent-x", r"C:\Users\you", None);
    assert_eq!(home.folder, "~");
    assert_eq!(entry_of("p", &home, &user("3:0", None, "hi"), 1).unwrap().folder, "~");
    // A folder inside it keeps its own name.
    assert_eq!(PaneContext::of("a", r"C:\Users\you\projects\shop", None).folder, "shop");
    assert_eq!(PaneContext::of("a", "/home/you", None).folder, "~");
}

#[test]
fn moving_a_file_aside_takes_its_wal_and_shm_along() {
    let dir = temp_dir();
    let path = dir.join(FILE_NAME);
    std::fs::write(&path, b"old main").unwrap();
    std::fs::write(dir.join(format!("{FILE_NAME}-wal")), b"old log with old chat text").unwrap();
    std::fs::write(dir.join(format!("{FILE_NAME}-shm")), b"old shared memory").unwrap();
    let aside = super::aside::move_aside(&path, 42).unwrap();
    assert_eq!(names(&dir), vec!["chat-index.bad-42", "chat-index.bad-42-shm", "chat-index.bad-42-wal"]);
    assert_eq!(std::fs::read(&aside).unwrap(), b"old main");
    assert_eq!(super::aside::count(&path), 1, "a copy counts once, not once per file");
    assert_eq!(super::aside::remove_all(&path), 3);
    assert!(names(&dir).is_empty());
}

#[test]
fn a_damaged_file_is_replaced_by_a_working_index_and_counted_as_an_old_copy() {
    let dir = temp_dir();
    let path = dir.join(FILE_NAME);
    std::fs::write(&path, b"this is not a database file at all, just text that is long enough to be read as a header, so sqlite refuses it").unwrap();
    std::fs::write(dir.join(format!("{FILE_NAME}-wal")), b"stale log").unwrap();
    let mut store = open_store(&path, 10).expect("a new index");
    store.insert(&[row("1:0", "fresh words", 1)]).unwrap();
    assert_eq!(store.search("fresh", 5).unwrap().len(), 1);
    assert_eq!(store.integrity().unwrap(), "ok");
    assert_eq!(store.old_copies(), 1);
}

#[test]
fn clear_removes_the_old_copies_and_the_status_counts_them() {
    let dir = temp_dir();
    let path = dir.join(FILE_NAME);
    for stamp in [1, 2] {
        std::fs::write(dir.join(format!("chat-index.bad-{stamp}")), b"old chat text").unwrap();
        std::fs::write(dir.join(format!("chat-index.bad-{stamp}-wal")), b"old chat text").unwrap();
    }
    std::fs::write(dir.join("other-file.bad-1"), b"not ours").unwrap();
    let index = ChatIndex::start(open_store(&path, 100).unwrap(), Arc::new(Fixed(ctx())));
    assert_eq!(index.status().unwrap().old_copies, 2);
    index.clear().unwrap();
    assert_eq!(index.status().unwrap().old_copies, 0);
    let left = names(&dir);
    assert!(!left.iter().any(|n| n.starts_with("chat-index.bad-")), "{left:?}");
    assert!(left.contains(&"other-file.bad-1".to_string()), "only the index's own copies go");
    assert!(left.contains(&FILE_NAME.to_string()));
}

#[test]
fn an_in_memory_index_has_no_old_copies() {
    let index = ChatIndex::start(Store::open_memory(10).unwrap(), Arc::new(Fixed(ctx())));
    assert_eq!(index.status().unwrap().old_copies, 0);
}

fn with_session(mut e: Entry, session: &str) -> Entry {
    e.session = session.into();
    e
}

#[test]
fn an_item_seen_before_and_after_its_session_id_is_one_row() {
    let mut store = Store::open_memory(100).unwrap();
    store.insert(&[with_session(row("1:0", "first look at the plan", 10), "")]).unwrap();
    assert_eq!(store.count().unwrap(), 1);
    // The same item again, now with the session id: it replaces the empty-session row and keeps its time.
    store.insert(&[with_session(row("1:0", "first look at the plan", 99), "sess-9")]).unwrap();
    assert_eq!(store.count().unwrap(), 1);
    let hits = store.search("plan", 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!((hits[0].session.as_str(), hits[0].at_ms), ("sess-9", 10));
    // And a late batch with no session id yet joins the row it already has.
    store.insert(&[with_session(row("1:0", "first look at the plan, longer now", 120), "")]).unwrap();
    assert_eq!(store.count().unwrap(), 1);
    let hits = store.search("longer", 10).unwrap();
    assert_eq!((hits.len(), hits[0].session.as_str()), (1, "sess-9"));
}

#[test]
fn two_real_sessions_of_one_pane_stay_apart() {
    let mut store = Store::open_memory(100).unwrap();
    store.insert(&[with_session(row("1:0", "alpha words", 1), "s1"), with_session(row("1:0", "beta words", 2), "s2")]).unwrap();
    assert_eq!(store.count().unwrap(), 2);
}

#[test]
fn the_labels_of_a_pane_that_is_gone_are_dropped_after_the_second_miss() {
    struct Gone;
    impl ContextLookup for Gone {
        fn context(&self, _pane: &str) -> Option<PaneContext> {
            None
        }
    }
    let mut labels = Labels::default();
    // Seen once while the pane lived.
    assert_eq!(labels.of(&Fixed(ctx()), "p").folder, "shop");
    assert_eq!(labels.len(), 1);
    // The first batch after it closed still gets its labels, the second one too, then the entry is gone.
    assert_eq!(labels.of(&Gone, "p").folder, "shop");
    assert_eq!(labels.len(), 1);
    assert_eq!(labels.of(&Gone, "p").folder, "shop");
    assert_eq!(labels.len(), 0);
    // A pane never seen gets empty labels and no entry.
    assert_eq!(labels.of(&Gone, "other"), PaneContext::default());
    assert_eq!(labels.len(), 0);
}

#[test]
fn a_session_id_seen_before_stays_while_the_lookup_has_none_yet() {
    let mut labels = Labels::default();
    labels.of(&Fixed(ctx()), "p");
    let later = PaneContext { session: String::new(), ..ctx() };
    assert_eq!(labels.of(&Fixed(later), "p").session, "sess-1");
}

mod imported_tests {
    use super::super::entry::{entry_of_with, Entry, PaneContext, Strength, MAX_IMPORTED_TEXT_CHARS};
    use super::super::imported::{import_pane_id, ImportedChatRow};
    use super::super::store::Store;
    use super::super::{open_store, FILE_NAME};
    use super::{row, temp_dir};
    use crate::session_chat::ChatItem;

    const AGENT: &str = "agent-x";

    fn chat(session: &str, stamp: &str) -> ImportedChatRow {
        ImportedChatRow {
            agent: AGENT.into(),
            session: session.into(),
            folder: "shop".into(),
            cwd: "~/projects/shop".into(),
            title: "first words".into(),
            updated_ms: 500,
            stamp: stamp.into(),
            items: 2,
            cut_short: false,
            imported_ms: 600,
        }
    }

    /// An item of an old chat, built the way the importer builds it (its import pane, imported flag).
    fn old(session: &str, id: &str, text: &str, at_ms: i64) -> Entry {
        Entry {
            pane_id: import_pane_id(AGENT),
            session: session.into(),
            item_id: id.into(),
            kind: "assistant",
            agent: AGENT.into(),
            folder: "shop".into(),
            at_ms,
            text: text.into(),
            imported: true,
        }
    }

    #[test]
    fn a_file_made_before_imports_is_migrated_and_keeps_its_rows() {
        let dir = temp_dir();
        let path = dir.join(FILE_NAME);
        {
            // The schema of a file made before the import columns existed.
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(super::super::store::SCHEMA).unwrap();
            conn.execute(
                "INSERT INTO chat_items (pane_id, session, item_id, kind, agent, folder, at_ms, text)
                 VALUES ('pane-1', 's', '1:0', 'user', 'agent-x', 'shop', 10, 'old live words')",
                [],
            )
            .unwrap();
        }
        let store = open_store(&path, 100).unwrap();
        assert_eq!(store.count().unwrap(), 1);
        let hits = store.search("live", 5).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(!hits[0].imported);
        assert_eq!(store.imported_usage().unwrap(), (0, 0));
        drop(store);
        // Opening it again changes nothing.
        let again = open_store(&path, 100).unwrap();
        assert_eq!(again.count().unwrap(), 1);
    }

    #[test]
    fn a_chat_is_written_whole_and_writing_it_twice_gives_the_same_count() {
        let mut store = Store::open_memory(100).unwrap();
        let entries = vec![old("s1", "1:0", "kittens first", 10), old("s1", "2:0", "kittens second", 20)];
        store.replace_chat(&chat("s1", "10:20"), &entries).unwrap();
        store.replace_chat(&chat("s1", "10:20"), &entries).unwrap();
        assert_eq!(store.imported_usage().unwrap(), (1, 2));
        assert_eq!(store.imported_stamp(AGENT, "s1").unwrap().as_deref(), Some("10:20"));
        // A new version of the chat replaces all of its old items.
        store.replace_chat(&chat("s1", "11:30"), &[old("s1", "1:0", "only one now", 10)]).unwrap();
        assert_eq!(store.imported_usage().unwrap(), (1, 1));
        assert!(store.search("kittens", 5).unwrap().is_empty());
        assert_eq!(store.imported_stamp(AGENT, "s1").unwrap().as_deref(), Some("11:30"));
    }

    #[test]
    fn a_failed_chat_write_leaves_the_chat_as_it_was() {
        let mut store = Store::open_memory(100).unwrap();
        store.replace_chat(&chat("s1", "10:20"), &[old("s1", "1:0", "kittens first", 10)]).unwrap();
        // Two items with one id: the write fails and nothing of the new version stays.
        let broken = vec![old("s1", "9:0", "new words", 5), old("s1", "9:0", "more new words", 6)];
        assert!(store.replace_chat(&chat("s1", "99:99"), &broken).is_err());
        assert_eq!(store.imported_stamp(AGENT, "s1").unwrap().as_deref(), Some("10:20"));
        assert_eq!(store.search("kittens", 5).unwrap().len(), 1);
        assert!(store.search("new", 5).unwrap().is_empty());
        assert_eq!(store.imported_usage().unwrap(), (1, 1));
    }

    #[test]
    fn the_imported_cap_drops_only_imported_items_and_the_live_cap_only_live_ones() {
        let mut store = Store::open_memory(3).unwrap();
        store.set_imported_cap(2);
        store.insert(&[row("1:0", "live one", 1), row("2:0", "live two", 2), row("3:0", "live three", 3)]).unwrap();
        // Three imported items against a cap of two: the oldest imported item goes, no live item does.
        store
            .replace_chat(&chat("s1", "a"), &[old("s1", "1:0", "old one", 100), old("s1", "2:0", "old two", 200), old("s1", "3:0", "old three", 300)])
            .unwrap();
        assert_eq!(store.imported_usage().unwrap(), (1, 2));
        assert!(store.search("old one", 9).unwrap().is_empty());
        assert_eq!(store.search("live", 9).unwrap().len(), 3);
        // Two more live items against a live cap of three: the two oldest live items go, the imports stay.
        store.insert(&[row("4:0", "live four", 4), row("5:0", "live five", 5)]).unwrap();
        assert!(store.search("live two", 9).unwrap().is_empty());
        assert!(store.search("live one", 9).unwrap().is_empty());
        assert_eq!(store.search("live", 9).unwrap().len(), 3);
        assert_eq!(store.imported_usage().unwrap(), (1, 2));
        assert_eq!(store.count().unwrap(), 5);
    }

    #[test]
    fn a_live_write_of_a_chat_replaces_its_imported_copy() {
        let mut store = Store::open_memory(100).unwrap();
        store.replace_chat(&chat("s", "10:20"), &[old("s", "1:0", "imported kittens", 10)]).unwrap();
        assert_eq!(store.imported_usage().unwrap(), (1, 1));
        assert!(!store.has_live(AGENT, "s").unwrap());
        // The pane runs the same chat now: the live item is kept and the imported copy is gone.
        store.insert(&[row("1:0", "live kittens", 50)]).unwrap();
        assert!(store.has_live(AGENT, "s").unwrap());
        assert_eq!(store.imported_usage().unwrap(), (0, 0));
        assert!(store.imported_chat(AGENT, "s").unwrap().is_none());
        let hits = store.search("kittens", 5).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(!hits[0].imported);
    }

    #[test]
    fn an_imported_hit_is_marked_and_no_pane_search_finds_it() {
        let mut store = Store::open_memory(100).unwrap();
        store.insert(&[row("1:0", "live kittens", 1)]).unwrap();
        store.replace_chat(&chat("s1", "a"), &[old("s1", "1:0", "imported kittens", 2)]).unwrap();
        let all = store.search("kittens", 10).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all.iter().filter(|h| h.imported).count(), 1);
        let imported_hit = all.iter().find(|h| h.imported).unwrap();
        assert_eq!(serde_json::to_value(imported_hit).unwrap()["imported"], serde_json::json!(true));
        let real: Vec<String> = ["pane-1", "companion-mochi", "pane-9"].iter().map(|p| p.to_string()).collect();
        let scoped = store.search_in("kittens", 10, &real).unwrap();
        assert_eq!(scoped.len(), 1);
        assert!(!scoped[0].imported);
    }

    #[test]
    fn the_live_search_and_the_pane_search_never_return_an_imported_row() {
        let mut store = Store::open_memory(100).unwrap();
        store.insert(&[row("1:0", "live kittens", 1)]).unwrap();
        store.replace_chat(&chat("s1", "a"), &[old("s1", "1:0", "imported kittens", 2)]).unwrap();
        // The every-row search (the desktop window) finds both; the live one (the phone) finds the live row only.
        assert_eq!(store.search("kittens", 10).unwrap().len(), 2);
        let live = store.search_live("kittens", 10).unwrap();
        assert_eq!(live.len(), 1);
        assert!(!live[0].imported);
        // Even when the old chat's own pane id is asked for, nothing imported comes back.
        assert!(store.search_in("kittens", 10, &[import_pane_id(AGENT)]).unwrap().is_empty());
        // The desktop's filtered search still finds the old chat (the chips see every row); the live search does not.
        let filters = super::super::Filters { agent: Some(AGENT.into()), ..Default::default() };
        assert!(store.search_live("kittens", 10).unwrap().iter().all(|hit| !hit.imported));
        assert!(store.search_filtered("kittens", 10, &filters).unwrap().iter().any(|hit| hit.imported));
    }

    #[test]
    fn an_imported_text_is_strictly_masked_and_cut_at_two_thousand() {
        let ctx = PaneContext::of(AGENT, "C:\\Users\\you\\projects\\shop", Some("s1"));
        let token = "sk-ant-api03-FakeFakeFakeFake0123456789abcdef";
        let hex = "0123456789abcdef0123456789abcdef01234567";
        let text = format!("key {token} mail someone@example.com hash {hex} file C:\\Users\\you\\proj\\.env");
        let item = ChatItem::User { id: "1:0".into(), at: None, text };
        let e = entry_of_with("import:agent-x", &ctx, &item, 1, Strength::Strict, true, MAX_IMPORTED_TEXT_CHARS).expect("kept");
        for secret in ["FakeFake", "someone@example.com", hex, "Users\\you"] {
            assert!(!e.text.contains(secret), "{}", e.text);
        }
        assert!(e.imported);
        let long = ChatItem::Assistant { id: "2:0".into(), at: None, text: "word ".repeat(1000), model: None, final_answer: true };
        let cut = entry_of_with("import:agent-x", &ctx, &long, 1, Strength::Strict, true, MAX_IMPORTED_TEXT_CHARS).expect("kept");
        assert_eq!(cut.text.chars().count(), MAX_IMPORTED_TEXT_CHARS + 1);
        assert!(cut.text.ends_with('…'));
    }

    #[test]
    fn the_record_reads_back_with_its_title_masked_and_cut() {
        let mut store = Store::open_memory(100).unwrap();
        let mut record = chat("s1", "7:8");
        record.title = format!("Use sk-ant-api03-FakeFakeFakeFake0123456789abcdef and someone@example.com {}", "x".repeat(200));
        store.replace_chat(&record, &[old("s1", "1:0", "kittens", 10)]).unwrap();
        let back = store.imported_chat(AGENT, "s1").unwrap().expect("the record");
        assert!(!back.title.contains("FakeFake") && !back.title.contains("someone@example.com"), "{}", back.title);
        assert!(back.title.chars().count() <= 120, "{}", back.title);
        assert_eq!((back.folder.as_str(), back.cwd.as_str(), back.updated_ms, back.items), ("shop", "~/projects/shop", 500, 2));
        assert_eq!((back.cut_short, back.imported_ms), (false, 600));
        assert!(store.imported_chat(AGENT, "other").unwrap().is_none());
        assert!(!store.has_live(AGENT, "s1").unwrap());
        assert_eq!(store.imported_stamp(AGENT, "other").unwrap(), None);
    }

    #[test]
    fn clearing_the_imports_keeps_the_live_rows_and_erases_the_imported_text_from_the_file() {
        let dir = temp_dir();
        let path = dir.join(FILE_NAME);
        let mut store = open_store(&path, 1000).unwrap();
        store.insert(&[row("1:0", "live LIVEMARKER42", 1)]).unwrap();
        store.replace_chat(&chat("s1", "a"), &[old("s1", "1:0", "imported IMPORTMARK9913 text", 2)]).unwrap();
        let needle: &[u8] = b"IMPORTMARK9913";
        let in_file = |file: &std::path::Path| std::fs::read(file).unwrap_or_default().windows(needle.len()).any(|w| w == needle);
        assert!(in_file(&path) || in_file(&dir.join(format!("{FILE_NAME}-wal"))), "the text is saved before the clear");

        store.clear_imported().unwrap();
        assert_eq!(store.imported_usage().unwrap(), (0, 0));
        assert!(store.imported_chat(AGENT, "s1").unwrap().is_none());
        assert_eq!(store.search("LIVEMARKER42", 5).unwrap().len(), 1);
        for file in [path.clone(), dir.join(format!("{FILE_NAME}-wal"))] {
            assert!(!in_file(&file), "{file:?} still has the text");
        }
        assert_eq!(store.integrity().unwrap(), "ok");
    }

    #[test]
    fn removing_the_imports_leaves_no_marker_in_the_index_file_or_its_log() {
        let dir = temp_dir();
        let path = dir.join(FILE_NAME);
        let wal = dir.join(format!("{FILE_NAME}-wal"));
        let mut store = open_store(&path, 1000).unwrap();
        store.insert(&[row("1:0", "live LIVEMARK5511 words", 1)]).unwrap();
        // Three old chats long enough to fill pages; the marker is in the first chat's title and in every text.
        for n in 0..3 {
            let session = format!("s{n}");
            let mut record = chat(&session, "a");
            if n == 0 {
                record.title = "titled OLDMARK5511".into();
            }
            let entries: Vec<Entry> = (0..120)
                .map(|i| old(&session, &format!("{i}:0"), &format!("OLDMARK5511 number {i} {}", "filler words ".repeat(30)), i as i64))
                .collect();
            store.replace_chat(&record, &entries).unwrap();
        }
        let needle: &[u8] = b"OLDMARK5511";
        let in_file = |file: &std::path::Path| std::fs::read(file).unwrap_or_default().windows(needle.len()).any(|w| w == needle);
        assert!(in_file(&path) || in_file(&wal), "the marker is saved before the removal");

        store.clear_imported().unwrap();
        assert_eq!(store.imported_usage().unwrap(), (0, 0));
        assert_eq!(std::fs::metadata(&wal).map(|m| m.len()).unwrap_or(0), 0, "the log is emptied");
        assert!(!in_file(&path), "the removed text is still in the index file");
        assert!(!in_file(&wal), "the removed text is still in the log");
        assert_eq!(store.search("LIVEMARK5511", 5).unwrap().len(), 1);
        assert_eq!(store.integrity().unwrap(), "ok");
    }

    #[test]
    fn clear_removes_the_imported_chats_too() {
        let mut store = Store::open_memory(100).unwrap();
        store.replace_chat(&chat("s1", "a"), &[old("s1", "1:0", "imported words", 2)]).unwrap();
        store.clear().unwrap();
        assert_eq!(store.imported_usage().unwrap(), (0, 0));
        assert_eq!(store.count().unwrap(), 0);
    }
}

#[test]
fn a_wsl_mount_user_folder_becomes_a_tilde_and_never_a_cut_drive() {
    // `/mnt/c/Users/<name>` loses the name and keeps the rest of the path (never `/mnt/c~`).
    let e = entry(user("5:0", None, "open /mnt/c/Users/you/proj/a.rs now"));
    assert!(!e.text.contains("you"), "{}", e.text);
    assert!(!e.text.contains("/mnt/c~"), "{}", e.text);
    assert!(e.text.contains("~/proj/a.rs"), "{}", e.text);
    // A pane in the WSL home folder shows as `~`.
    let home = PaneContext::of("agent-x", "/mnt/c/Users/you", None);
    assert_eq!(home.folder, "~");
}
