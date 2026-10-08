use std::path::PathBuf;

use super::{KILO, MINIMAL};
use crate::tui::declarative::{parse_definition, DeclarativeProfile};
use crate::tui::types::{HeadlessLaunch, Kind, StandbySupport};
use crate::tui::TuiProfile;

fn profile(text: &str) -> DeclarativeProfile {
    DeclarativeProfile::new(parse_definition(text).unwrap()).unwrap()
}

fn kilo() -> DeclarativeProfile {
    profile(KILO)
}

fn spec_args(p: &DeclarativeProfile, launch: &HeadlessLaunch<'_>) -> Vec<String> {
    p.headless_spec(launch, PathBuf::from("kilo-bin")).unwrap().args
}

#[test]
fn features_follow_definition() {
    let k = kilo();
    assert_eq!(k.id(), "kilo");
    let f = k.features();
    assert_eq!(f.kind, Kind::Cli);
    assert!(f.dispatchable);
    assert!(!f.read_only_headless);
    assert!(!f.read_only_spawned_pane);
    assert!(f.resumable_headless);
    let run = k.run_options();
    assert!(run.allow_shell);
    assert!(!run.trust_workspace && !run.shell_default_on && !run.allowed_commands);
    assert!(k.accepts_headless_model());
    assert_eq!(k.standby_support(), StandbySupport::default());

    let m = profile(MINIMAL);
    assert_eq!(m.id(), "mycoder");
    assert!(!m.features().resumable_headless);
    assert!(!m.accepts_headless_model());
}

#[test]
fn not_dispatchable_reason_and_read_only_note() {
    let text = KILO.replace("dispatchable = true", "dispatchable = false\nnot_dispatchable_reason = \"nope\"");
    let k = profile(&text);
    assert!(!k.features().dispatchable);
    assert_eq!(k.not_dispatchable_reason(), "nope");
    let text = KILO.replace("read_only_headless = false", "read_only_note = \"policy text\"");
    assert!(profile(&text).capability_notes(false).iter().any(|n| n == "policy text"));
}

#[test]
fn headless_launch_renders_argv() {
    let k = kilo();
    let mut launch = HeadlessLaunch::new("C:/proj", "do it");
    assert_eq!(spec_args(&k, &launch), ["run", "--format", "json", "--", "do it"]);
    launch.model = Some("kilo/auto");
    launch.resume_session = Some("ses_1");
    launch.allow_shell = true;
    assert_eq!(
        spec_args(&k, &launch),
        ["run", "--format", "json", "-m", "kilo/auto", "-s", "ses_1", "--auto", "--", "do it"]
    );
    let spec = k.headless_spec(&launch, PathBuf::from("kilo-bin")).unwrap();
    assert_eq!(spec.program, "kilo-bin");
    assert_eq!(spec.cwd, PathBuf::from("C:/proj"));
    assert!(!spec.read_only_enforced);
    // hostile task text stays one argv element after the separator
    let hostile = HeadlessLaunch::new("p", "a; {model} && `x`");
    assert_eq!(spec_args(&k, &hostile).last().unwrap(), "a; {model} && `x`");
}

#[test]
fn read_only_placement_and_enforcement() {
    let text = KILO
        .replace("read_only_headless = false", "read_only_headless = true\nread_only_note = \"plan agent\"")
        .replace(
            "[[headless.optional]]\nwhen = \"allow_shell\"",
            "[[headless.optional]]\nwhen = \"read_only\"\nargs = [\"--agent\", \"plan\"]\n\n[[headless.optional]]\nwhen = \"allow_shell\"",
        );
    let k = profile(&text);
    assert!(k.features().read_only_headless);
    let mut launch = HeadlessLaunch::new("p", "t");
    launch.read_only = true;
    let spec = k.headless_spec(&launch, PathBuf::from("k")).unwrap();
    assert!(spec.read_only_enforced);
    assert_eq!(spec.args, ["run", "--format", "json", "--agent", "plan", "--", "t"]);
}

