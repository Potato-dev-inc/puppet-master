use super::{KILO, MINIMAL};
use crate::tui::declarative::schema::{Definition, Kind, OptionalSegment, Pick, Selector};
use crate::tui::declarative::{parse_definition, validate, Source, ValidationContext};

const CODE_IDS: &[&str] = &["claude", "codex", "cursor_agent", "opencode", "shell"];

fn kilo() -> Definition {
    parse_definition(KILO).unwrap()
}

fn minimal() -> Definition {
    parse_definition(MINIMAL).unwrap()
}

fn codes_for(def: &Definition, source: Source, raw_len: usize) -> Vec<(&'static str, String)> {
    let ctx = ValidationContext { source, code_ids: CODE_IDS, raw_len };
    validate(def, &ctx).into_iter().map(|e| (e.code, e.path)).collect()
}

fn codes(def: &Definition) -> Vec<(&'static str, String)> {
    codes_for(def, Source::User, 1000)
}

#[track_caller]
fn has(def: &Definition, code: &str) {
    let found = codes(def);
    assert!(found.iter().any(|(c, _)| *c == code), "expected {code}, got {found:?}");
}

#[track_caller]
fn has_at(def: &Definition, code: &str, path: &str) {
    let found = codes(def);
    assert!(found.iter().any(|(c, p)| *c == code && p == path), "expected {code} at {path}, got {found:?}");
}

fn selector() -> Selector {
    kilo().headless.unwrap().output.unwrap().result.unwrap()
}

#[test]
fn spec_examples_validate_clean() {
    assert_eq!(codes(&kilo()), vec![]);
    assert_eq!(codes(&minimal()), vec![]);
}

#[test]
fn bad_id_pattern() {
    for id in ["Kilo", "k", "1abc", "has-dash", "a_very_long_identifier_over_thirty_two_chars"] {
        let mut d = minimal();
        d.agent.id = id.into();
        has_at(&d, "AGENT_DEF_BAD_ID", "agent.id");
    }
}

#[test]
fn reserved_ids_and_code_profiles() {
    for id in ["unknown", "all", "any", "auto", "default", "none", "shell", "claude", "cursor_agent"] {
        let mut d = minimal();
        d.agent.id = id.into();
        has_at(&d, "AGENT_DEF_RESERVED_ID", "agent.id");
    }
}

#[test]
fn reserved_fields_rejected() {
    let mut d = minimal();
    d.agent.base = Some("claude".into());
    has_at(&d, "AGENT_DEF_FIELD_RESERVED", "agent.base");
    let mut d = minimal();
    d.agent.kind = Kind::NativeProtocol;
    has_at(&d, "AGENT_DEF_FIELD_RESERVED", "agent.kind");
    d.agent.kind = Kind::Shell;
    has_at(&d, "AGENT_DEF_FIELD_RESERVED", "agent.kind");
}

#[test]
fn regex_compile_failure() {
    let mut d = minimal();
    d.tui.as_mut().unwrap().busy.as_mut().unwrap().working_regex = vec!["(unclosed".into()];
    has_at(&d, "AGENT_DEF_BAD_REGEX", "tui.busy.working_regex.0");
    // look-around and backreferences are not supported by the linear-time engine
    d.tui.as_mut().unwrap().busy.as_mut().unwrap().working_regex = vec!["(?=a)b".into()];
    has(&d, "AGENT_DEF_BAD_REGEX");
}

#[test]
fn regex_size_limit() {
    let mut d = minimal();
    d.tui.as_mut().unwrap().busy.as_mut().unwrap().working_regex = vec!["(?:[a-z]{100}){100}".into()];
    has_at(&d, "AGENT_DEF_REGEX_TOO_LARGE", "tui.busy.working_regex.0");
}

#[test]
fn regex_count_limit() {
    let mut d = minimal();
    d.tui.as_mut().unwrap().menu =
        Some(crate::tui::declarative::schema::MenuTable { regex: (0..200).map(|i| format!("m{i}")).collect() });
    // 200 menu + busy + prompt regexes is over the cap of 200
    has_at(&d, "AGENT_DEF_REGEX_TOO_LARGE", "");
}

