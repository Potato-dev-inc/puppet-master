//! What the bot remembers: the list reads the note folders only; a note is read and deleted only by a path that the
//! checks accept. What it shows: the list is newest first and skips the brain's own folders, the open-work file and
//! dot names; every path that leaves the bot's folder, names a non-note, or goes through a link is refused as "not
//! found"; a delete removes one note and nothing else.

use std::path::{Path, PathBuf};

use super::memory_notes::*;
use super::store;

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pm-potato-memory-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    store::add_bot(&dir, "nimbus", "fish", 1).unwrap();
    dir
}

fn write_note(root: &Path, rel: &str, body: &str) {
    let full = store::vault_dir(root, "nimbus").unwrap().join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
    std::fs::create_dir_all(full.parent().unwrap()).unwrap();
    std::fs::write(full, body).unwrap();
}

fn set_age(root: &Path, rel: &str, secs: u64) {
    // Older files sort later: the list is newest first.
    let full = store::vault_dir(root, "nimbus").unwrap().join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
    let when = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000 + secs);
    let file = std::fs::File::options().write(true).open(full).unwrap();
    file.set_modified(when).unwrap();
}

#[test]
fn the_list_shows_the_notes_newest_first_and_skips_bookkeeping() {
    let root = temp("list");
    write_note(&root, "journal/2026/10/2026-10-28.md", "# day\n- 10:00 went for a walk\n");
    write_note(&root, "nodes/people/sam.md", "# Sam\n");
    write_note(&root, "diary/2026-10-29.md", "# diary\n");
    write_note(&root, "state/cursor.md", "bookkeeping");
    write_note(&root, "sessions/cp-001.md", "checkpoint");
    write_note(&root, "nodes/open-work.md", "# Open work\n");
    write_note(&root, "nodes/.hidden.md", "secret-ish");
    write_note(&root, "nodes/notes.txt", "not markdown");
    set_age(&root, "journal/2026/10/2026-10-28.md", 10);
    set_age(&root, "nodes/people/sam.md", 5);
    set_age(&root, "diary/2026-10-29.md", 20);

    let paths: Vec<String> = list(&root, "nimbus").unwrap().into_iter().map(|n| n.path).collect();
    assert_eq!(paths, vec!["diary/2026-10-29.md", "journal/2026/10/2026-10-28.md", "nodes/people/sam.md"]);
}

#[test]
fn a_note_is_read_by_its_path_and_a_delete_removes_only_that_note() {
    let root = temp("read");
    write_note(&root, "nodes/people/sam.md", "# Sam\nLikes short emails.\n");
    write_note(&root, "nodes/people/lee.md", "# Lee\n");
    assert_eq!(read(&root, "nimbus", "nodes/people/sam.md").unwrap(), "# Sam\nLikes short emails.\n");

    delete(&root, "nimbus", "nodes/people/sam.md").unwrap();
    assert!(!store::vault_dir(&root, "nimbus").unwrap().join("nodes/people/sam.md").exists());
    assert!(store::vault_dir(&root, "nimbus").unwrap().join("nodes/people/lee.md").exists());
    let paths: Vec<String> = list(&root, "nimbus").unwrap().into_iter().map(|n| n.path).collect();
    assert_eq!(paths, vec!["nodes/people/lee.md"]);
    assert_eq!(read(&root, "nimbus", "nodes/people/sam.md").unwrap_err().code, "POTATO_NOTE_NOT_FOUND");
}

#[test]
fn every_path_that_leaves_the_notes_or_names_a_non_note_is_refused() {
    let root = temp("paths");
    write_note(&root, "nodes/people/sam.md", "# Sam\n");
    write_note(&root, "state/cursor.md", "bookkeeping");
    write_note(&root, "nodes/open-work.md", "# Open work\n");
    std::fs::write(root.join("bots.json.md"), "outside").unwrap();
    let refused = [
        "",
        "nodes",
        "../bots.json",
        "nodes/../../bots.json",
        "nodes/../people/sam.md",
        "/nodes/people/sam.md",
        "C:/nodes/people/sam.md",
        "nodes\\people\\sam.md",
        "nodes/people/sam.txt",
        "state/cursor.md",
        "sessions/cp-001.md",
        "nodes/open-work.md",
        "nodes/.hidden.md",
        "nodes/./people/sam.md",
        "nodes/people/missing.md",
        "nodes/people/sam.md\0",
    ];
    for rel in refused {
        assert_eq!(checked_path(&root, "nimbus", rel).unwrap_err().code, "POTATO_NOTE_NOT_FOUND", "{rel:?}");
        assert!(read(&root, "nimbus", rel).is_err(), "{rel:?}");
        assert!(delete(&root, "nimbus", rel).is_err(), "{rel:?}");
    }
    // The bookkeeping and the open-work file are still there after every refused delete.
    assert!(store::vault_dir(&root, "nimbus").unwrap().join("state/cursor.md").exists());
    assert!(store::vault_dir(&root, "nimbus").unwrap().join("nodes/open-work.md").exists());
    assert!(root.join("bots.json.md").exists());
}

#[test]
fn a_bot_that_is_not_known_has_no_notes() {
    let root = temp("unknown");
    assert_eq!(list(&root, "ghost").unwrap_err().code, "POTATO_BOT_NOT_FOUND");
    assert_eq!(read(&root, "ghost", "nodes/people/sam.md").unwrap_err().code, "POTATO_BOT_NOT_FOUND");
}

#[test]
fn a_long_note_is_cut_with_a_line_that_says_so() {
    let root = temp("long");
    let big = "a".repeat(MAX_VIEW_BYTES as usize + 500);
    write_note(&root, "journal/2026/10/2026-10-30.md", &big);
    let text = read(&root, "nimbus", "journal/2026/10/2026-10-30.md").unwrap();
    assert!(text.ends_with("[Cut short: the note is longer than 256 KB.]"));
    assert!(text.len() < big.len());
}

#[test]
fn a_secret_in_a_note_is_not_shown() {
    let root = temp("secret");
    write_note(&root, "nodes/people/sam.md", "Sam's key is sk-abcdefghijklmnopqrstuvwxyz123456\n");
    let text = read(&root, "nimbus", "nodes/people/sam.md").unwrap();
    assert!(!text.contains("sk-abcdefghijklmnopqrstuvwxyz123456"), "{text}");
}

#[cfg(unix)]
#[test]
fn a_link_on_the_way_is_refused() {
    let root = temp("link");
    let outside = root.join("outside.md");
    std::fs::write(&outside, "not the bot's").unwrap();
    let nodes = store::vault_dir(&root, "nimbus").unwrap().join("nodes");
    std::fs::create_dir_all(&nodes).unwrap();
    std::os::unix::fs::symlink(&outside, nodes.join("link.md")).unwrap();
    assert_eq!(read(&root, "nimbus", "nodes/link.md").unwrap_err().code, "POTATO_NOTE_NOT_FOUND");
    assert_eq!(delete(&root, "nimbus", "nodes/link.md").unwrap_err().code, "POTATO_NOTE_NOT_FOUND");
    assert!(outside.exists());
    assert!(list(&root, "nimbus").unwrap().is_empty());
}
