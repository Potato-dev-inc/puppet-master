//! Benchmark harness (SPEC 10.1): score a draft definition against the recorded headless fixtures and
//! the hand-written definition of a known agent (`kilo`, `kiro`). The scorer runs in the normal test
//! suite against the references themselves (zero tokens); the `#[ignore]` tests are for the lead to
//! run by hand against the real programs. See the SPEC for the commands.

use std::path::{Path, PathBuf};

use crate::tui::conformance_rules as rules;
use crate::tui::declarative::check::check_dir;
use crate::tui::declarative::schema::{parse_definition, Definition};
use crate::tui::declarative::template;
use crate::tui::declarative::validate::{validate, Source, ValidationContext};
use crate::tui::declarative::wizard::session::Wizard;
use crate::tui::declarative::DeclarativeProfile;
use crate::tui::types::HeadlessLaunch;

const CODE_IDS: &[&str] = &["claude", "codex", "opencode", "opencode_native", "cursor", "cursor_agent", "cmd", "powershell", "bash"];

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The headless captures of a reference agent and its hand-written definition.
pub struct Reference {
    pub name: String,
    pub definition: Definition,
    pub fixtures: PathBuf,
}

pub fn reference(name: &str) -> Reference {
    assert!(matches!(name, "kilo" | "kiro"), "the references are kilo and kiro, not {name:?}");
    let text = std::fs::read_to_string(manifest().join("tests/agent-definitions").join(format!("{name}.toml"))).unwrap();
    Reference {
        name: name.to_string(),
        definition: parse_definition(&text).unwrap(),
        fixtures: manifest().join("src/tui/declarative/fixtures").join(name),
    }
}

/// Copy the `headless*` captures and their expectation files (the draft is headless only, so the
/// screen captures of Kilo do not apply) into `to`.
pub fn headless_fixtures(from: &Path, to: &Path) -> usize {
    std::fs::create_dir_all(to).unwrap();
    let mut captures = 0;
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("headless") {
            std::fs::copy(entry.path(), to.join(&name)).unwrap();
            if !name.ends_with(".expect.toml") {
                captures += 1;
            }
        }
    }
    captures
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArgvRow {
    pub launch: &'static str,
    pub draft: Vec<String>,
    pub reference: Vec<String>,
}

impl ArgvRow {
    pub fn same(&self) -> bool {
        self.draft == self.reference
    }
}

#[derive(Debug)]
pub struct Score {
    pub problems: Vec<String>,
    pub fixtures: Vec<(String, bool, Vec<String>)>,
    pub argv: Vec<ArgvRow>,
}

impl Score {
    pub fn fixtures_pass(&self) -> bool {
        !self.fixtures.is_empty() && self.fixtures.iter().all(|(_, passed, _)| *passed)
    }

    pub fn argv_same(&self) -> bool {
        self.argv.iter().all(ArgvRow::same)
    }

    pub fn table(&self, reference: &str) -> String {
        let mut out = format!("draft scored against {reference}\n");
        out.push_str(&format!("  definition problems: {}\n", if self.problems.is_empty() { "none".into() } else { self.problems.join("; ") }));
        for (name, passed, failures) in &self.fixtures {
            out.push_str(&format!("  fixture {name:32} {}{}\n", if *passed { "PASS" } else { "FAIL" }, if failures.is_empty() { String::new() } else { format!("  {}", failures.join("; ")) }));
        }
        for row in &self.argv {
            out.push_str(&format!("  argv {:14} {}\n", row.launch, if row.same() { "same as the reference".to_string() } else { format!("draft {:?} vs reference {:?}", row.draft, row.reference) }));
        }
        out
    }
}

