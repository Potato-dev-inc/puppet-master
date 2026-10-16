//! Screen classification and stability for the guided capture (SPEC 11.5), on real captures kept in the
//! repository (Claude Code, Codex, Cursor, Kilo) and on small made-up screens.

use crate::tui::declarative::wizard::visible::screen::{
    classify, frame_key, last_row_with, row_with, rows_with, Flavour, FrameBag, ScreenKind, Stability,
};

fn fixture(rel: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tui").join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn kind_of(rel: &str) -> ScreenKind {
    classify(&fixture(rel))
}

#[test]
fn real_idle_screens_are_input_or_unknown_never_a_dialog() {
    for rel in [
        "claude/fixtures/ready_fresh_2_1_299_live.txt",
        "claude/fixtures/ready_after_turn_2_1_299_live.txt",
        "codex/fixtures/standby_ready_live.txt",
    ] {
        assert_eq!(kind_of(rel), ScreenKind::Input, "{rel}");
    }
    // A screen the classifier cannot read is Unknown: the wizard then asks the person.
    for rel in ["codex/fixtures/result_footer_live.txt", "cursor/fixtures/input_box_unframed_live.txt", "opencode/fixtures/standby_ready_live.txt"] {
        assert!(matches!(kind_of(rel), ScreenKind::Unknown | ScreenKind::Input), "{rel}: {:?}", kind_of(rel));
    }
}

#[test]
fn real_trust_questions_are_dialogs_the_wizard_never_answers() {
    for rel in [
        "claude/fixtures/trust_dialog_2_1_299.txt",
        "claude/fixtures/trust_card_verify_r7_live.txt",
        "codex/fixtures/trust_folder_live.txt",
        "cursor/fixtures/workspace_trust_screen.txt",
    ] {
        assert_eq!(kind_of(rel), ScreenKind::Dialog(Flavour::Trust), "{rel}");
    }
}

#[test]
fn real_permission_questions_are_dialogs() {
    for rel in [
        "claude/fixtures/bash_simple_4_options_2_1_299.txt",
        "claude/fixtures/create_file_2_1_299.txt",
        "claude/fixtures/edit_file_2_1_299.txt",
        "cursor/fixtures/command_approval_visible.txt",
        "declarative/fixtures/kilo/tui_permission.txt",
    ] {
        assert_eq!(kind_of(rel), ScreenKind::Dialog(Flavour::Permission), "{rel}");
    }
    assert_eq!(kind_of("codex/fixtures/update_prompt_live.txt"), ScreenKind::Dialog(Flavour::Question));
}

#[test]
fn real_working_screens_are_working() {
    for rel in ["claude/fixtures/working_2_1_299_live.txt", "codex/fixtures/working_live.txt", "declarative/fixtures/kilo/tui_working.txt"] {
        assert_eq!(kind_of(rel), ScreenKind::Working, "{rel}");
    }
}

#[test]
fn small_screens_are_read_the_way_a_person_would() {
    assert_eq!(classify(""), ScreenKind::Blank);
    assert_eq!(classify("   \n\n  "), ScreenKind::Blank);
    assert_eq!(classify(r"PS C:\Users\me\proj> "), ScreenKind::Shell);
    assert_eq!(classify("Allow nimbus to create hello.txt? [y/n]"), ScreenKind::Dialog(Flavour::Permission));
    assert_eq!(classify("Please sign in to continue.\n1. Yes\n2. No"), ScreenKind::Dialog(Flavour::Login));
    assert_eq!(classify("Welcome\n\n| > Type a task...    |"), ScreenKind::Input);
    assert_eq!(classify("| thinking...\n| > Type a task..."), ScreenKind::Working);
    assert_eq!(classify("loading models\nready soon"), ScreenKind::Unknown);
    // an input box next to question words is not trusted on its own
    assert_eq!(classify("Do you trust the authors?\n> "), ScreenKind::Dialog(Flavour::Trust));
    assert_eq!(classify("approve all changes later\n> Type a task..."), ScreenKind::Unknown);
}

#[test]
fn an_answered_dialog_that_scrolled_away_is_not_a_dialog() {
    let mut screen = String::from("Allow once? [y/n]\nyes\n");
    for i in 0..30 {
        screen.push_str(&format!("transcript line {i}\n"));
    }
    screen.push_str("> Type a task...\n");
    assert_eq!(classify(&screen), ScreenKind::Input);
}

#[test]
fn frame_keys_fold_what_animates() {
    let a = frame_key("⠋ Thinking 3s\nbox\n");
    let b = frame_key("⠙ Thinking 12s\nbox\n");
    assert_eq!(a, b);
    assert_eq!(frame_key("took 3s"), frame_key("took 1234s"), "a run of digits is one digit");
    assert_ne!(frame_key("one\n"), frame_key("two\n"));
}

#[test]
fn one_animated_row_does_not_reset_the_timer_but_two_changed_rows_do() {
    let mut s = Stability::new();
    assert!(s.observe("title\nbody\nclock 10:01\n", 0));
    assert!(!s.observe("title\nbody\nclock 10:02\n", 1000), "digits are folded");
    assert!(!s.observe("title\nbody\ntick\n", 2000), "one differing row is tolerated");
    assert_eq!(s.stable_for(2600), 2600);
    assert!(s.observe("different title\nother body\ntick\n", 2700), "two rows differ: a new picture");
    assert_eq!(s.stable_for(3000), 300);
}

#[test]
fn the_most_frequent_frame_is_kept_and_the_others_are_noted_as_variants() {
    let mut s = Stability::new();
    for (i, text) in ["menu\nready  \n", "menu\nready\n", "menu\nready\n", "menu\nbusy-a\n"].iter().enumerate() {
        s.observe(text, i as u64 * 250);
    }
    let (best, variants) = s.best().unwrap();
    assert_eq!(best, "menu\nready");
    assert_eq!(variants, vec!["busy-a".to_string()]);
    assert_eq!(s.samples(), 4);
}

#[test]
fn the_working_frame_bag_skips_the_pictures_it_is_told_to() {
    let mut bag = FrameBag::default();
    for text in ["top\n* thinking\nbox", "top\n* thinking\nbox", "top\nreply\nbox", "top\n* working hard\nbox"] {
        bag.add(text);
    }
    let reply = frame_key("top\nreply\nbox");
    let (best, variants) = bag.best_excluding(&[reply]).unwrap();
    assert_eq!(best, "top\n* thinking\nbox");
    assert!(variants.contains(&"* working hard".to_string()));
    assert!(bag.best_excluding(&[frame_key("top\n* thinking\nbox"), frame_key("top\nreply\nbox"), frame_key("top\n* working hard\nbox")]).is_none());
}

#[test]
fn row_helpers_find_the_word() {
    let screen = "> Reply with exactly the word: pong-abc\n  pong-abc\nfooter";
    assert_eq!(rows_with(screen, "PONG-ABC"), 2);
    assert_eq!(row_with(screen, "pong-abc"), Some("> Reply with exactly the word: pong-abc"));
    assert_eq!(last_row_with(screen, "pong-abc"), Some("pong-abc"));
    assert_eq!(row_with(screen, "zzz"), None);
}
