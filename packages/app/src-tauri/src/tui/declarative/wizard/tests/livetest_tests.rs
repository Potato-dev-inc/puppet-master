//! Live test: the three stages, classification, samples and replay.

use std::time::Duration;

use serde_json::{json, Value};

use crate::tui::declarative::wizard::livetest::{judge, stage_args, BAD_MODEL};
use crate::tui::declarative::wizard::session::Stage;
use crate::tui::declarative::DeclarativeProfile;

use super::support::{call, ok, start, tested, verified, Tree, GOOD, MINIMAL, WRONG_PATH};

fn live(wizard: &crate::tui::declarative::wizard::Wizard, id: &str) -> Value {
    ok(wizard, "livetest", json!({"session": id, "confirm": true}))
}

fn stage<'a>(v: &'a Value, name: &str) -> &'a Value {
    v["stages"].as_array().unwrap().iter().find(|s| s["name"] == name).unwrap_or_else(|| panic!("no stage {name}: {v}"))
}

#[test]
fn it_needs_an_explicit_confirm_because_it_uses_the_plan() {
    let tree = Tree::new("l-confirm");
    let wizard = tree.wizard();
    let id = verified(&wizard, "fake-agent", GOOD);
    for body in [json!({"session": id}), json!({"session": id, "confirm": false}), json!({"session": id, "confirm": "yes"})] {
        let err = call(&wizard, "livetest", body).unwrap_err();
        assert_eq!((err.status, err.code), (400, "WIZARD_CONFIRM_REQUIRED"));
    }
    assert_eq!(wizard.read(&id, |s| s.state.as_str()).unwrap(), "verified", "nothing ran");
    assert!(wizard.read(&id, |s| s.samples.is_empty()).unwrap());
}

#[test]
fn all_three_stages_pass_for_a_good_draft() {
    let tree = Tree::new("l-good");
    let wizard = tree.wizard();
    let id = verified(&wizard, "fake-agent", GOOD);
    let t = live(&wizard, &id);
    assert_eq!(t["ok"], true, "{}", t["text"]);
    assert_eq!(t["state"], "tested");
    assert_eq!(t["tested_version"], "1.4.2");
    let pong = stage(&t, "pong");
    assert_eq!(pong["status"], "passed");
    assert_eq!(pong["result"], "pong");
    assert_eq!(pong["session"], "fk-0001");
    assert_eq!(pong["exit"], 0);
    assert_eq!(pong["args"], json!(["run", "--format", "json", "--", "Reply with exactly the word: pong"]));
    let resume = stage(&t, "resume");
    assert_eq!(resume["status"], "passed");
    assert_eq!(resume["result"], "pong2");
    assert_eq!(
        resume["args"],
        json!(["run", "--format", "json", "-s", "fk-0001", "--", "Reply with exactly the word: pong2"]),
        "the resume stage continues the session stage 1 produced"
    );
    let bad = stage(&t, "bad_model");
    assert_eq!(bad["status"], "passed");
    assert_eq!(bad["result"], json!(null));
    assert_eq!(bad["error_code"], "AGENT_REPORTED_ERROR");
    assert_eq!(bad["exit"], 1);
    assert!(bad["args"].to_string().contains(BAD_MODEL));
    assert!(bad["stdout"].as_str().unwrap().contains("Model not found"));
    assert_eq!(
        t["fixtures"],
        json!(["headless_success.jsonl", "headless_resume.jsonl", "headless_error.jsonl", "headless_garbage.txt"])
    );
    assert_eq!(t["errors"], json!([]));
    assert_eq!(wizard.read(&id, |s| s.samples.len()).unwrap(), 3);
}

#[test]
fn a_wrong_error_case_is_judged_by_exit_code_when_there_is_no_error_selector() {
    let tree = Tree::new("l-exitcode");
    let wizard = tree.wizard();
    let draft = GOOD.replace("[headless.output.error]\nwhere = { event = \"error\" }\npath = \"message\"\npick = \"last\"\n", "");
    let id = verified(&wizard, "fake-agent", &draft);
    let t = live(&wizard, &id);
    assert_eq!(t["ok"], true, "{}", t["text"]);
    let bad = stage(&t, "bad_model");
    assert_eq!(bad["status"], "passed");
    assert_eq!(bad["error_code"], json!(null), "the draft cannot name the error, but the exit code 1 says it failed");
}

#[test]
fn stages_that_do_not_apply_are_skipped_with_a_reason() {
    let tree = Tree::new("l-minimal");
    let wizard = tree.wizard();
    let id = verified(&wizard, "fake-agent", MINIMAL);
    let t = live(&wizard, &id);
    assert_eq!(t["ok"], true, "{}", t["text"]);
    assert_eq!(t["state"], "tested");
    assert_eq!(stage(&t, "pong")["status"], "passed");
    let resume = stage(&t, "resume");
    assert_eq!(resume["status"], "skipped");
    assert!(resume["note"].as_str().unwrap().contains("no resume"), "{resume}");
    let bad = stage(&t, "bad_model");
    assert_eq!(bad["status"], "skipped");
    assert!(bad["note"].as_str().unwrap().contains("no model option"), "{bad}");
    assert_eq!(t["fixtures"], json!(["headless_success.jsonl", "headless_garbage.txt"]));
}