#[test]
fn unknown_and_misplaced_placeholders() {
    let mut d = kilo();
    d.headless.as_mut().unwrap().argv.push("{nope}".into());
    has_at(&d, "AGENT_DEF_BAD_PLACEHOLDER", "headless.argv.3");
    // {task} is not allowed in the TUI table, nor in optional segments
    let mut d = kilo();
    d.tui.as_mut().unwrap().argv.push("{task}".into());
    has_at(&d, "AGENT_DEF_BAD_PLACEHOLDER", "tui.argv.0");
    let mut d = kilo();
    d.headless.as_mut().unwrap().optional[0].args.push("{task}".into());
    has(&d, "AGENT_DEF_BAD_PLACEHOLDER");
    // placeholders in env values
    let mut d = minimal();
    d.env.set.insert("FOO".into(), "{project}".into());
    has_at(&d, "AGENT_DEF_BAD_PLACEHOLDER", "env.set.FOO");
}

#[test]
fn task_delivery_must_be_exactly_one() {
    let mut d = kilo();
    d.headless.as_mut().unwrap().argv.extend(["{task}".into(), "{task}".into()]);
    has_at(&d, "AGENT_DEF_BAD_PLACEHOLDER", "headless");
    let mut d = kilo();
    let h = d.headless.as_mut().unwrap();
    h.argv.push("{task}".into());
    h.task_flag = Some("--prompt".into());
    has_at(&d, "AGENT_DEF_BAD_PLACEHOLDER", "headless");
    // stdin plus argv placeholder
    let mut d = kilo();
    let h = d.headless.as_mut().unwrap();
    h.argv.push("{task}".into());
    h.stdin = Some(crate::tui::declarative::schema::StdinTable { template: "{task}".into() });
    has_at(&d, "AGENT_DEF_BAD_PLACEHOLDER", "headless");
    // stdin template needs exactly one {task} and nothing else
    let mut d = kilo();
    d.headless.as_mut().unwrap().stdin =
        Some(crate::tui::declarative::schema::StdinTable { template: "{task} {project}".into() });
    has_at(&d, "AGENT_DEF_BAD_PLACEHOLDER", "headless.stdin.template");
}

#[test]
fn when_outside_closed_set() {
    let mut d = kilo();
    d.headless.as_mut().unwrap().optional.push(OptionalSegment { when: "sometimes".into(), args: vec![] });
    has(&d, "AGENT_DEF_BAD_WHEN");
    // tui allows only model, read_only, resume
    let mut d = kilo();
    d.tui.as_mut().unwrap().optional.push(OptionalSegment { when: "allow_shell".into(), args: vec![] });
    has(&d, "AGENT_DEF_BAD_WHEN");
}

#[test]
fn binary_names_must_be_bare() {
    for name in ["bin/kilo", "..\\kilo", "C:kilo", "../kilo", "", "a..b"] {
        let mut d = minimal();
        d.binary.names = vec![name.into()];
        has(&d, "AGENT_DEF_BAD_BINARY");
    }
    let mut d = minimal();
    d.binary.names = vec![];
    has(&d, "AGENT_DEF_BAD_BINARY");
    let mut d = kilo();
    d.binary.shim_native_globs = vec!["../escape/*.exe".into()];
    has(&d, "AGENT_DEF_BAD_BINARY");
    let mut d = kilo();
    d.binary.windows_extensions = vec!["sh".into()];
    has(&d, "AGENT_DEF_BAD_BINARY");
}

#[test]
fn interpreter_denylist() {
    for name in ["bash", "PowerShell.exe", "python3", "node", "env", "cmd.exe"] {
        let mut d = minimal();
        d.binary.names = vec![name.into()];
        has_at(&d, "AGENT_DEF_SHELL_INTERPRETER", "binary.names.0");
    }
    // allowed with the explicit opt-in for a trusted source, never for project/extra
    let mut d = minimal();
    d.binary.names = vec!["python".into()];
    d.agent.allow_interpreter = true;
    assert_eq!(codes_for(&d, Source::User, 1000), vec![]);
    for source in [Source::Project, Source::Extra] {
        let found = codes_for(&d, source, 1000);
        assert!(found.iter().any(|(c, _)| *c == "AGENT_DEF_SHELL_INTERPRETER"), "{found:?}");
    }
}

#[test]
fn env_denylist() {
    for key in ["PATH", "path", "LD_PRELOAD", "DYLD_INSERT_LIBRARIES", "NODE_OPTIONS", "COMSPEC", "SHELL"] {
        let mut d = minimal();
        d.env.set.insert(key.into(), "x".into());
        has(&d, "AGENT_DEF_DENIED_ENV");
    }
    let mut d = minimal();
    d.env.unset = vec!["PATHEXT".into()];
    has_at(&d, "AGENT_DEF_DENIED_ENV", "env.unset.0");
    let mut d = minimal();
    d.env.set.insert("KILO_TELEMETRY".into(), "0".into());
    assert_eq!(codes(&d), vec![]);
}

