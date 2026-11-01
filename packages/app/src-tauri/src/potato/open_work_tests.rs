//! Open work: the list lives in the bot's vault file. What it shows: add, done and remove touch only their own line; a
//! line a person wrote stays; the text is one short line; a bot that is not known is refused; a damaged file is an
//! error and is never overwritten; the running helpers are filtered to the bot.

use std::path::PathBuf;

use super::helpers::Running;
use super::open_work::*;
use super::store;

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pm-potato-openwork-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    store::add_bot(&dir, "nimbus", "fish", 1).unwrap();
    dir
}

fn file_text(root: &PathBuf) -> String {
    std::fs::read_to_string(path(root, "nimbus").unwrap()).unwrap()
}

#[test]
fn an_empty_list_is_empty_and_the_first_add_writes_the_header() {
    let root = temp("first");
    assert!(list(&root, "nimbus").unwrap().is_empty());
    let item = add(&root, "nimbus", "Send the weekly summary to Sam").unwrap();
    assert!(is_id(&item.id));
    assert_eq!(item.text, "Send the weekly summary to Sam");
    assert!(!item.done);
    let text = file_text(&root);
    assert!(text.starts_with("# Open work\n\n- [ ] Send the weekly summary to Sam ^ow-"), "{text}");
    assert_eq!(list(&root, "nimbus").unwrap(), vec![item]);
}

#[test]
fn done_and_remove_touch_only_their_own_line_and_a_person_s_own_lines_stay() {
    let root = temp("edit");
    let first = add(&root, "nimbus", "Check the build").unwrap();
    let second = add(&root, "nimbus", "Email the list").unwrap();
    // A line a person wrote by hand is kept as it is.
    let mut text = file_text(&root);
    text.push_str("Remember: Sam likes short emails.\n");
    std::fs::write(path(&root, "nimbus").unwrap(), text).unwrap();

    let done = set_done(&root, "nimbus", &first.id, true).unwrap();
    assert!(done.done);
    let items = list(&root, "nimbus").unwrap();
    assert_eq!(items.len(), 2);
    assert!(items[0].done && items[0].id == first.id);
    assert!(!items[1].done && items[1].id == second.id);
    assert!(file_text(&root).contains("Remember: Sam likes short emails."));

    set_done(&root, "nimbus", &first.id, false).unwrap();
    assert!(!list(&root, "nimbus").unwrap()[0].done);

    remove(&root, "nimbus", &second.id).unwrap();
    let left = list(&root, "nimbus").unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].id, first.id);
    let text = file_text(&root);
    assert!(text.contains("Remember: Sam likes short emails."));
    assert!(!text.contains("Email the list"));
}

#[test]
fn unknown_items_and_bad_ids_are_refused() {
    let root = temp("missing");
    add(&root, "nimbus", "Something").unwrap();
    assert_eq!(set_done(&root, "nimbus", "ow-00000000", true).unwrap_err().code, "POTATO_OPEN_WORK_NOT_FOUND");
    assert_eq!(remove(&root, "nimbus", "../etc").unwrap_err().code, "POTATO_OPEN_WORK_NOT_FOUND");
    assert_eq!(set_done(&root, "nimbus", "ow-ABCDEF12", true).unwrap_err().code, "POTATO_OPEN_WORK_NOT_FOUND");
}

#[test]
fn a_bot_that_is_not_known_is_refused_and_no_file_is_written() {
    let root = temp("unknown");
    assert_eq!(list(&root, "ghost").unwrap_err().code, "POTATO_BOT_NOT_FOUND");
    assert_eq!(add(&root, "ghost", "x").unwrap_err().code, "POTATO_BOT_NOT_FOUND");
    assert!(!root.join("brain").join("ghost").exists());
}

#[test]
fn text_is_one_short_line_and_an_empty_item_is_refused() {
    let root = temp("text");
    assert_eq!(add(&root, "nimbus", "   \n  ").unwrap_err().code, "POTATO_OPEN_WORK_EMPTY");
    let item = add(&root, "nimbus", "first line\nsecond   line").unwrap();
    assert_eq!(item.text, "first line second line");
    let long = "word ".repeat(100);
    let cut = add(&root, "nimbus", &long).unwrap();
    assert_eq!(cut.text.chars().count(), MAX_TEXT_CHARS);
    assert_eq!(list(&root, "nimbus").unwrap().len(), 2);
}

#[test]
fn a_damaged_file_is_an_error_and_is_never_overwritten() {
    let root = temp("damaged");
    let file = path(&root, "nimbus").unwrap();
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, [0xff, 0xfe, 0x00, 0x41]).unwrap();
    assert_eq!(list(&root, "nimbus").unwrap_err().code, "POTATO_OPEN_WORK_INVALID");
    assert_eq!(add(&root, "nimbus", "new").unwrap_err().code, "POTATO_OPEN_WORK_INVALID");
    assert_eq!(std::fs::read(&file).unwrap(), vec![0xff, 0xfe, 0x00, 0x41]);
}

#[test]
fn a_line_is_an_item_only_when_its_id_is_well_formed() {
    assert_eq!(
        parse_line("- [x] Done thing ^ow-0b7d44aa"),
        Some(Item { id: "ow-0b7d44aa".into(), text: "Done thing".into(), done: true })
    );
    assert!(parse_line("- [ ] No id here").is_none());
    assert!(parse_line("- [ ] Bad id ^ow-XYZ").is_none());
    assert!(parse_line("- [ ] Upper ^ow-0B7D44AA").is_none());
    assert!(parse_line("# Open work").is_none());
    assert!(parse_line("Plain sentence ^ow-0b7d44aa").is_none());
}

#[test]
fn the_helper_rows_are_the_running_helpers_of_this_bot() {
    let running = |bot: &str, id: &str| Running {
        bot: bot.into(),
        request_id: id.into(),
        folder_name: "demo".into(),
        project: "demo".into(),
        operation_id: "op".into(),
        pane_id: None,
        started_ms: 42,
    };
    let rows = helper_rows(&[running("nimbus", "r1"), running("other", "r2")], "nimbus");
    assert_eq!(rows, vec![HelperRow { request_id: "r1".into(), project: "demo".into(), started_ms: 42 }]);
}
