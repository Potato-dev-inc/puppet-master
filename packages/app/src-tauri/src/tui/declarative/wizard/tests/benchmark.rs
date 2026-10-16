//! Benchmark harness (SPEC 10.1 and 11.13): score a draft definition against the recorded fixtures (the
//! headless captures, and Kilo's screen captures for a draft with `[tui]`) and the hand-written
//! definition of a known agent (`kilo`, `kiro`). The scorer runs in the normal test
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

/// Copy the `headless*` captures and their expectation files (a headless draft is not held against the
/// screen captures of Kilo) into `to`.
pub fn headless_fixtures(from: &Path, to: &Path) -> usize {
    copy_fixtures(from, to, "headless")
}

/// Copy the `tui*` screen captures and their expectation files into `to` (for a draft with `[tui]`).
pub fn screen_fixtures(from: &Path, to: &Path) -> usize {
    copy_fixtures(from, to, "tui")
}

fn copy_fixtures(from: &Path, to: &Path, prefix: &str) -> usize {
    std::fs::create_dir_all(to).unwrap();
    let mut captures = 0;
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(prefix) {
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
    let has_headless = def.headless.as_ref().is_some_and(|h| h.output.is_some());
    let has_tui = def.tui.is_some();
    if !has_headless && !has_tui {
        score.problems.push("the draft has neither a [headless] table nor a [tui] table".into());
    }
    if has_headless {
        for (name, model, resume) in launches() {
            score.argv.push(ArgvRow { launch: name, draft: argv(&def, model, resume), reference: argv(&reference.definition, model, resume) });
        }
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
    if has_headless {
        let dir = scratch.join("headless-fixtures");
        let _ = std::fs::remove_dir_all(&dir);
        headless_fixtures(&reference.fixtures, &dir);
        for r in check_dir(&profile, &dir) {
            score.fixtures.push((r.fixture, r.passed, r.failures));
        }
    }
    if has_tui {
        let dir = scratch.join("screen-fixtures");
        let _ = std::fs::remove_dir_all(&dir);
        if screen_fixtures(&reference.fixtures, &dir) > 0 {
            for r in check_dir(&profile, &dir) {
                score.fixtures.push((r.fixture, r.passed, r.failures));
            }
        }
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
    assert!(score.fixtures_pass(), "a recorded fixture does not pass");
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

/// The agent error of the round before `round` (a folder named `round-N`), if it had one.
fn previous_agent_error(round: &Path) -> Option<String> {
    let n: usize = round.file_name()?.to_str()?.strip_prefix("round-")?.parse().ok()?;
    let previous = round.parent()?.join(format!("round-{}", n.checked_sub(1)?)).join("agent_error.txt");
    std::fs::read_to_string(previous).ok().map(|t| t.trim().to_string()).filter(|t| !t.is_empty())
}

/// One round of the headless benchmark loop (SPEC "Benchmark"): verify `draft` for `command` and, if it
/// passes, run the live test. Writes `verify.txt`, `live.json`, `samples.json`, `feedback.json` and
/// `result.txt` (`PASS`, `FAIL verify` or `FAIL livetest`) into `round`, for the next round's prompt.
/// Returns the result line.
pub fn run_round(wizard: &Wizard, command: &str, round: &Path, draft: &str, reference_name: Option<&str>) -> (String, crate::tui::declarative::wizard::session::TestWords) {
    let write = |name: &str, text: &str| std::fs::write(round.join(name), text).unwrap_or_else(|e| panic!("{name}: {e}"));
    let id = super::support::start(wizard, command);
    super::support::ok(wizard, "probe", serde_json::json!({"session": id}));
    if let Some(name) = reference_name {
        let dir = scratch("bench-score");
        write("score.txt", &score(draft, &reference(name), &dir).table(name));
        let _ = std::fs::remove_dir_all(&dir);
    }
    let verify = super::support::ok(wizard, "verify", serde_json::json!({"session": id, "toml": draft}));
    let mut samples = serde_json::json!([]);
    // A draft that fails only the shared rules is still tested live: the real output goes into the
    // feedback together with those problems. Only a draft that does not validate skips the live test.
    let (result, text, live_json) = if verify["ok"] != true && verify.get("planned").is_none() {
        ("FAIL verify", verify["text"].as_str().unwrap_or("").to_string(), serde_json::Value::Null)
    } else {
        let live = super::support::ok(wizard, "livetest", super::support::live_body(wizard, &id));
        samples = serde_json::Value::Array(
            live["stages"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|s| s["status"] != "skipped")
                .map(|s| {
                    serde_json::json!({
                        "stage": s["name"], "args": s["args"], "exit": s["exit"], "stdout": s["stdout"], "stderr": s["stderr"],
                        "result": s["result"], "session": s["session"], "error_code": s["error_code"],
                    })
                })
                .collect(),
        );
        let agent_error = live["agent_error"]["message"].as_str().map(str::to_string);
        // The program's own error ends the loop: a new draft cannot fix its sign-in, model or provider.
        let repeated = agent_error.as_deref().is_some_and(|m| Some(m) == previous_agent_error(round).as_deref());
        if let Some(message) = &agent_error {
            write("agent_error.txt", message);
        }
        if live["ok"] == true {
            ("PASS", String::new(), live)
        } else if live["agent_error"]["external"] == true || repeated {
            let message = agent_error.unwrap_or_default();
            ("STOP agent-error", format!("{command} itself returned an error: {message}. This is a problem with {command}'s own setup (sign-in, model or provider), not with the profile. Fix it in {command}, then run this round again."), live)
        } else {
            // the live reply's text already holds the shared-rule and fixture problems too
            ("FAIL livetest", live["text"].as_str().unwrap_or("").to_string(), live)
        }
    };
    write("verify.txt", &format!("{}\n{}", if verify["ok"] == true { "OK" } else { "NOT OK" }, verify["text"].as_str().unwrap_or("")));
    write("live.json", &serde_json::to_string_pretty(&live_json).unwrap());
    write("samples.json", &serde_json::to_string_pretty(&samples).unwrap());
    write("feedback.json", &serde_json::to_string_pretty(&serde_json::json!({"previous_toml": draft, "text": text, "samples": samples})).unwrap());
    write("result.txt", &format!("{result}\n"));
    let words = wizard.read(&id, |s| s.words.clone()).unwrap();
    super::support::ok(wizard, "cancel", serde_json::json!({"session": id}));
    (result.to_string(), words)
}

/// `PM_WIZARD_COMMAND=kilo` and `PM_WIZARD_ROUND_DIR=<dir>\round-N`, with the drafter's `draft.toml` in
/// that folder. Runs `run_round` against the real program (the live test SPENDS about 4 small messages).
/// With `PM_WIZARD_REFERENCE=kilo|kiro` it also writes `score.txt`. A failing draft is a normal result,
/// not a test failure: the loop goes on.
#[test]
#[ignore = "spends real agent messages; run by hand (see SPEC Benchmark)"]
fn bench_round() {
    let command = env_var("PM_WIZARD_COMMAND");
    let round = PathBuf::from(env_var("PM_WIZARD_ROUND_DIR"));
    let draft_path = round.join("draft.toml");
    let draft = std::fs::read_to_string(&draft_path).unwrap_or_else(|e| panic!("{}: {e} (run the extract step first)", draft_path.display()));
    let wizard = real_wizard(&round);
    let reference = std::env::var("PM_WIZARD_REFERENCE").ok();
    let (result, _) = run_round(&wizard, &command, &round, &draft, reference.as_deref());
    println!("{result}");
    println!("{}", std::fs::read_to_string(round.join("feedback.json")).map(|t| serde_json::from_str::<serde_json::Value>(&t).unwrap()["text"].as_str().unwrap_or("").to_string()).unwrap_or_default());
}

// ---- the round writer, against the fake CLI (zero tokens) -----------------------------------

#[test]
fn a_passing_round_writes_pass_and_empty_feedback() {
    let tree = super::support::Tree::new("bench-pass");
    let wizard = tree.wizard();
    let round = tree.root.join("round-1");
    std::fs::create_dir_all(&round).unwrap();
    assert_eq!(run_round(&wizard, "fake-agent", &round, super::support::GOOD, None).0, "PASS");
    assert_eq!(std::fs::read_to_string(round.join("result.txt")).unwrap(), "PASS\n");
    let feedback: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(round.join("feedback.json")).unwrap()).unwrap();
    assert_eq!(feedback["text"], "");
    assert_eq!(feedback["previous_toml"], super::support::GOOD);
    assert!(std::fs::read_to_string(round.join("verify.txt")).unwrap().starts_with("OK"));
    assert_eq!(wizard.session_count(), 0);
}

#[test]
fn a_failing_round_writes_what_the_next_prompt_needs_with_the_answers_masked() {
    let tree = super::support::Tree::new("bench-fail");
    let wizard = tree.wizard();
    let round = tree.root.join("round-1");
    std::fs::create_dir_all(&round).unwrap();
    let (result, words) = run_round(&wizard, "fake-agent", &round, super::support::WRONG_PATH, None);
    assert_eq!(result, "FAIL livetest");
    let feedback: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(round.join("feedback.json")).unwrap()).unwrap();
    assert!(feedback["text"].as_str().unwrap().starts_with("The live test ran your file"));
    let samples = feedback["samples"].as_array().unwrap();
    assert_eq!(samples.len(), 1);
    assert_eq!(samples[0]["stage"], "pong");
    assert!(samples[0]["stdout"].as_str().unwrap().contains("<word-1>"), "{}", samples[0]);
    let all = feedback.to_string();
    for w in words.all() {
        assert!(!all.contains(w), "the real answer {w} must not reach the next prompt: {all}");
    }

    // a draft that does not verify
    let round2 = tree.root.join("round-2");
    std::fs::create_dir_all(&round2).unwrap();
    assert_eq!(run_round(&wizard, "fake-agent", &round2, super::support::NO_PATH, None).0, "FAIL verify");
    let feedback: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(round2.join("feedback.json")).unwrap()).unwrap();
    assert!(feedback["text"].as_str().unwrap().contains("AGENT_DEF_INCONSISTENT"));
    assert_eq!(feedback["samples"], serde_json::json!([]));
}

#[test]
fn a_round_whose_draft_fails_only_the_shared_rules_still_runs_the_live_test() {
    let tree = super::support::Tree::new("bench-rules-only");
    let wizard = tree.wizard();
    let round = tree.root.join("round-1");
    std::fs::create_dir_all(&round).unwrap();
    let (result, _) = run_round(&wizard, "fake-agent", &round, super::livetest_tests::UNANCHORED, None);
    assert_eq!(result, "FAIL livetest");
    let feedback: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(round.join("feedback.json")).unwrap()).unwrap();
    assert!(feedback["text"].as_str().unwrap().contains("[conformance]"), "{}", feedback["text"]);
    assert!(!feedback["samples"].as_array().unwrap().is_empty(), "the real output is in the feedback");
    assert!(std::fs::read_to_string(round.join("verify.txt")).unwrap().starts_with("NOT OK"));
    assert_ne!(std::fs::read_to_string(round.join("live.json")).unwrap().trim(), "null");
}

