//! Recently deleted (UX-2): a deleted note, routine or whole bot moves aside into `potato/trash/<id>/` with a record,
//! lists newest first, comes back intact by its id, and is purged once it is older than 7 days. These tests use temp
//! folders only; no real app data is touched.

use std::path::{Path, PathBuf};

use super::bot_settings;
use super::routines::{self, RoutineInput, Schedule};
use super::store;
use super::trash::{self, DAY_MS};

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pm-potato-trash-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    store::add_bot(&dir, "nimbus", "fish", 1).unwrap();
    dir
}

fn write(path: &Path, body: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

fn note_path(root: &Path, rel: &str) -> PathBuf {
    store::vault_dir(root, "nimbus").unwrap().join(rel)
}

fn write_note(root: &Path, rel: &str, body: &str) {
    write(&note_path(root, rel), body);
}

fn routine_input(title: &str) -> RoutineInput {
    RoutineInput { title: title.into(), prompt: "Sweep the inbox.".into(), schedule: Schedule::Daily { time: "06:00".into() } }
}

#[test]
fn a_deleted_note_is_listed_in_the_trash_and_restores_intact() {
    let root = temp("note");
    write_note(&root, "journal/2026/10/2026-10-28.md", "# day\n- 10:00 went for a walk\n");

    let item = trash::trash_note(&root, "nimbus", "journal/2026/10/2026-10-28.md", 1_000).unwrap();
    assert_eq!(item.kind, "note");
    assert_eq!(item.bot, "nimbus");
    assert_eq!(item.title, "2026-10-28");
    assert!(!note_path(&root, "journal/2026/10/2026-10-28.md").exists(), "the note left the memory");

    let listed = trash::list(&root).unwrap();
    assert_eq!(listed.iter().map(|i| i.id.clone()).collect::<Vec<_>>(), vec![item.id.clone()]);

    trash::restore(&root, &item.id).unwrap();
    assert_eq!(std::fs::read_to_string(note_path(&root, "journal/2026/10/2026-10-28.md")).unwrap(), "# day\n- 10:00 went for a walk\n");
    assert!(trash::list(&root).unwrap().is_empty(), "a restored item leaves the trash");
}

#[test]
fn a_deleted_routine_restores_with_its_schedule_and_prompt() {
    let root = temp("routine");
    let made = routines::create(&root, "nimbus", &routine_input("Morning sweep"), 5).unwrap();

    let item = trash::trash_routine(&root, &made.id, 1_000).unwrap();
    assert_eq!(item.kind, "routine");
    assert_eq!(item.title, "Morning sweep");
    assert!(routines::list(&root, Some("nimbus")).unwrap().is_empty());

    trash::restore(&root, &item.id).unwrap();
    let back = routines::list(&root, Some("nimbus")).unwrap();
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].id, made.id);
    assert_eq!(back[0].title, "Morning sweep");
    assert_eq!(back[0].summary, made.summary);
}

#[test]
fn a_deleted_bot_keeps_its_memory_and_routines_and_comes_back_whole() {
    let root = temp("bot");
    write_note(&root, "nodes/people/sam.md", "# Sam\nLikes short emails.\n");
    write(&store::conversation_path(&root, &store::app_conversation("nimbus")).unwrap(), "{\"line\":1}\n");
    let settings = bot_settings::path(&root, "nimbus").unwrap();
    write(&settings, "{\"brain\":\"go\"}");
    let made = routines::create(&root, "nimbus", &routine_input("Morning sweep"), 5).unwrap();
    let mut reads = store::load_reads(&root).unwrap();
    reads.insert("nimbus".into(), 42);
    store::write_json(&store::reads_path(&root), &reads, "reads.json").unwrap();

    let item = trash::trash_bot(&root, "nimbus", "Nimbus", 1_000).unwrap();
    assert_eq!(item.kind, "bot");
    assert_eq!(item.title, "Nimbus");
    assert!(store::load_bots(&root).unwrap().is_empty(), "the bot left the list");
    assert!(!note_path(&root, "nodes/people/sam.md").exists(), "its memory left the brain folder");
    assert!(routines::list(&root, Some("nimbus")).unwrap().is_empty());

    trash::restore(&root, &item.id).unwrap();
    let bots = store::load_bots(&root).unwrap();
    assert_eq!(bots.len(), 1);
    assert_eq!(bots[0].id, "nimbus");
    assert_eq!(bots[0].talk_persona, "fish");
    assert_eq!(std::fs::read_to_string(note_path(&root, "nodes/people/sam.md")).unwrap(), "# Sam\nLikes short emails.\n");
    assert_eq!(std::fs::read_to_string(&settings).unwrap(), "{\"brain\":\"go\"}");
    assert!(store::conversation_path(&root, &store::app_conversation("nimbus")).unwrap().exists());
    let back = routines::list(&root, Some("nimbus")).unwrap();
    assert_eq!(back.iter().map(|r| r.id.clone()).collect::<Vec<_>>(), vec![made.id]);
    assert_eq!(store::load_reads(&root).unwrap().get("nimbus"), Some(&42));
}

#[test]
fn restoring_a_bot_refuses_a_name_that_is_back_already() {
    let root = temp("bot-clash");
    let item = trash::trash_bot(&root, "nimbus", "Nimbus", 1_000).unwrap();
    store::add_bot(&root, "nimbus", "potato", 2).unwrap();
    let err = trash::restore(&root, &item.id).unwrap_err();
    assert_eq!(err.code, "POTATO_TRASH_CONFLICT");
    assert_eq!(trash::list(&root).unwrap().len(), 1, "a refused restore keeps the item in the trash");
}

#[test]
fn an_item_eight_days_old_is_purged_and_one_six_days_old_is_kept() {
    let root = temp("purge");
    let now = 100 * DAY_MS;
    write_note(&root, "nodes/old.md", "old");
    write_note(&root, "nodes/recent.md", "recent");
    trash::trash_note(&root, "nimbus", "nodes/old.md", now - 8 * DAY_MS).unwrap();
    trash::trash_note(&root, "nimbus", "nodes/recent.md", now - 6 * DAY_MS).unwrap();

    assert_eq!(trash::purge_expired(&root, now).unwrap(), 1);
    let left = trash::list(&root).unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].title, "recent");
}

#[test]
fn an_unknown_id_is_refused_and_a_bad_id_never_leaves_the_trash_folder() {
    let root = temp("unknown");
    assert_eq!(trash::restore(&root, "no-such-item").unwrap_err().code, "POTATO_TRASH_NOT_FOUND");
    assert_eq!(trash::restore(&root, "../../bots").unwrap_err().code, "POTATO_TRASH_NOT_FOUND");
}
