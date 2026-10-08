//! The `[tui]` table engine on synthetic screens (real Kilo captures arrive in T23).

use std::time::Duration;

use serde_json::json;

use crate::pty::status::PaneStatus;
use crate::tui::declarative::build_for_test;
use crate::tui::declarative::screen::SCREEN_CAP;
use crate::tui::types::{Reply, StandbyScreen};
use crate::tui::{generic, TuiProfile};

const GENERIC: &str = include_str!("../fixtures/definitions/generic.toml");
const KILO_LIKE: &str = include_str!("../fixtures/definitions/kilo-like.toml");
const MINIMAL_TUI: &str = include_str!("../fixtures/definitions/minimal-tui.toml");

fn gen() -> impl TuiProfile {
    build_for_test(GENERIC).unwrap()
}

#[test]
fn busy_marker_detects_working_not_idle() {
    let p = gen();
    assert_eq!(p.busy_marker("some output\nWorking... 12s\n"), Some(true));
    assert_eq!(p.busy_marker("answer text\nesc to stop\n"), Some(true));
    assert_eq!(p.busy_marker("answer text\n>\n"), Some(false));
    assert_eq!(p.busy_marker("only plain text\n"), None);
    // working beats idle when both show
    assert_eq!(p.busy_marker("Working...\n>\n"), Some(true));
    // only the last 12 non-empty lines count
    let old = format!("Working...\n{}", "filler\n".repeat(20));
    assert_eq!(p.busy_marker(&old), None);
    // ANSI is stripped first
    assert_eq!(p.busy_marker("\u{1b}[1mWorking...\u{1b}[0m"), Some(true));
    // a definition without [tui.busy] cannot tell
    let kilo_without = build_for_test(&KILO_LIKE.replace("working_regex = [\"esc\\\\s+interrupt\"]", "working_regex = []")).unwrap();
    assert_eq!(kilo_without.busy_marker("esc interrupt"), None);
    let kilo = build_for_test(KILO_LIKE).unwrap();
    assert_eq!(kilo.busy_marker("running\nesc   interrupt\n"), Some(true));
}

#[test]
fn chrome_and_tool_rows_match_trimmed_lowercased_lines() {
    let p = gen();
    assert!(p.is_chrome_line("model: gpt/5"));
    assert!(p.is_chrome_line("  ? for help  "));
    assert!(!p.is_chrome_line("the model: is in the middle"));
    assert!(!p.is_chrome_line(""));
    assert!(p.is_tool_activity_line("running: cargo test"));
    assert!(p.is_tool_activity_line("\u{2022} ran ls"));
    assert!(!p.is_tool_activity_line("it was running: nothing"));
    let m = build_for_test(MINIMAL_TUI).unwrap();
    assert!(!m.is_chrome_line("model: x") && !m.is_tool_activity_line("running: x"));
}

#[test]
fn menu_detected_in_the_tail_only() {
    let p = gen();
    assert!(p.looks_like_menu("pick one\n1. Yes\n2. No\nenter to select"));
    assert!(!p.looks_like_menu("just some output"));
    let stale = format!("enter to select\n{}", "filler\n".repeat(20));
    assert!(!p.looks_like_menu(&stale));
    assert!(!build_for_test(MINIMAL_TUI).unwrap().looks_like_menu("enter to select"));
}

#[test]
fn strip_dialogs_removes_terminated_blocks_only() {
    let p = gen();
    let screen = "reply one\n+---- trust this folder\n| yes | no |\n+------+\nreply two\n";
    assert_eq!(p.strip_dialogs(screen), "reply one\nreply two\n");
    // two blocks
    let twice = format!("{screen}{screen}");
    assert_eq!(p.strip_dialogs(&twice), "reply one\nreply two\nreply one\nreply two\n");
    // an unterminated block is left alone: the rest of the screen is not lost
    let open = "reply\n+---- trust this folder\n| yes | no |\n";
    assert_eq!(p.strip_dialogs(open), open);
    // no table: identity
    let m = build_for_test(MINIMAL_TUI).unwrap();
    assert_eq!(m.strip_dialogs(screen), screen);
}