fn draft_for(program: &str) -> String {
    super::support::GOOD.replace("fake-agent", program).replace("fake_agent", &program.replace('-', "_"))
}

#[test]
fn a_round_stops_when_the_agent_itself_reports_an_error() {
    let tree = super::support::Tree::new("bench-stop");
    let wizard = tree.wizard();
    let round = tree.root.join("round-1");
    std::fs::create_dir_all(&round).unwrap();
    let (result, _) = run_round(&wizard, "broken-agent", &round, &draft_for("broken-agent"), None);
    assert_eq!(result, "STOP agent-error");
    assert_eq!(std::fs::read_to_string(round.join("result.txt")).unwrap(), "STOP agent-error\n");
    assert!(std::fs::read_to_string(round.join("agent_error.txt")).unwrap().starts_with("No endpoints found"));
    let feedback: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(round.join("feedback.json")).unwrap()).unwrap();
    let text = feedback["text"].as_str().unwrap();
    assert!(text.contains("broken-agent itself returned an error: No endpoints found"), "{text}");
    assert!(text.contains("not with the profile"), "{text}");
}

#[test]
fn the_same_unexplained_error_in_two_rounds_stops_the_loop() {
    let tree = super::support::Tree::new("bench-repeat");
    let wizard = tree.wizard();
    let draft = draft_for("crash-agent");
    let one = tree.root.join("round-1");
    let two = tree.root.join("round-2");
    std::fs::create_dir_all(&one).unwrap();
    std::fs::create_dir_all(&two).unwrap();
    assert_eq!(run_round(&wizard, "crash-agent", &one, &draft, None).0, "FAIL livetest", "nothing says it is the agent's setup yet");
    assert_eq!(run_round(&wizard, "crash-agent", &two, &draft, None).0, "STOP agent-error", "the same error again");
}

