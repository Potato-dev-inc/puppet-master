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
    // KILO has a [tui] table but no [tui.ready]: visible standby on the experimental path.
    assert_eq!(
        k.standby_support(),
        StandbySupport { visible: true, headless: false, experimental: true }
    );

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
    // No [tui] table: no pane to pre-launch, so no visible standby.
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

#[test]
fn failed_turns_are_classified_by_the_definition() {
    let k = kilo();
    let error = r#"{"type":"error","sessionID":"s","error":{"name":"APIError","data":{"message":"No endpoints found"}}}"#;
    assert_eq!(
        k.classify_headless_failure(error, 0),
        Some(("AGENT_REPORTED_ERROR", "No endpoints found".to_string()))
    );
    // a reported error wins whatever the exit code says (kilo exits 0, or never)
    assert_eq!(k.classify_headless_failure(error, 1).map(|c| c.0), Some("AGENT_REPORTED_ERROR"));
    // exit 0 and nothing readable: the run fails instead of guessing
    assert_eq!(
        k.classify_headless_failure("not json at all\n", 0).map(|c| c.0),
        Some("AGENT_OUTPUT_UNPARSEABLE")
    );
    // a non-zero exit with no reported error keeps the generic WORKER_EXIT_FAILED path
    assert_eq!(k.classify_headless_failure("not json at all\n", 2), None);
    // a profile with no parser never classifies
    assert_eq!(profile(MINIMAL).classify_headless_failure(error, 0), None);
}

#[test]
fn reprints_on_resize_defaults_false_and_follows_the_definition() {
    assert!(!kilo().reprints_on_resize());
    let text = KILO.replace("dispatchable = true", "dispatchable = true\nreprints_on_resize = true");
    assert!(profile(&text).reprints_on_resize());
}

#[test]
fn only_claude_reprints_on_resize_among_the_code_profiles() {
    for (id, expected) in [("claude", true), ("codex", false), ("powershell", false), ("cmd", false), ("opencode", false)] {
        assert_eq!(crate::tui::profile_for(id).reprints_on_resize(), expected, "{id}");
    }
}

#[test]
fn install_and_login_hints_come_from_the_binary_table() {
    let text = KILO.replace(
        "install_hint = \"npm install -g @kilocode/cli\"",
        "install_hint = \"npm install -g @kilocode/cli\"\nlogin_hint = \"  Open Kilo and type /connect  \"",
    );
    let k = profile(&text);
    assert_eq!(k.install_hint().as_deref(), Some("npm install -g @kilocode/cli"));
    assert_eq!(k.login_hint().as_deref(), Some("Open Kilo and type /connect"));
    // no login_hint in the file: None, not an empty string
    assert_eq!(profile(KILO).login_hint(), None);
}

#[test]
fn version_command_uses_the_declared_version_args() {
    let there = KILO
        .replace("names = [\"kilo\", \"kilocode\"]", "names = [\"cmd\", \"sh\"]")
        .replace("version_args = [\"--version\"]", "version_args = [\"-v\", \"--x\"]");
    let command = profile(&there).version_command().expect("resolves");
    assert_eq!(command.args, vec!["-v".to_string(), "--x".to_string()]);
    let missing = KILO.replace("names = [\"kilo\", \"kilocode\"]", "names = [\"definitely_not_installed_xyz\"]");
    assert!(profile(&missing).version_command().is_none());
}

fn kilo_with_env_and_accounts() -> DeclarativeProfile {
    let text = KILO.replace(
        "[headless]\n",
        "[env]\nset = { PM_T = \"from-toml\" }\nunset = [\"PM_DROP\"]\n\n[accounts]\nhome_env = \"KILO_HOME\"\nextra_env = { KILO_X = \"1\" }\nlogin_hint = \"Run kilo and log in.\"\n\n[headless]\n",
    );
    profile(&text)
}

