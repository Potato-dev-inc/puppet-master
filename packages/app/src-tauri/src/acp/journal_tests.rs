use super::*;
use crate::session_chat::{ChatItem, TurnOutcome};

fn dir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("pm-acp-journal-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn user(n: u64, text: &str) -> ChatItem {
    ChatItem::User { id: format!("u{n}"), at: None, text: text.into() }
}

fn assistant(id: &str, text: &str) -> ChatItem {
    ChatItem::Assistant { id: id.into(), at: None, text: text.into(), model: None, final_answer: false }
}

fn texts(items: &[ChatItem]) -> Vec<String> {
    items.iter().map(|i| serde_json::to_value(i).unwrap()["text"].as_str().unwrap_or("").to_string()).collect()
}

fn settle() {
    std::thread::sleep(COALESCE * 4);
}

#[test]
fn a_killed_process_has_every_item_it_handed_over() {
    let d = dir();
    let path = d.join("c.jsonl");
    let journal = Journal::at(&path);
    for n in 0..50 {
        journal.append(&user(n, &format!("message {n}")));
    }
    settle();
    // The process dies here: no flush, no close. Only the file is left.
    std::mem::forget(journal);
    let back = load(&path);
    assert_eq!(back.items.len(), 50);
    assert_eq!(back.skipped, 0);
    assert_eq!(texts(&back.items)[49], "message 49");
}

#[test]
fn the_end_of_a_turn_is_on_disk_at_once() {
    let d = dir();
    let path = d.join("c.jsonl");
    let journal = Journal::at(&path);
    for n in 0..5 {
        journal.append(&user(n, "x"));
    }
    journal.append(&ChatItem::TurnEnd { id: "end:1".into(), at: None, outcome: TurnOutcome::Completed, duration_ms: None });
    let back = load(&path);
    assert_eq!(back.items.len(), 6, "no waiting for the timer");
}

#[test]
fn a_growing_message_keeps_its_place_and_its_latest_text() {
    let d = dir();
    let path = d.join("c.jsonl");
    let journal = Journal::at(&path);
    journal.append(&user(1, "hi"));
    for n in 1..=40 {
        journal.append(&assistant("a1.1", &"w ".repeat(n)));
    }
    journal.append(&assistant("a1.2", "second"));
    journal.flush();
    let back = load(&path);
    let ids: Vec<&str> = back.items.iter().map(|i| i.id()).collect();
    assert_eq!(ids, vec!["u1", "a1.1", "a1.2"]);
    assert_eq!(texts(&back.items)[1], "w ".repeat(40));
    let lines = std::fs::read_to_string(&path).unwrap().lines().count();
    assert!(lines < 40, "chunks are coalesced, not one line each: {lines}");
}

#[test]
fn a_half_written_last_line_is_skipped_and_the_next_line_starts_clean() {
    let d = dir();
    let path = d.join("c.jsonl");
    let journal = Journal::at(&path);
    journal.append(&user(1, "one"));
    journal.append(&user(2, "two"));
    journal.flush();
    // The kill cut the next line in the middle.
    let mut raw = std::fs::read(&path).unwrap();
    raw.extend_from_slice(br#"{"v":1,"at":1,"item":{"kind":"user","id":"u3","te"#);
    std::fs::write(&path, raw).unwrap();
    let back = load(&path);
    assert_eq!(texts(&back.items), vec!["one", "two"]);
    assert_eq!(back.skipped, 1);
    // A new run appends after the broken line: that line is not glued to the cut one.
    let again = Journal::at(&path);
    again.append(&user(4, "four"));
    again.flush();
    let back = load(&path);
    assert_eq!(texts(&back.items), vec!["one", "two", "four"]);
}

#[test]
fn garbage_and_unknown_lines_are_never_fatal() {
    let d = dir();
    let path = d.join("c.jsonl");
    std::fs::write(&path, "not json\n\n{\"v\":9,\"item\":{\"kind\":\"hologram\"}}\n\u{0}\u{0}\n").unwrap();
    let back = load(&path);
    assert!(back.items.is_empty());
    assert_eq!(back.skipped, 3);
    assert!(load(&d.join("missing.jsonl")).items.is_empty());
}

#[test]
fn nothing_secret_reaches_the_file() {
    let d = dir();
    let path = d.join("c.jsonl");
    let journal = Journal::at(&path);
    let secret = format!("ghp_{}", "a1B2c3D4e5".repeat(4));
    journal.append(&user(1, &format!("my token is {secret} ok")));
    journal.flush();
    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(!raw.contains(&secret), "{raw}");
    assert!(raw.contains("my token is"));
}

#[test]
fn a_big_journal_is_compacted_and_keeps_the_latest_of_each_item() {
    let d = dir();
    let path = d.join("c.jsonl");
    let journal = Journal::at(&path);
    journal.append(&user(1, "first"));
    journal.flush();
    // Fill the file with old copies of one growing message.
    let filler = "x".repeat(200_000);
    for n in 0..60 {
        journal.append(&assistant("a1.1", &format!("{filler}{n}")));
        journal.flush();
    }
    let size = std::fs::metadata(&path).unwrap().len();
    assert!(size < MAX_BYTES + 400_000, "the file stays bounded: {size}");
    let back = load(&path);
    assert_eq!(back.items.len(), 2);
    assert!(texts(&back.items)[1].ends_with("59"));
    // And it still takes lines afterwards.
    journal.append(&user(2, "after"));
    journal.flush();
    assert_eq!(load(&path).items.len(), 3);
}

#[test]
fn a_reset_keeps_the_old_words_until_the_new_chat_has_some() {
    let d = dir();
    let path = d.join("c.jsonl");
    let journal = Journal::at(&path);
    journal.append(&user(1, "old words"));
    journal.flush();
    journal.reset();
    let kept = Journal::at(&path).load_or_previous();
    assert_eq!(texts(&kept.items), vec!["old words"], "a rebuild that never finished costs nothing");
    journal.append(&user(1, "rebuilt"));
    journal.flush();
    assert_eq!(texts(&Journal::at(&path).load_or_previous().items), vec!["rebuilt"]);
}
