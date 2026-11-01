//! "Notify me" per bot: the default is a reply push, the choice is saved per bot, `never` and `needs_you` push no
//! reply, an unknown bot is refused, and a damaged file is an error that is never overwritten.

use std::path::PathBuf;

use super::notify::*;
use super::store;

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pm-potato-notify-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    store::add_bot(&dir, "nimbus", "fish", 1).unwrap();
    store::add_bot(&dir, "mochi", "potato", 2).unwrap();
    dir
}

#[test]
fn the_default_is_a_push_when_it_replies_and_each_bot_keeps_its_own_choice() {
    let root = temp("default");
    assert_eq!(get(&root, "nimbus").unwrap(), Notify::Reply);
    assert!(pushes_replies(&root, "nimbus"));

    assert_eq!(set(&root, "mochi", Notify::Never).unwrap(), Notify::Never);
    assert_eq!(get(&root, "mochi").unwrap(), Notify::Never);
    assert!(!pushes_replies(&root, "mochi"));
    assert_eq!(get(&root, "nimbus").unwrap(), Notify::Reply, "another bot's choice is not touched");

    set(&root, "nimbus", Notify::NeedsYou).unwrap();
    assert_eq!(get(&root, "nimbus").unwrap(), Notify::NeedsYou);
    assert!(!pushes_replies(&root, "nimbus"), "needs-you is stored; it does not push a reply");
}

#[test]
fn the_wire_words_are_the_three_choices_and_nothing_else() {
    assert_eq!(Notify::parse("reply"), Some(Notify::Reply));
    assert_eq!(Notify::parse("needs_you"), Some(Notify::NeedsYou));
    assert_eq!(Notify::parse("never"), Some(Notify::Never));
    assert_eq!(Notify::parse("always"), None);
    assert_eq!(Notify::parse(""), None);
}

#[test]
fn an_unknown_bot_is_refused_and_nothing_is_written() {
    let root = temp("unknown");
    assert_eq!(get(&root, "ghost").unwrap_err().code, "POTATO_BOT_NOT_FOUND");
    assert_eq!(set(&root, "ghost", Notify::Never).unwrap_err().code, "POTATO_BOT_NOT_FOUND");
    assert!(!root.join("notify.json").exists());
    assert!(!pushes_replies(&root, "ghost"));
}

#[test]
fn a_damaged_file_is_an_error_that_is_never_overwritten() {
    let root = temp("damaged");
    std::fs::write(root.join("notify.json"), "{ damaged").unwrap();
    assert!(get(&root, "nimbus").is_err());
    assert_eq!(set(&root, "nimbus", Notify::Never).unwrap_err().code, "POTATO_STORE_INVALID");
    assert!(!pushes_replies(&root, "nimbus"), "a damaged file pushes nothing");
    assert_eq!(std::fs::read_to_string(root.join("notify.json")).unwrap(), "{ damaged");
}