#[test]
fn a_selector_that_misses_the_answer_fails_stage_one_and_stops_there() {
    let tree = Tree::new("l-wrong");
    let wizard = tree.wizard();
    let id = verified(&wizard, "fake-agent", WRONG_PATH);
    let t = live(&wizard, &id);
    assert_eq!(t["ok"], false);
    assert_eq!(t["state"], "verified", "a failed live test is not a tested draft");
    assert_eq!(stage(&t, "pong")["status"], "failed");
    assert_eq!(stage(&t, "resume")["status"], "skipped");
    assert!(stage(&t, "resume")["note"].as_str().unwrap().contains("not run"));
    assert_eq!(stage(&t, "bad_model")["status"], "skipped", "no tokens are spent after the first failure");
    let err = &t["errors"][0];
    assert_eq!(err["stage"], "livetest");
    assert_eq!(err["code"], "LIVETEST_PONG_FAILED");
    assert!(err["message"].as_str().unwrap().contains("no result was found"), "{err}");
    assert!(err["excerpt"].as_str().unwrap().contains("\"event\":\"end\""), "the real output is shown: {err}");
    let text = t["text"].as_str().unwrap();
    assert!(text.starts_with("The live test ran your file against the real program and found"), "{text}");
    assert!(text.contains("Real output (first lines):"), "{text}");
    assert_eq!(wizard.read(&id, |s| s.samples.len()).unwrap(), 1, "the real output is kept as a sample");
}

#[test]
fn the_failed_output_is_replayed_for_free_against_the_next_draft() {
    let tree = Tree::new("l-replay");
    let wizard = tree.wizard();
    let id = verified(&wizard, "fake-agent", WRONG_PATH);
    live(&wizard, &id);
    // the same wrong draft is rejected without starting a process
    let again = ok(&wizard, "verify", json!({"session": id, "toml": WRONG_PATH}));
    assert_eq!(again["ok"], false);
    assert_eq!(again["samples_replayed"], 1);
    let e = again["errors"].as_array().unwrap().iter().find(|e| e["stage"] == "fixture").expect("a fixture error");
    assert_eq!(e["code"], "FIXTURE_FAILED");
    assert!(e["path"].as_str().unwrap().starts_with("headless_success"), "{e}");
    assert!(e["excerpt"].as_str().unwrap().contains("\"text\":\"pong\""), "{e}");
    // the fixed draft passes the replay and then the live test
    let fixed = ok(&wizard, "verify", json!({"session": id, "toml": GOOD}));
    assert_eq!(fixed["ok"], true, "{}", fixed["text"]);
    assert_eq!(fixed["samples_replayed"], 1);
    assert_eq!(live(&wizard, &id)["state"], "tested");
}

#[test]
fn an_output_made_by_a_different_command_is_not_replayed() {
    let tree = Tree::new("l-replay-other");
    let wizard = tree.wizard();
    let id = verified(&wizard, "fake-agent", WRONG_PATH);
    live(&wizard, &id);
    // a draft whose command differs (an extra flag) says nothing about the old output
    let other = GOOD.replace("argv = [\"run\", \"--format\", \"json\"]", "argv = [\"run\", \"--format\", \"json\", \"-m\", \"fake/slow\"]");
    let v = ok(&wizard, "verify", json!({"session": id, "toml": other}));
    assert_eq!(v["samples_replayed"], 0);
}

#[test]
fn a_program_that_prints_plain_text_fails_with_the_text_shown() {
    let tree = Tree::new("l-raw");
    let wizard = tree.wizard();
    let id = verified_with_binary(&wizard, "raw-agent");
    let t = live(&wizard, &id);
    assert_eq!(t["ok"], false);
    assert!(t["errors"][0]["excerpt"].as_str().unwrap().contains("Thinking..."));
}

fn verified_with_binary(wizard: &crate::tui::declarative::wizard::Wizard, name: &str) -> String {
    verified(wizard, name, &GOOD.replace("fake-agent", name).replace("fake_agent", &name.replace('-', "_")))
}

#[test]
fn a_program_that_prints_nothing_is_reported() {
    let tree = Tree::new("l-silent");
    let wizard = tree.wizard();
    let id = verified_with_binary(&wizard, "silent-agent");
    let t = live(&wizard, &id);
    assert_eq!(t["ok"], false);
    assert_eq!(t["errors"][0]["code"], "LIVETEST_NO_OUTPUT");
    assert!(t["errors"][0]["fix"].as_str().unwrap().contains("nothing"));
}

#[test]
fn a_program_that_hangs_times_out() {
    let tree = Tree::new("l-hang");
    let wizard = tree.wizard_with(|env| {
        env.limits.version_timeout = Duration::from_millis(300);
        env.limits.live_timeout = Duration::from_millis(500);
    });
    let id = verified_with_binary(&wizard, "hang-agent");
    let started = std::time::Instant::now();
    let t = live(&wizard, &id);
    assert_eq!(t["ok"], false);
    assert_eq!(t["errors"][0]["code"], "LIVETEST_TIMEOUT");
    assert_eq!(stage(&t, "pong")["timed_out"], true);
    assert!(started.elapsed() < Duration::from_secs(20));
}

