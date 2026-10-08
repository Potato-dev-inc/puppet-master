//! The shipped Kilo definition (`tests/agent-definitions/kilo.toml`) against REAL captures from the
//! kilo CLI (`fixtures/kilo/*`, sessionIDs and paths scrubbed). Nothing here is hand-written output.

use std::path::{Path, PathBuf};

use crate::tui::declarative::loader::load_text;
use crate::tui::declarative::sources::Origin;
use crate::tui::declarative::validate::Source;
use crate::tui::declarative::DeclarativeProfile;
use crate::tui::types::HeadlessLaunch;
use crate::tui::TuiProfile;

const KILO_TOML: &str = include_str!("../../../../tests/agent-definitions/kilo.toml");
const SUCCESS: &str = include_str!("../fixtures/kilo/headless_success.jsonl");
const TOOL_TURN: &str = include_str!("../fixtures/kilo/headless_tool_turn.jsonl");
const RESUME: &str = include_str!("../fixtures/kilo/headless_resume.jsonl");
const ERROR: &str = include_str!("../fixtures/kilo/headless_error.jsonl");
const GARBAGE: &str = include_str!("../fixtures/kilo/headless_garbage.txt");

fn profile() -> DeclarativeProfile {
    let file = load_text(
        Path::new("kilo.toml"),
        KILO_TOML,
        KILO_TOML.len(),
        Origin::new(Source::User),
        None,
        &["claude", "codex"],
    );
    assert!(file.errors.is_empty(), "kilo.toml must validate cleanly: {:?}", file.errors);
    DeclarativeProfile::new(file.definition.expect("valid")).unwrap()
}

fn args(p: &DeclarativeProfile, launch: &HeadlessLaunch<'_>) -> Vec<String> {
    p.headless_spec(launch, PathBuf::from("kilo-bin")).unwrap().args
}

#[test]
fn kilo_toml_validates_and_declares_the_verified_facts() {
    let p = profile();
    let f = p.features();
    assert!(f.dispatchable && f.resumable_headless);
    assert!(f.read_only_headless, "read-only is --agent ask (T05 check 5)");
    let run = p.run_options();
    assert!(run.allow_shell && run.shell_default_on, "shell needs no flag (T05 checks 1-2)");
    assert!(p.accepts_headless_model());
    // `--auto` is never produced: it is not the shell switch
    let mut launch = HeadlessLaunch::new("C:/proj", "t");
    launch.allow_shell = true;
    assert!(!args(&p, &launch).iter().any(|a| a == "--auto"));
}

#[test]
fn headless_success_extracts_result_and_session() {
    let p = profile();
    assert_eq!(p.extract_result(SUCCESS, 0).as_deref(), Some("pong"));
    assert_eq!(p.extract_provider_session(SUCCESS).as_deref(), Some("ses_FIXTURE_SUCCESS"));
    assert!(p.headless_result_ready(SUCCESS), "step_finish reason=stop is the done marker");
    assert_eq!(p.extract_model(SUCCESS).as_deref(), Some("stealth/glyph-cluster"));
    // the done marker alone is enough: no process exit is needed to know the turn is over
    let first_two: String = SUCCESS.lines().take(2).map(|l| format!("{l}\n")).collect();
    assert!(!p.headless_result_ready(&first_two));
    assert!(p.headless_result_ready(SUCCESS));
}