#[test]
fn size_limits() {
    has_raw(70 * 1024);
    let mut d = minimal();
    d.tui.as_mut().unwrap().argv = (0..65).map(|i| format!("a{i}")).collect();
    has_at(&d, "AGENT_DEF_TOO_LARGE", "tui.argv");
    let mut d = minimal();
    d.tui.as_mut().unwrap().argv = vec!["x".repeat(5000)];
    has_at(&d, "AGENT_DEF_TOO_LARGE", "tui.argv.0");
    let mut d = minimal();
    d.binary.install_hint = "h".repeat(201);
    has_at(&d, "AGENT_DEF_TOO_LARGE", "binary.install_hint");
}

fn has_raw(len: usize) {
    let found = codes_for(&minimal(), Source::User, len);
    assert!(found.iter().any(|(c, _)| *c == "AGENT_DEF_TOO_LARGE"), "{found:?}");
}

#[test]
fn read_only_needs_segment_and_note() {
    let mut d = kilo();
    d.features.read_only_headless = true;
    has_at(&d, "AGENT_DEF_INCONSISTENT", "features.read_only_headless");
    has_at(&d, "AGENT_DEF_INCONSISTENT", "features.read_only_note");
    d.headless.as_mut().unwrap().optional.push(OptionalSegment { when: "read_only".into(), args: vec!["--agent".into(), "plan".into()] });
    d.features.read_only_note = "plan agent denies edits".into();
    assert_eq!(codes(&d), vec![]);
    let mut d = minimal();
    d.features.read_only_spawned_pane = true;
    has_at(&d, "AGENT_DEF_INCONSISTENT", "features.read_only_spawned_pane");
}

#[test]
fn resumable_must_agree_with_definition() {
    let mut d = kilo();
    d.headless.as_mut().unwrap().output.as_mut().unwrap().session = None;
    has_at(&d, "AGENT_DEF_INCONSISTENT", "features.resumable_headless");
    let mut d = kilo();
    d.features.resumable_headless = Some(false);
    has_at(&d, "AGENT_DEF_INCONSISTENT", "features.resumable_headless");
    let mut d = kilo();
    d.features.resumable_headless = None;
    assert_eq!(codes(&d), vec![]);
}

#[test]
fn other_consistency_rules() {
    let mut d = minimal();
    d.features.dispatchable = false;
    has_at(&d, "AGENT_DEF_INCONSISTENT", "features.not_dispatchable_reason");
    let mut d = minimal();
    d.tui = None;
    has_at(&d, "AGENT_DEF_INCONSISTENT", "");
    let mut d = minimal();
    d.run_options.allow_shell = true;
    has_at(&d, "AGENT_DEF_INCONSISTENT", "run_options.allow_shell");
    let mut d = kilo();
    d.headless.as_mut().unwrap().output.as_mut().unwrap().result = None;
    has_at(&d, "AGENT_DEF_INCONSISTENT", "headless.output.result");
    let mut d = kilo();
    let mut sel = selector();
    sel.pick = Pick::MaxBy;
    d.headless.as_mut().unwrap().output.as_mut().unwrap().result = Some(sel);
    has_at(&d, "AGENT_DEF_INCONSISTENT", "headless.output.result.max_by");
    let mut d = minimal();
    d.tui.as_mut().unwrap().prompt[0].choices[0].text = Some("y".into());
    has(&d, "AGENT_DEF_INCONSISTENT");
    let mut d = minimal();
    d.binary.allow_cmd_shim = true;
    has_at(&d, "AGENT_DEF_INCONSISTENT", "binary.allow_cmd_shim");
}

#[test]
fn positional_task_needs_separator() {
    let mut d = kilo();
    d.headless.as_mut().unwrap().task_separator = None;
    has_at(&d, "AGENT_DEF_BAD_PLACEHOLDER", "headless");
    // a flag, an explicit placeholder or stdin delivers the task without a separator
    d.headless.as_mut().unwrap().task_flag = Some("--prompt".into());
    assert_eq!(codes(&d), vec![]);
}

#[test]
fn a_tested_range_must_be_a_version_range() {
    let mut d = minimal();
    d.profile.tested = Some(">=0.150, <0.170".into());
    assert_eq!(codes(&d), vec![]);
    d.profile.tested = Some("whenever it works".into());
    has_at(&d, "AGENT_DEF_INCONSISTENT", "profile.tested");
    d.profile.tested = Some("  ".into());
    has_at(&d, "AGENT_DEF_INCONSISTENT", "profile.tested");
}