#[test]
fn a_program_that_lingers_after_its_last_event_is_stopped_and_still_passes() {
    let tree = Tree::new("l-linger");
    let wizard = tree.wizard_with(|env| env.limits.live_grace = Duration::from_millis(200));
    let id = verified_with_binary(&wizard, "linger-agent");
    let started = std::time::Instant::now();
    let t = live(&wizard, &id);
    assert_eq!(t["ok"], true, "{}", t["text"]);
    assert!(started.elapsed() < Duration::from_secs(45), "three stages would take three minutes if it waited for exit");
}

#[test]
fn live_output_is_redacted_and_the_reply_is_capped() {
    let tree = Tree::new("l-redact");
    let wizard = tree.wizard();
    let id = verified(&wizard, "fake-agent", GOOD);
    let t = live(&wizard, &id);
    for s in t["stages"].as_array().unwrap() {
        assert!(s["stdout"].as_str().unwrap().len() <= 8 * 1024 * 4);
    }
    // stored samples are the redacted text
    let sample = wizard.read(&id, |s| s.samples[0].stdout.clone()).unwrap();
    assert!(sample.contains("\"text\":\"pong\""));
}

fn profile(text: &str) -> DeclarativeProfile {
    DeclarativeProfile::new(crate::tui::declarative::schema::parse_definition(text).unwrap()).unwrap()
}

#[test]
fn judge_accepts_the_word_with_trailing_punctuation_and_quotes() {
    let p = profile(GOOD);
    let out = |text: &str| format!("{{\"event\":\"start\",\"session\":\"fk-1\"}}\n{{\"event\":\"end\",\"session\":\"fk-1\",\"text\":{}}}\n", serde_json::to_string(text).unwrap());
    for answer in ["pong", "Pong.", "  PONG!\n", "\"pong\"", "`pong`"] {
        let j = judge(&p, Stage::Pong, &out(answer), Some(0), None, true);
        assert!(j.passed(), "{answer:?}: {:?}", j.problems);
    }
    for answer in ["pong2", "ping", "the word is pong", ""] {
        let j = judge(&p, Stage::Pong, &out(answer), Some(0), None, true);
        assert!(!j.passed(), "{answer:?} must not pass");
    }
}

#[test]
fn judge_checks_the_session_and_the_resume_id() {
    let p = profile(GOOD);
    let body = "{\"event\":\"end\",\"session\":\"fk-1\",\"text\":\"pong2\"}\n";
    assert!(judge(&p, Stage::Resume, body, Some(0), Some("fk-1"), true).passed());
    let moved = judge(&p, Stage::Resume, body, Some(0), Some("fk-9"), true);
    assert!(moved.problems[0].contains("does not continue"), "{:?}", moved.problems);
    let no_session = "{\"event\":\"end\",\"text\":\"pong\"}\n";
    let j = judge(&p, Stage::Pong, no_session, Some(0), None, true);
    assert!(j.problems.iter().any(|m| m.contains("no session id")), "{:?}", j.problems);
    assert!(judge(&p, Stage::Pong, no_session, Some(0), None, false).passed(), "no session selector: nothing to find");
}

#[test]
fn judge_wants_an_error_and_no_answer_for_a_wrong_model() {
    let p = profile(GOOD);
    let error = "{\"event\":\"error\",\"message\":\"Model not found: x.\"}\n";
    let j = judge(&p, Stage::BadModel, error, Some(1), None, true);
    assert!(j.passed(), "{:?}", j.problems);
    assert_eq!(j.classified.as_ref().unwrap().0, "AGENT_REPORTED_ERROR");
    // an answer despite the wrong model is wrong
    let answered = "{\"event\":\"end\",\"session\":\"s\",\"text\":\"pong\"}\n";
    assert!(!judge(&p, Stage::BadModel, answered, Some(0), None, true).passed());
    // nothing recognisable and a clean exit is wrong too (not recognised as an error)
    assert!(!judge(&p, Stage::BadModel, "", Some(0), None, true).passed());
}

#[test]
fn stage_args_render_the_same_command_the_run_used() {
    let def = crate::tui::declarative::schema::parse_definition(GOOD).unwrap();
    assert_eq!(
        stage_args(&def, Stage::Resume, "W", Some("fk-7")),
        ["run", "--format", "json", "-s", "fk-7", "--", "Reply with exactly the word: pong2"]
    );
    assert_eq!(stage_args(&def, Stage::BadModel, "W", None)[3], "-m");
}

#[test]
fn tested_helper_leaves_a_tested_session() {
    let tree = Tree::new("l-helper");
    let wizard = tree.wizard();
    let id = tested(&wizard, "fake-agent", GOOD);
    assert_eq!(wizard.read(&id, |s| s.state.as_str()).unwrap(), "tested");
    let _ = start;
}