#[test]
fn tidy_dedupes_adjacent_lines_when_asked() {
    let p = gen();
    assert_eq!(p.tidy_reply("a\na\n  a  \nb\na\n"), "a\nb\na\n");
    assert_eq!(p.tidy_reply("a\n\n\nb"), "a\n\n\nb");
    let m = build_for_test(MINIMAL_TUI).unwrap();
    assert_eq!(m.tidy_reply("a\na\n"), "a\na\n");
}

#[test]
fn footer_model_takes_the_last_match() {
    let p = gen();
    assert_eq!(p.detect_footer_model("hello\nmodel: old/one\nstuff\nmodel: kilo/auto-free\n"), Some("kilo/auto-free".into()));
    assert_eq!(p.detect_footer_model("no footer here"), None);
    assert_eq!(p.detect_footer_model("\u{1b}[2mmodel: x/y\u{1b}[0m"), Some("x/y".into()));
    assert_eq!(build_for_test(MINIMAL_TUI).unwrap().detect_footer_model("model: x/y"), None);
}

#[test]
fn classify_waiting_regex_marks_waiting_input() {
    let p = gen();
    assert_eq!(p.classify_screen("blah\nDo you want to proceed?\n"), Some(PaneStatus::WaitingInput));
    assert_eq!(p.classify_screen("blah\nall done\n"), None);
    assert_eq!(p.classify_screen(""), None);
    assert_eq!(build_for_test(MINIMAL_TUI).unwrap().classify_screen("Do you want to proceed?"), None);
}

#[test]
fn prompts_detect_answer_and_suppress() {
    let p = gen();
    let prompt = p.detect_prompt("Run it\nAllow once?\n", false).unwrap();
    assert_eq!(prompt["kind"], "command_approval");
    assert_eq!(prompt["agent_type"], "gentui");
    assert_eq!(prompt["choices"], json!(["allow_once", "deny"]));
    assert_eq!(p.prompt_reply(&prompt, "allow_once", false).unwrap(), Reply::key("y"));
    assert_eq!(p.prompt_reply(&prompt, "deny", false).unwrap(), Reply::key("n"));
    assert_eq!(p.prompt_reply(&prompt, "nope", false).unwrap_err(), "unsupported approval choice");
    // answered dialogs that stay in scrollback are suppressed
    assert!(p.detect_prompt("Allow once?\nanswered: yes\n", false).is_none());
    // text choice
    let q = p.detect_prompt("Please type your answer", false).unwrap();
    assert_eq!(p.prompt_reply(&q, "skip", false).unwrap(), Reply { key: None, text: Some("skip\r".into()) });
    // an unrelated screen falls back to the generic heuristic (and so does an unknown kind)
    assert!(p.detect_prompt("hello", false).is_none());
    assert!(p.detect_prompt("Approve this command?", false).is_some());
}

/// `broad` / `allow_broad` behave exactly like `generic::detect_prompt` / `generic::prompt_reply`.
#[test]
fn broad_semantics_match_generic() {
    let p = gen();
    let screen = "Always allow this command?";
    let without = p.detect_prompt(screen, false).unwrap();
    assert_eq!(without["choices"], json!(["allow_once", "deny"]), "allow_all is not advertised without allow_broad");
    let with = p.detect_prompt(screen, true).unwrap();
    assert_eq!(with["choices"], json!(["allow_once", "deny", "allow_all"]));

    let generic_broad = generic::detect_prompt("x", screen, true).unwrap();
    let generic_err = generic::prompt_reply(&generic_broad, "allow_all", false).unwrap_err();
    assert_eq!(p.prompt_reply(&with, "allow_all", false).unwrap_err(), generic_err);
    assert_eq!(p.prompt_reply(&without, "allow_all", false).unwrap_err(), generic_err);
    assert_eq!(p.prompt_reply(&with, "allow_all", true).unwrap(), Reply::key(""), "enter is the empty reply: the route appends Enter");
    // a non-broad prompt never grants allow_all even with allow_broad
    let plain = p.detect_prompt("Allow once?", true).unwrap();
    assert_eq!(p.prompt_reply(&plain, "allow_all", true).unwrap_err(), generic_err);
}