/// The `[env]` table reaches every launch the profile builds. Before the env channel it was parsed
/// and fingerprinted but dropped here, so the child never saw it.
#[test]
fn toml_env_is_carried_by_the_headless_and_pane_launch_specs() {
    let p = kilo_with_env_and_accounts();
    let headless = p.headless_spec(&HeadlessLaunch::new("C:/proj", "do it"), PathBuf::from("kilo-bin")).unwrap();
    assert_eq!(headless.env, vec![("PM_T".to_string(), "from-toml".to_string())]);
    assert_eq!(headless.env_remove, vec!["PM_DROP".to_string()]);
    let tui = p.definition().tui.clone().expect("kilo has a [tui] table");
    let pane = p.tui_spec(
        &tui,
        &crate::tui::types::SpawnRequest { cwd: ".", extra_args: &[], cols: 80, rows: 24, read_only: false },
        PathBuf::from("kilo-bin"),
    );
    assert_eq!(pane.env, headless.env);
    assert_eq!(pane.env_remove, headless.env_remove);
    // a definition without [env] adds nothing
    let plain = kilo().headless_spec(&HeadlessLaunch::new("p", "t"), PathBuf::from("kilo-bin")).unwrap();
    assert!(plain.env.is_empty() && plain.env_remove.is_empty());
}

#[test]
fn accounts_table_becomes_the_account_scheme() {
    let scheme = kilo_with_env_and_accounts().account_scheme().expect("declared");
    assert_eq!(scheme.home_env, "KILO_HOME");
    assert_eq!(scheme.extra_env, vec![("KILO_X".to_string(), "1".to_string())]);
    assert_eq!(scheme.login_hint, "Run kilo and log in.");
    assert!(kilo().account_scheme().is_none(), "no [accounts] table, no accounts");
}

#[test]
fn accounts_table_is_validated_and_fingerprinted() {
    use crate::tui::declarative::{fingerprint, validate, Source, ValidationContext};
    let ctx = ValidationContext { source: Source::User, code_ids: &["claude", "codex", "shell"], raw_len: 1000 };
    let bad = KILO.replace("[headless]\n", "[accounts]\nhome_env = \"LD_PRELOAD\"\n\n[headless]\n");
    let def = parse_definition(&bad).unwrap();
    let errors = validate(&def, &ctx);
    assert!(errors.iter().any(|e| e.code == "AGENT_DEF_DENIED_ENV" && e.path == "accounts.home_env"), "{errors:?}");
    let worse = KILO.replace("[headless]\n", "[accounts]\nhome_env = \"A B\"\n\n[headless]\n");
    let def = parse_definition(&worse).unwrap();
    assert!(validate(&def, &ctx)
        .iter()
        .any(|e| e.path == "accounts.home_env"));
    // a changed [accounts] changes the fingerprint (it changes what the agent is launched with)
    let a = parse_definition(&KILO.replace("[headless]\n", "[accounts]\nhome_env = \"A_HOME\"\n\n[headless]\n")).unwrap();
    let b = parse_definition(&KILO.replace("[headless]\n", "[accounts]\nhome_env = \"B_HOME\"\n\n[headless]\n")).unwrap();
    let file = std::path::Path::new("kilo.toml");
    let src = Source::User;
    assert_ne!(fingerprint::fingerprint(&a, src, None, file), fingerprint::fingerprint(&b, src, None, file));
}

#[test]
fn checkpoints_default_on_and_follow_the_definition() {
    assert!(kilo().checkpoints());
    let text = KILO.replace("dispatchable = true", "dispatchable = true\ncheckpoints = false");
    assert!(!profile(&text).checkpoints());
}

#[test]
fn tested_versions_come_from_the_profile_table_and_default_to_no_pin() {
    assert!(kilo().tested_versions().is_none(), "no [profile] table: no pin");
    let pinned = profile(&format!("{KILO}\n[profile]\ntested = \">=0.150, <0.170\"\n"));
    let range = pinned.tested_versions().expect("a declared range");
    assert_eq!(range.as_str(), ">=0.150, <0.170");
    assert!(range.contains(&semver::Version::parse("0.159.3").unwrap()));
    assert!(!range.contains(&semver::Version::parse("0.170.0").unwrap()));
}

#[test]
fn install_windows_table_becomes_the_profiles_install_spec() {
    assert_eq!(profile(KILO).install_spec(), None, "no [install.windows]: the doctor can only show the hint");
    let text = format!(
        "{KILO}\n[install.windows]\nsource = \"https://kilo.example/docs\"\nnpm = \"@kilocode/cli\"\nnote = \"Needs Node.js.\"\n"
    );
    let spec = profile(&text).install_spec().expect("spec");
    assert_eq!(spec.npm.as_deref(), Some("@kilocode/cli"));
    assert_eq!(spec.ordered_methods(), ["npm"]);
    assert_eq!(spec.source, "https://kilo.example/docs");
}