#[test]
fn a_normal_failure_does_not_stop_the_loop() {
    let tree = super::support::Tree::new("bench-nostop");
    let wizard = tree.wizard();
    let round = tree.root.join("round-2");
    std::fs::create_dir_all(&round).unwrap();
    assert_eq!(run_round(&wizard, "fake-agent", &round, super::support::WRONG_PATH, None).0, "FAIL livetest");
    assert!(!round.join("agent_error.txt").exists());
}

// ---- the visible half of the scorer (SPEC 11.13) -------------------------------------------------

/// Kilo's file without its headless half: the agent and binary tables, then everything from "Visible mode".
fn kilo_visible_only() -> String {
    let text = reference_text("kilo").replace("\r\n", "\n");
    let head = text.split("[features]").next().unwrap();
    let from = text.find("# Visible mode.").unwrap();
    // up to the [model] table, which reads the model from a headless selector
    let tail = &text[from..from + text[from..].find("[model]").unwrap()];
    format!("{head}\n{tail}")
}

#[test]
fn a_visible_only_draft_is_scored_against_kilos_screen_captures_and_not_its_headless_ones() {
    let dir = scratch("visible-score");
    let draft = kilo_visible_only();
    assert!(!draft.contains("[headless"), "the test draft has no headless table");
    let score = score(&draft, &reference("kilo"), &dir);
    assert!(score.problems.is_empty(), "{:?}", score.problems);
    let names: Vec<&str> = score.fixtures.iter().map(|(n, _, _)| n.as_str()).collect();
    assert_eq!(names, ["tui_idle.txt", "tui_permission.txt", "tui_reply.txt", "tui_working.txt"], "{}", score.table("kilo"));
    assert!(score.fixtures_pass(), "{}", score.table("kilo"));
    assert!(score.argv.is_empty(), "no [headless]: nothing to compare");
    assert!(score.argv_same());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_visible_draft_that_cannot_tell_busy_from_idle_fails_the_working_capture() {
    let dir = scratch("visible-wrong");
    let draft = kilo_visible_only().replace("working_regex = [\"esc\\\\s+interrupt\"]", "working_regex = [\"never on screen\"]");
    assert!(draft.contains("never on screen"), "the edit applied");
    let score = score(&draft, &reference("kilo"), &dir);
    assert!(!score.fixtures_pass(), "{}", score.table("kilo"));
    let (name, _, failures) = score.fixtures.iter().find(|(_, passed, _)| !passed).unwrap();
    assert_eq!(name, "tui_working.txt");
    assert!(failures.iter().any(|f| f.starts_with("busy:")), "{failures:?}");
    assert!(score.table("kilo").contains("FAIL"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_draft_with_both_halves_is_scored_on_both() {
    let dir = scratch("both-score");
    let score = score(&reference_text("kilo"), &reference("kilo"), &dir);
    assert_eq!(score.fixtures.len(), 9, "five headless captures and four screens");
    assert_eq!(score.argv.len(), 4);
    assert!(score.fixtures_pass() && score.argv_same(), "{}", score.table("kilo"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_draft_with_neither_table_is_a_problem() {
    let dir = scratch("neither");
    let draft = "schema_version = 1\n[agent]\nid = \"zz\"\nlabel = \"Z\"\n[binary]\nnames = [\"zz\"]\n";
    let score = score(draft, &reference("kilo"), &dir);
    assert!(score.problems.iter().any(|p| p.contains("neither a [headless] table nor a [tui] table")), "{:?}", score.problems);
    assert!(!score.fixtures_pass());
    let _ = std::fs::remove_dir_all(&dir);
}