fn launches() -> [(&'static str, Option<&'static str>, Option<&'static str>); 4] {
    [("plain", None, None), ("with model", Some("provider/model-x"), None), ("with resume", None, Some("ses_123")), ("model+resume", Some("provider/model-x"), Some("ses_123"))]
}

fn argv(def: &Definition, model: Option<&str>, resume: Option<&str>) -> Vec<String> {
    let launch = HeadlessLaunch {
        project: "<project>",
        task: "the task",
        read_only: false,
        resume_session: resume,
        trust_workspace: false,
        allow_shell: false,
        model,
    };
    template::render_headless(def, &launch).unwrap_or_default()
}

/// Validate and run the shared rules on `draft_text`, replay the reference's headless fixtures through
/// it, and compare the commands it renders with the reference's.
pub fn score(draft_text: &str, reference: &Reference, scratch: &Path) -> Score {
    let mut score = Score { problems: Vec::new(), fixtures: Vec::new(), argv: Vec::new() };
    let def = match parse_definition(draft_text) {
        Ok(def) => def,
        Err(e) => {
            score.problems.push(e.to_string());
            return score;
        }
    };
    let ctx = ValidationContext { source: Source::User, code_ids: CODE_IDS, raw_len: draft_text.len() };
    score.problems.extend(validate(&def, &ctx).iter().map(|e| e.to_string()));
    for (name, model, resume) in launches() {
        score.argv.push(ArgvRow { launch: name, draft: argv(&def, model, resume), reference: argv(&reference.definition, model, resume) });
    }
    let Ok(profile) = DeclarativeProfile::new(def) else {
        score.problems.push("the draft does not build".into());
        return score;
    };
    for (name, rule) in rules::ALL {
        if let Err(e) = rule(&profile) {
            score.problems.push(format!("rule {name}: {e}"));
        }
    }
    let dir = scratch.join("headless-fixtures");
    let _ = std::fs::remove_dir_all(&dir);
    headless_fixtures(&reference.fixtures, &dir);
    for r in check_dir(&profile, &dir) {
        score.fixtures.push((r.fixture, r.passed, r.failures));
    }
    score
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pm-wizard-bench-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

// ---- zero-token tests: the harness itself ----------------------------------------------------

fn reference_text(name: &str) -> String {
    std::fs::read_to_string(manifest().join("tests/agent-definitions").join(format!("{name}.toml"))).unwrap()
}

#[test]
fn the_references_score_perfectly_against_their_own_headless_fixtures() {
    for name in ["kilo", "kiro"] {
        let reference = reference(name);
        let dir = scratch(name);
        let score = score(&reference_text(name), &reference, &dir);
        assert!(score.problems.is_empty(), "{name}: {:?}", score.problems);
        assert!(score.fixtures_pass(), "{}", score.table(name));
        assert!(score.argv_same(), "{}", score.table(name));
        let _ = std::fs::remove_dir_all(&dir);
    }
    // kilo has five headless captures, kiro one (the screen captures are not part of a headless draft)
    let dir = scratch("count");
    assert_eq!(headless_fixtures(&reference("kilo").fixtures, &dir.join("k")), 5);
    assert_eq!(headless_fixtures(&reference("kiro").fixtures, &dir.join("r")), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_wrong_draft_is_caught_by_the_fixtures_and_the_argv_comparison() {
    // Kiro's definition does not read Kilo's event stream and renders another command.
    let dir = scratch("wrong");
    let score = score(&reference_text("kiro"), &reference("kilo"), &dir);
    assert!(!score.fixtures_pass(), "{}", score.table("kilo"));
    assert!(!score.argv_same());
    assert!(score.table("kilo").contains("FAIL"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_draft_that_does_not_parse_scores_as_a_problem_not_a_panic() {
    let dir = scratch("broken");
    let score = score("this is not toml [", &reference("kiro"), &dir);
    assert!(!score.problems.is_empty());
    assert!(!score.fixtures_pass());
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- by hand, against the real programs (see SPEC 10.1) --------------------------------------

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("set {name}"))
}

/// `PM_WIZARD_DRAFT=<draft.toml> PM_WIZARD_REFERENCE=kilo|kiro`: no agent is started.
#[test]
#[ignore = "run by hand with PM_WIZARD_DRAFT and PM_WIZARD_REFERENCE (see SPEC 10.1)"]
fn score_draft() {
    let draft = std::fs::read_to_string(env_var("PM_WIZARD_DRAFT")).expect("read the draft");
    let name = env_var("PM_WIZARD_REFERENCE");
    let reference = reference(&name);
    let dir = scratch("score");
    let score = score(&draft, &reference, &dir);
    println!("{}", score.table(&name));
    let _ = std::fs::remove_dir_all(&dir);
    assert!(score.problems.is_empty(), "the draft has problems: {:?}", score.problems);
    assert!(score.fixtures_pass(), "a recorded headless fixture does not pass");
}

/// A wizard on the real PATH that writes only into a temp agents folder.
fn real_wizard(out: &Path) -> Wizard {
    let agents = out.join("agents");
    Wizard::new(move || {
        let mut env = crate::tui::declarative::wizard::WizardEnv::app();
        env.agents_dir = agents.clone();
        env
    })
}

/// `PM_WIZARD_COMMAND=kilo PM_WIZARD_OUT=<dir>`: resolve and probe the real program; writes
/// `probe.json` (what the drafter would see). Starts only `--version`, `--help`, `-h`, `help`.
#[test]
#[ignore = "run by hand with PM_WIZARD_COMMAND and PM_WIZARD_OUT (see SPEC 10.1)"]
fn probe_dump() {
    let command = env_var("PM_WIZARD_COMMAND");
    let out = PathBuf::from(env_var("PM_WIZARD_OUT"));
    std::fs::create_dir_all(&out).unwrap();
    let wizard = real_wizard(&out);
    let started = super::support::ok(&wizard, "start", serde_json::json!({"command": command}));
    let id = started["session"].as_str().unwrap().to_string();
    let probe = super::support::ok(&wizard, "probe", serde_json::json!({"session": id}));
    let both = serde_json::json!({"start": started, "probe": probe});
    std::fs::write(out.join("probe.json"), serde_json::to_string_pretty(&both).unwrap()).unwrap();
    println!("{}", serde_json::to_string_pretty(&both).unwrap());
    super::support::ok(&wizard, "cancel", serde_json::json!({"session": id}));
}

/// `PM_WIZARD_COMMAND=kilo PM_WIZARD_DRAFT=<draft.toml> [PM_WIZARD_OUT=<dir>]`: verify the draft and run
/// the live test against the real program. SPENDS about 3 small messages. Nothing is written to the
/// real agents folder (the files go to `<out>/agents` only if you also set `PM_WIZARD_FINISH=1`).
#[test]
#[ignore = "spends real agent messages; run by hand (see SPEC 10.1)"]
fn livetest_draft() {
    let command = env_var("PM_WIZARD_COMMAND");
    let draft = std::fs::read_to_string(env_var("PM_WIZARD_DRAFT")).expect("read the draft");
    let out = std::env::var("PM_WIZARD_OUT").map(PathBuf::from).unwrap_or_else(|_| scratch("live"));
    std::fs::create_dir_all(&out).unwrap();
    let wizard = real_wizard(&out);
    let id = super::support::start(&wizard, &command);
    super::support::ok(&wizard, "probe", serde_json::json!({"session": id}));
    let verify = super::support::ok(&wizard, "verify", serde_json::json!({"session": id, "toml": draft}));
    println!("verify ok = {}\n{}", verify["ok"], verify["text"].as_str().unwrap_or(""));
    assert_eq!(verify["ok"], true, "the draft does not verify");
    let live = super::support::ok(&wizard, "livetest", super::support::live_body(&wizard, &id));
    for stage in live["stages"].as_array().unwrap() {
        println!("stage {:10} {:8} result={} session={} error={}", stage["name"].as_str().unwrap(), stage["status"].as_str().unwrap(), stage["result"], stage["session"], stage["error_code"]);
    }
    println!("{}", live["text"].as_str().unwrap_or(""));
    if std::env::var("PM_WIZARD_FINISH").is_ok() && live["ok"] == true {
        let done = super::support::ok(&wizard, "finish", serde_json::json!({"session": id, "provenance": super::support::provenance()}));
        println!("{}", serde_json::to_string_pretty(&done).unwrap());
    } else {
        super::support::ok(&wizard, "cancel", serde_json::json!({"session": id}));
    }
    assert_eq!(live["ok"], true, "the live test did not pass");
}
