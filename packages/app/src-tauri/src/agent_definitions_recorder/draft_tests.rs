use super::draft::*;
use crate::tui::declarative::loader::load_text;
use crate::tui::declarative::sources::Origin;
use crate::tui::declarative::validate::Source;
use std::path::Path;

const IDLE: &str = include_str!("fixtures/kilo_tui_idle.txt");
const WORKING: &str = include_str!("fixtures/kilo_tui_working.txt");
const PERMISSION: &str = include_str!("fixtures/kilo_tui_permission.txt");
const REPLY: &str = include_str!("fixtures/kilo_tui_reply.txt");
const PROBE: &str = include_str!("fixtures/kilo_probe.jsonl");

fn kilo_input() -> DraftInput {
    DraftInput {
        id: "kilodraft".into(),
        label: "Kilo draft".into(),
        command: "kilo".into(),
        idle: vec![IDLE.into()],
        working: vec![WORKING.into()],
        prompt: vec![PERMISSION.into()],
        reply: vec![REPLY.into()],
        probe: Some(Probe { argv: vec!["run".into(), "--format".into(), "json".into()], output: PROBE.into() }),
    }
}

fn validate(text: &str) -> Vec<String> {
    let f = load_text(Path::new("kilodraft.toml"), text, text.len(), Origin::new(Source::User), None, &[]);
    f.errors.iter().map(|e| format!("{} {} {}", e.code, e.path, e.message)).collect()
}

#[test]
fn kilo_captures_produce_valid_draft() {
    let draft = generate(&kilo_input());
    let errors = validate(&draft.toml);
    assert!(errors.is_empty(), "{errors:?}\n{}", draft.toml);

    let value: toml::Value = draft.toml.parse().unwrap();
    // busy regex matches working, not idle
    let busy = value["tui"]["busy"]["working_regex"].as_array().unwrap();
    assert!(!busy.is_empty());
    for re in busy {
        let re = regex::Regex::new(&format!("(?is){}", re.as_str().unwrap())).unwrap();
        assert!(re.is_match(WORKING), "busy regex must match the working capture");
        assert!(!re.is_match(IDLE), "busy regex must not match the idle capture");
        assert!(!re.is_match(REPLY), "busy regex must not match the finished capture");
    }
    // chrome list and prompt table
    assert!(!value["tui"]["chrome"]["line_regex"].as_array().unwrap().is_empty());
    let prompts = value["tui"]["prompt"].as_array().unwrap();
    let detect = regex::Regex::new(&format!("(?is){}", prompts[0]["detect_regex"].as_str().unwrap())).unwrap();
    assert!(detect.is_match(PERMISSION) && !detect.is_match(WORKING) && !detect.is_match(IDLE));
    // headless skeleton parses the probe: result is the last non-blank text, session found, done found
    let out = &value["headless"]["output"];
    assert_eq!(out["format"].as_str(), Some("json_lines"));
    assert_eq!(out["result"]["path"].as_str(), Some("part.text"));
    assert!(out["session"]["path"].as_str().unwrap().ends_with("sessionID"));
    assert_eq!(out["done"]["where"]["type"].as_str(), Some("step_finish"));
}

#[test]
fn candidates_carry_provenance() {
    let draft = generate(&kilo_input());
    let lines: Vec<&str> = draft.toml.lines().collect();
    // every regex candidate line is preceded (directly, or through other comments/candidates of the
    // same table) by a `# from` comment, and each selector table by one right above its header
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim();
        let candidate = t.starts_with("working_regex")
            || t.starts_with("idle_regex")
            || t.starts_with("detect_regex");
        if candidate {
            assert!(
                lines[..i].iter().rev().take(4).any(|l| l.trim().starts_with("# from")),
                "no provenance above: {t}\n{}",
                draft.toml
            );
        }
    }
    for table in ["[headless.output.result]", "[headless.output.session]", "[headless.output.done]", "[tui.menu]", "[[tui.prompt]]"] {
        let i = lines.iter().position(|l| l.trim() == table).unwrap_or_else(|| panic!("{table} missing\n{}", draft.toml));
        assert!(
            lines[..i].iter().rev().take(3).any(|l| l.trim().starts_with("# from")),
            "{table} has no `# from:` above it\n{}",
            draft.toml
        );
    }
    // every chrome candidate has its own
    let chrome_at = lines.iter().position(|l| l.trim() == "[tui.chrome]").unwrap();
    for (i, l) in lines.iter().enumerate().skip(chrome_at + 2) {
        if l.trim().starts_with('"') {
            assert!(lines[i - 1].trim().starts_with("# from:"), "chrome line without provenance: {l}");
        }
        if l.trim() == "]" {
            break;
        }
    }
}