#[test]
fn tool_turn_result_is_the_last_non_blank_text() {
    let p = profile();
    assert!(TOOL_TURN.contains("\"reason\":\"tool-calls\""), "fixture really has an intermediate step");
    assert!(TOOL_TURN.contains(r#""text":"\n\n\n""#), "fixture really has a blank text event");
    assert_eq!(p.extract_result(TOOL_TURN, 0).as_deref(), Some("alpha"));
    assert_eq!(p.extract_provider_session(TOOL_TURN).as_deref(), Some("ses_FIXTURE_TOOLTURN"));
    assert!(p.headless_result_ready(TOOL_TURN));
    // a turn cut at the tool-calls step (rejected permission) is not "done"
    let cut: String = TOOL_TURN.lines().take(3).map(|l| format!("{l}\n")).collect();
    assert!(!p.headless_result_ready(&cut));
}

#[test]
fn resume_continues_the_same_session() {
    let p = profile();
    assert_eq!(p.extract_provider_session(RESUME), p.extract_provider_session(SUCCESS));
    assert_eq!(p.extract_result(RESUME, 0).as_deref(), Some("pong; alpha"));
}

#[test]
fn error_event_maps_to_reported_error() {
    let p = profile();
    assert_eq!(p.extract_result(ERROR, 1), None);
    assert_eq!(p.extract_result(ERROR, 0), None);
    assert!(p.headless_result_ready(ERROR), "an error event ends the turn");
    let (code, message) = p.classify_headless_failure(ERROR, 0).expect("classified");
    assert_eq!(code, "AGENT_REPORTED_ERROR");
    assert_eq!(message, "Model not found: nosuchprovider/nosuchmodel.", "the specific message wins");
    assert_eq!(p.classify_headless_failure(ERROR, 1).map(|c| c.0), Some("AGENT_REPORTED_ERROR"));
}

#[test]
fn garbage_returns_none() {
    let p = profile();
    assert_eq!(p.extract_result(GARBAGE, 0), None);
    assert_eq!(p.extract_provider_session(GARBAGE), None);
    assert!(!p.headless_result_ready(GARBAGE));
    assert_eq!(p.classify_headless_failure(GARBAGE, 0).map(|c| c.0), Some("AGENT_OUTPUT_UNPARSEABLE"));
}

#[test]
fn resume_argv_contains_session() {
    let p = profile();
    let mut launch = HeadlessLaunch::new("C:/proj", "-dash task");
    launch.model = Some("kilo/kilo-auto/free");
    launch.resume_session = Some("ses_FIXTURE_SUCCESS");
    assert_eq!(
        args(&p, &launch),
        ["run", "--format", "json", "-m", "kilo/kilo-auto/free", "-s", "ses_FIXTURE_SUCCESS", "--", "-dash task"]
    );
    launch.resume_session = None;
    launch.model = None;
    assert_eq!(args(&p, &launch), ["run", "--format", "json", "--", "-dash task"]);
    launch.read_only = true;
    let spec = p.headless_spec(&launch, PathBuf::from("kilo-bin")).unwrap();
    assert_eq!(spec.args, ["run", "--format", "json", "--agent", "ask", "--", "-dash task"]);
    assert!(spec.read_only_enforced);
}

// ---- visible mode (T23): real captures of the kilo 7.8.8 TUI --------------------------------

const TUI_IDLE: &str = include_str!("../fixtures/kilo/tui_idle.txt");
const TUI_WORKING: &str = include_str!("../fixtures/kilo/tui_working.txt");
const TUI_PERMISSION: &str = include_str!("../fixtures/kilo/tui_permission.txt");
const TUI_REPLY: &str = include_str!("../fixtures/kilo/tui_reply.txt");

/// A capture without its `# capture:` header line.
fn screen(fixture: &str) -> &str {
    assert!(fixture.starts_with("# capture: kilo 7.8.8"), "fixture header records the kilo version");
    fixture.split_once('\n').unwrap().1
}

/// Rows of a capture: split at newlines, and rows longer than the 121-column pane (full-width rows
/// the app's screen text joins together) cut back apart.
fn rows(fixture: &str) -> Vec<String> {
    screen(fixture)
        .split('\n')
        .flat_map(|piece| {
            let chars: Vec<char> = piece.chars().collect();
            chars.chunks(121).map(|c| c.iter().collect::<String>()).collect::<Vec<_>>()
        })
        .collect()
}

#[test]
fn tui_busy_matches_working_not_idle() {
    let p = profile();
    assert_eq!(p.busy_marker(screen(TUI_WORKING)), Some(true), "esc interrupt is in the footer");
    assert_eq!(p.busy_marker(screen(TUI_PERMISSION)), None, "the dialog replaces the footer: not busy, not idle");
    assert_eq!(p.busy_marker(screen(TUI_IDLE)), Some(false));
    assert_eq!(p.busy_marker(screen(TUI_REPLY)), Some(false), "a finished reply is idle");
    assert_eq!(p.busy_marker("some unrelated output"), None);
}

#[test]
fn tui_permission_prompt_detected() {
    let p = profile();
    let prompt = p.detect_prompt(screen(TUI_PERMISSION), false).expect("permission dialog");
    assert_eq!(prompt["kind"], "command_approval");
    assert_eq!(prompt["agent_type"], "kilo");
    assert_eq!(prompt["choices"], serde_json::json!(["allow_once", "deny"]));
    // the answer routes type the reply and then press Enter themselves
    assert_eq!(p.prompt_reply(&prompt, "allow_once", false).unwrap(), crate::tui::types::Reply::key(""));
    assert_eq!(p.prompt_reply(&prompt, "deny", false).unwrap(), crate::tui::types::Reply::key("\u{1b}"));
    assert!(p.looks_like_menu(screen(TUI_PERMISSION)), "select/confirm hints mark an open menu");
    // the same turn after it was answered (dialog gone) is not a prompt, nor is any other capture
    for other in [TUI_IDLE, TUI_WORKING, TUI_REPLY] {
        assert!(p.detect_prompt(screen(other), true).is_none());
        assert!(!p.looks_like_menu(screen(other)));
    }
}

#[test]
fn tui_reply_tidied() {
    let p = profile();
    // chrome: the footer hint row, sidebar headings/totals and the model row are never reply text
    let idle_rows = rows(TUI_IDLE);
    assert!(idle_rows.iter().any(|r| p.is_chrome_line(&r.trim().to_lowercase())), "footer hint row is chrome");
    assert!(p.is_chrome_line("tab agents  ctrl+p commands"));
    assert!(p.is_chrome_line("\u{2b1d}\u{2b1d}\u{25a0}\u{25a0}\u{25a0}\u{25a0}\u{25a0}  esc interrupt"));
    assert!(p.is_chrome_line("\u{25bc} context"));
    assert!(p.is_chrome_line("22,450 tokens"));
    assert!(p.is_chrome_line("9% used"));
    assert!(p.is_chrome_line("$0.00 spent"));
    assert!(p.is_chrome_line("\u{25a3}  code \u{b7} auto free \u{b7} nemotron 3 ultra (free) \u{b7} 19.0s"));
    // tool rows
    assert!(p.is_tool_activity_line("+ thought: 343ms \u{b7} nemotron 3 ultra (free)"));
    assert!(p.is_tool_activity_line("\u{2192} read .env  \u{b7} approved by you"));
    assert!(p.is_tool_activity_line("\u{2826} read .env"));
    // the answer is neither chrome nor a tool row
    for answer in ["the variable name is secret_fixture_value.", "alpha", "tuipong"] {
        assert!(!p.is_chrome_line(answer) && !p.is_tool_activity_line(answer), "{answer}");
    }
    // what is left of the real reply screen keeps the answer
    let kept: Vec<String> = rows(TUI_REPLY)
        .into_iter()
        .filter(|r| {
            let lower = r.trim().to_lowercase();
            !lower.is_empty() && !p.is_chrome_line(&lower) && !p.is_tool_activity_line(&lower)
        })
        .collect();
    assert!(kept.iter().any(|r| r.contains("The variable name is SECRET_FIXTURE_VALUE.")), "{kept:#?}");
    // dedupe/dialogs tables are absent: both pass text through
    assert_eq!(p.tidy_reply("a\na\n"), "a\na\n");
    assert_eq!(p.strip_dialogs(screen(TUI_REPLY)), screen(TUI_REPLY));
}

#[test]
fn tui_footer_model_comes_from_the_reply_row() {
    let p = profile();
    assert_eq!(p.detect_footer_model(screen(TUI_REPLY)).as_deref(), Some("Nemotron 3 Ultra (free)"));
    assert_eq!(p.detect_footer_model(screen(TUI_WORKING)), None, "no model is shown while the turn runs");
    assert_eq!(p.detect_footer_model(screen(TUI_IDLE)), None);
}

#[test]
fn tui_launch_argv_and_idle_windows() {
    let p = profile();
    let spec = p
        .spawn_command(&crate::tui::types::SpawnRequest {
            cwd: ".",
            extra_args: &["-m".to_string(), "kilo/kilo-auto/free".to_string()],
            cols: 121,
            rows: 36,
            read_only: false,
        })
        .map(|s| s.args);
    // the real kilo is resolved on this machine; a CI box without it gets None
    if let Some(args) = spec {
        assert_eq!(args, ["-m", "kilo/kilo-auto/free"]);
    }
    assert!(p.verifies_prompt_delivery(), "typed, verified in the box, then submitted (T24)");
    assert_eq!(p.clear_input_sequence(), Some("\u{15}"));
    assert_eq!(p.idle_after(), std::time::Duration::from_millis(2500));
    assert_eq!(p.quiet_idle_after(), std::time::Duration::from_millis(1200));
}