#[test]
fn headless_parsing_forwards_to_output_parser() {
    let k = kilo();
    let out = concat!(
        "{\"type\":\"step_start\",\"sessionID\":\"ses_abc\"}\n",
        "{\"type\":\"text\",\"part\":{\"text\":\"the answer\"}}\n",
        "{\"type\":\"step_finish\",\"part\":{\"reason\":\"stop\"}}\n",
    );
    assert_eq!(k.extract_result(out, 0).as_deref(), Some("the answer"));
    assert_eq!(k.extract_result(out, 1), None);
    assert_eq!(k.extract_provider_session(out).as_deref(), Some("ses_abc"));
    assert!(k.headless_result_ready(out));
    assert!(!k.headless_result_ready("{\"type\":\"step_start\"}\n"));
}

#[test]
fn rejected_warning_is_interned() {
    let text = KILO.replace(
        "[model]",
        "[headless.output.rejected]\nwhere = { type = \"rejected\" }\nwarning_code = \"TOOL_REJECTED\"\n\n[model]",
    );
    let k = profile(&text);
    assert_eq!(k.rejected_call_warning("{\"type\":\"rejected\"}\n"), Some("TOOL_REJECTED"));
    assert_eq!(k.rejected_call_warning("{\"type\":\"text\"}\n"), None);
}

#[test]
fn tui_methods_default_when_tui_absent() {
    let text = KILO.split("[tui]").next().unwrap().to_string()
        + "[context]\nsmartness = 6\nstrengths = [\"implementation\", \"debugging\"]\n";
    let k = profile(&text);
    assert!(k.spawn_command(&crate::tui::types::SpawnRequest {
        cwd: "p",
        extra_args: &[],
        cols: 80,
        rows: 24,
        read_only: false
    })
    .is_none());
    assert_eq!(k.busy_marker("esc interrupt"), None);
    assert!(!k.looks_like_menu("1. yes 2. no"));
    assert!(!k.is_chrome_line("ctrl+x"));
    assert_eq!(k.strip_dialogs("abc"), "abc");
    assert_eq!(k.standby_support(), StandbySupport::default());
}

#[test]
fn headless_absent_is_unsupported() {
    let m = profile(MINIMAL);
    let err = m.headless_command(&HeadlessLaunch::new("p", "t")).unwrap_err();
    assert!(err.contains("unsupported"), "{err}");
    assert!(m.extract_result("x", 0).is_none());
}

#[test]
fn stdin_task_is_not_in_argv_and_enables_headless_standby() {
    let text = KILO
        .replace("task_separator = \"--\"", "[headless.stdin]\ntemplate = \"{task}\"");
    let text = format!("{text}\n[standby]\nheadless = true\n");
    let k = profile(&text);
    let launch = HeadlessLaunch::new("p", "secret task");
    assert!(!spec_args(&k, &launch).iter().any(|a| a.contains("secret")));
    assert!(k.standby_support().headless);
    assert_eq!(k.headless_standby_input("secret task").as_deref(), Some("secret task"));
}

#[test]
fn is_installed_follows_resolver() {
    let missing = KILO.replace("names = [\"kilo\", \"kilocode\"]", "names = [\"definitely_not_installed_xyz\"]");
    assert!(!profile(&missing).is_installed());
    let there = KILO.replace("names = [\"kilo\", \"kilocode\"]", "names = [\"cmd\", \"sh\"]");
    assert!(profile(&there).is_installed());
}

#[test]
fn headless_command_reports_missing_binary_code() {
    let missing = KILO.replace("names = [\"kilo\", \"kilocode\"]", "names = [\"definitely_not_installed_xyz\"]");
    let err = profile(&missing).headless_command(&HeadlessLaunch::new("p", "t")).unwrap_err();
    assert!(err.starts_with("AGENT_NOT_INSTALLED"), "{err}");
}

#[test]
fn context_maps_definition() {
    let c = kilo().context().unwrap();
    assert_eq!(c.agent_type.as_str(), "kilo");
    assert_eq!(c.label, "Kilo Code");
    assert_eq!(c.smartness, 6);
    assert_eq!(c.strengths.len(), 2);
    assert_eq!(c.context_notes.len(), 1);
}
