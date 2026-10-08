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