#[test]
fn standby_screen_follows_ready_and_dialogs() {
    let p = gen();
    assert_eq!(p.standby_screen("banner\nmodel: x/y\n> \n"), StandbyScreen::Ready);
    assert_eq!(p.standby_screen("starting up..."), StandbyScreen::NotReady);
    assert_eq!(p.standby_screen("Do you trust this folder?\n>\n"), StandbyScreen::Dialog);
    assert_eq!(p.standby_screen("Allow once?\n>\n"), StandbyScreen::Dialog);
    // TUI-3: without [tui.ready] a profile is never a ready standby
    let m = build_for_test(MINIMAL_TUI).unwrap();
    assert_eq!(m.standby_screen("> \n"), StandbyScreen::NotReady);
    assert!(!m.standby_support().visible);
    assert!(gen().standby_support().visible || !GENERIC.contains("visible = true"));
}

#[test]
fn clear_input_and_delivery_flags() {
    assert_eq!(gen().clear_input_sequence(), Some("\u{15}"));
    assert_eq!(build_for_test(MINIMAL_TUI).unwrap().clear_input_sequence(), None);
    assert!(!gen().verifies_prompt_delivery());
    let with = GENERIC.replace(
        "[tui.classify]",
        "[tui.delivery]\ninput_echo_regex = \"^>\"\ntranscript_echo_regex = \"^you:\"\n\n[tui.classify]",
    );
    assert!(build_for_test(&with).unwrap().verifies_prompt_delivery());
}

#[test]
fn idle_windows_are_clamped() {
    let with = |idle: &str, quiet: &str| {
        let text = GENERIC
            .replace("idle_after_ms = 3000", &format!("idle_after_ms = {idle}"))
            .replace("quiet_idle_after_ms = 900", &format!("quiet_idle_after_ms = {quiet}"));
        build_for_test(&text).unwrap()
    };
    let ms = Duration::from_millis;
    let p = with("10", "5");
    assert_eq!((p.idle_after(), p.quiet_idle_after()), (ms(1500), ms(500)));
    let p = with("99999", "99999");
    assert_eq!((p.idle_after(), p.quiet_idle_after()), (ms(5000), ms(5000)));
    let p = with("2000", "4000");
    assert_eq!((p.idle_after(), p.quiet_idle_after()), (ms(2000), ms(2000)), "quiet never exceeds idle");
    let p = build_for_test(MINIMAL_TUI).unwrap();
    assert_eq!((p.idle_after(), p.quiet_idle_after()), (ms(2500), ms(2500)), "defaults");
}

#[test]
fn screen_text_is_capped_to_its_tail() {
    let p = gen();
    let filler = "x".repeat(SCREEN_CAP + 10);
    // a marker only in the dropped head is not seen; one in the kept tail is
    assert_eq!(p.detect_footer_model(&format!("model: head/one\n{filler}")), None);
    assert_eq!(p.detect_footer_model(&format!("{filler}\nmodel: tail/two")), Some("tail/two".into()));
    assert!(p.strip_dialogs(&filler).len() <= SCREEN_CAP);
    assert!(p.tidy_reply(&filler).len() <= SCREEN_CAP);
    // multi-byte text near the cut does not panic
    let wide = "\u{2022}".repeat(SCREEN_CAP);
    assert!(p.tidy_reply(&wide).len() <= SCREEN_CAP);
}

#[test]
fn unknown_key_in_a_prompt_is_rejected() {
    let bad = MINIMAL_TUI.replace("key = \"y\"", "key = \"hyper+q\"");
    assert!(build_for_test(&bad).is_err());
}

#[test]
fn visible_pane_launch_renders_tui_argv() {
    use crate::tui::declarative::template::render_tui;
    let text = KILO_LIKE.replace("argv = []\nidle_after_ms", "argv = [\"--pure\"]\nread_only_argv = [\"--read-only\"]\nidle_after_ms");
    let def = crate::tui::declarative::parse_definition(&text).unwrap();
    let tui = def.tui.as_ref().unwrap();
    assert_eq!(render_tui(tui, "C:/p", false, None, None), ["--pure"]);
    assert_eq!(render_tui(tui, "C:/p", false, Some("a/b"), Some("ses_1")), ["--pure", "-m", "a/b", "-s", "ses_1"]);
    assert_eq!(render_tui(tui, "C:/p", true, None, None), ["--pure", "--read-only"]);
}