#[test]
fn draft_holds_no_secrets_or_session_text_from_the_probe_beyond_samples() {
    let draft = generate(&kilo_input());
    assert!(!draft.toml.contains("SECRET_FIXTURE_VALUE"));
}

#[test]
fn write_draft_never_overwrites_and_stays_out_of_the_agents_folder() {
    let dir = std::env::temp_dir().join(format!("pm-draft-test-{}", std::process::id()));
    let draft = generate(&kilo_input());
    let a = write_draft(&dir, &draft).unwrap();
    let b = write_draft(&dir, &draft).unwrap();
    assert_ne!(a, b);
    assert!(a.to_string_lossy().contains("pm-draft-test"));
    assert!(drafts_dir().ends_with("drafts"));
    std::fs::remove_file(a).unwrap();
    std::fs::remove_file(b).unwrap();
    let _ = std::fs::remove_dir(dir);
}

#[test]
fn a_session_without_a_busy_marker_still_yields_notes_not_a_panic() {
    let mut input = kilo_input();
    input.working = vec![IDLE.to_string()];
    input.probe = None;
    let draft = generate(&input);
    assert!(draft.notes.iter().any(|n| n.contains("busy marker")), "{:?}", draft.notes);
    assert!(validate(&draft.toml).is_empty(), "{:?}", validate(&draft.toml));
}

/// Phase 4 gate: the untouched draft already passes 7 of the 9 Kilo fixtures; two human additions (the
/// model footer, which a draft cannot know, and the error selector, which the success probe has no
/// sample of) make all of them pass.
#[test]
fn gate_draft_passes_kilo_fixtures_after_two_human_edits() {
    use crate::tui::declarative::{check, DeclarativeProfile};
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tui/declarative/fixtures/kilo");
    let run = |text: &str| {
        let f = load_text(Path::new("kilodraft.toml"), text, text.len(), Origin::new(Source::User), None, &[]);
        assert!(f.errors.is_empty(), "{:?}", f.errors.iter().map(|e| format!("{} {}", e.code, e.message)).collect::<Vec<_>>());
        let p = DeclarativeProfile::new(f.definition.unwrap()).unwrap();
        check::check_dir(&p, &fixtures)
    };
    let draft = generate(&kilo_input());
    let raw = run(&draft.toml);
    let failed: Vec<&str> = raw.iter().filter(|r| !r.passed).map(|r| r.fixture.as_str()).collect();
    assert_eq!(failed, vec!["headless_error.jsonl", "tui_reply.txt"], "{raw:?}");

    let edited = draft.toml.clone()
        + r#"
[tui.footer_model]
regex = '▣\s+code\s+·\s+[^·\n]+?\s+·\s+(?P<model>[^·\n]+?)\s+·\s+\d'

[headless.output.error]
where = { type = "error" }
path = "error.data.message"
pick = "last"
"#;
    let fixed = run(&edited);
    let still: Vec<_> = fixed.iter().filter(|r| !r.passed).collect();
    assert!(still.is_empty(), "{still:?}");
}

#[test]
fn provenance_samples_are_redacted_again() {
    let mut input = kilo_input();
    input.working = vec!["  esc interrupt   sk-ant-api03-AbCdEf123456 token".into()];
    let ctx = super::redact::RedactContext { env_values: vec![], home: None };
    let draft = generate_with(&input, &ctx);
    assert!(!draft.toml.contains("sk-ant"), "{}", draft.toml);
}
