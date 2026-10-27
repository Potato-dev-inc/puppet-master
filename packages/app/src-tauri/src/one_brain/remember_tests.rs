//! "Remember for all agents": cleaning, redaction, de-duplication, the off switches and Undo, on temp projects.

use std::path::{Path, PathBuf};

use super::remember::{clean, EMPTY, MAX_CHARS, MEMORY_OFF_APP, MEMORY_OFF_PROJECT, SECRET};
use super::{forget_with, read_memory, remember_with, set_project_enabled, write_memory};
use crate::redact::RedactContext;

fn project() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pm-remember-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn ctx() -> RedactContext {
    RedactContext::default()
}

fn remember(dir: &Path, text: &str) -> Result<super::remember::Remembered, String> {
    remember_with(dir, text, true, &ctx())
}

fn code(err: &str) -> &str {
    err.split(':').next().unwrap_or(err)
}

#[test]
fn clean_makes_one_plain_line() {
    assert_eq!(clean("  Use\ttabs,\n\nnot   spaces.  "), "Use tabs, not spaces.");
    assert_eq!(clean("a\u{0007}b\u{001b}[0m c"), "ab[0m c");
    assert_eq!(clean("- - Run the tests first"), "Run the tests first");
}

#[test]
fn first_remember_creates_the_memory_with_one_line() {
    let dir = project();
    let done = remember(&dir, "Run cargo test --lib before pushing.").unwrap();
    assert!(done.added);
    assert_eq!(done.text, "Run cargo test --lib before pushing.");
    assert_eq!(read_memory(&dir), "- Run cargo test --lib before pushing.\n");
}

#[test]
fn remember_appends_after_the_existing_text() {
    let dir = project();
    write_memory(&dir, "We use Postgres.\n").unwrap();
    remember(&dir, "Use tabs.").unwrap();
    assert_eq!(read_memory(&dir), "We use Postgres.\n- Use tabs.\n");
}

#[test]
fn a_line_already_in_the_memory_is_not_added_twice() {
    let dir = project();
    write_memory(&dir, "- Use tabs.\nWe use Postgres.\n").unwrap();
    let before = read_memory(&dir);
    let again = remember(&dir, "  USE   tabs. ").unwrap();
    assert!(!again.added);
    assert_eq!(read_memory(&dir), before);
    // The bare text (no marker) of a line the person wrote by hand also counts as there.
    write_memory(&dir, "We use Postgres.\n").unwrap();
    assert!(!remember(&dir, "we use postgres.").unwrap().added);
    assert_eq!(read_memory(&dir), "We use Postgres.\n");
}

#[test]
fn empty_or_blank_selection_is_refused_and_nothing_is_written() {
    let dir = project();
    assert_eq!(code(&remember(&dir, " \n\t ").unwrap_err()), EMPTY);
    assert_eq!(read_memory(&dir), "");
}

#[test]
fn a_secret_is_refused_and_never_stored() {
    let dir = project();
    let token = remember(&dir, "Our key is sk-ant-api03-ABCDEFGH12345678 use it.").unwrap_err();
    assert_eq!(code(&token), SECRET);
    let known = remember_with(
        &dir,
        "The password is hunter2-very-secret.",
        true,
        &RedactContext { env_values: vec!["hunter2-very-secret".into()], home: None },
    )
    .unwrap_err();
    assert_eq!(code(&known), SECRET);
    assert_eq!(read_memory(&dir), "");
}

#[test]
fn a_secret_cut_by_the_cap_is_still_refused() {
    // Redaction runs before the cap: the key starts before character 300, so it is caught whole.
    let dir = project();
    let text = format!("{} sk-ant-api03-ABCDEFGHIJKLMNOPQRSTUVWXYZ", "a".repeat(290));
    assert_eq!(code(&remember(&dir, &text).unwrap_err()), SECRET);
    assert_eq!(read_memory(&dir), "");
}

#[test]
fn a_long_selection_is_capped_to_one_line_of_300_characters() {
    let dir = project();
    let done = remember(&dir, &"b".repeat(500)).unwrap();
    assert_eq!(done.text.chars().count(), MAX_CHARS);
    let lines: Vec<String> = read_memory(&dir).lines().map(str::to_string).collect();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].chars().count(), MAX_CHARS + 2);
}

#[test]
fn the_app_wide_switch_and_the_project_switch_refuse_with_their_own_code() {
    let dir = project();
    let app_off = remember_with(&dir, "Use tabs.", false, &ctx()).unwrap_err();
    assert_eq!(code(&app_off), MEMORY_OFF_APP);
    set_project_enabled(&dir, false).unwrap();
    let project_off = remember(&dir, "Use tabs.").unwrap_err();
    assert_eq!(code(&project_off), MEMORY_OFF_PROJECT);
    assert_eq!(read_memory(&dir), "");
    set_project_enabled(&dir, true).unwrap();
    assert!(remember(&dir, "Use tabs.").unwrap().added);
}

#[test]
fn undo_removes_only_that_line_and_keeps_the_rest_byte_for_byte() {
    let dir = project();
    write_memory(&dir, "# Notes\r\nWe use Postgres.\r\n").unwrap();
    let original = read_memory(&dir);
    let done = remember(&dir, "Use tabs.").unwrap();
    assert_eq!(read_memory(&dir), format!("{original}- Use tabs.\n"));
    assert!(forget_with(&dir, &done.text).unwrap());
    assert_eq!(read_memory(&dir), original);
    // A second Undo, or an Undo of a line that is gone, changes nothing.
    assert!(!forget_with(&dir, &done.text).unwrap());
    assert_eq!(read_memory(&dir), original);
}

#[test]
fn undo_leaves_an_earlier_identical_line_alone() {
    let dir = project();
    write_memory(&dir, "- Use tabs.\n- Use tabs.\n").unwrap();
    assert!(forget_with(&dir, "Use tabs.").unwrap());
    assert_eq!(read_memory(&dir), "- Use tabs.\n");
}
