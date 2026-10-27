//! Live test: the three stages, classification, samples and replay.

use std::time::Duration;

use serde_json::{json, Value};

use crate::tui::declarative::wizard::livetest::{judge, mask_words, stage_args, BAD_MODEL};
use crate::tui::declarative::wizard::session::{Stage, TestWords};
use crate::tui::declarative::DeclarativeProfile;

use super::support::{call, ok, start, tested, verified, Tree, GOOD, MINIMAL, WRONG_PATH};

/// `GOOD` without its `[headless.output.error]` table, whatever line endings the checkout gave the file
/// (a Windows checkout with `core.autocrlf` writes CRLF; a multi-line `replace` of `\n` text then
/// silently changes nothing, which is how the CI run once classified the wrong-model error by its event).
fn without_error_selector(text: &str) -> String {
    text.replace("\r\n", "\n").replace(
        "[headless.output.error]\nwhere = { event = \"error\" }\npath = \"message\"\npick = \"last\"\n",
        "",
    )
}

fn live(wizard: &crate::tui::declarative::wizard::Wizard, id: &str) -> Value {
    ok(wizard, "livetest", super::support::live_body(wizard, id))
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
    // the real answers are random per session and masked in everything that goes back to the UI
    let pong = stage(&t, "pong");
    assert_eq!(pong["status"], "passed");
    assert_eq!(pong["result"], "<word-1>");
    assert_eq!(pong["session"], "fk-0001");
    assert_eq!(pong["exit"], 0);
    assert_eq!(pong["args"], json!(["run", "--format", "json", "--", "Reply with exactly the word: <word-1>"]));
    let sentence = stage(&t, "sentence");
    assert_eq!(sentence["status"], "passed");
    assert_eq!(sentence["result"], "the sky is <word-3>.");
    assert_eq!(sentence["args"][4], "Reply with exactly: the sky is <word-3>.");
    let resume = stage(&t, "resume");
    assert_eq!(resume["status"], "passed");
    assert_eq!(resume["result"], "<word-2>");
    assert_eq!(
        resume["args"],
        json!(["run", "--format", "json", "-s", "fk-0001", "--", "Reply with exactly the word: <word-2>"]),
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
        json!(["headless_success.jsonl", "headless_sentence.jsonl", "headless_resume.jsonl", "headless_error.jsonl", "headless_garbage.txt"])
    );
    assert_eq!(t["errors"], json!([]));
    assert_eq!(wizard.read(&id, |s| s.samples.len()).unwrap(), 4);
    let words = wizard.read(&id, |s| s.words.clone()).unwrap();
    for w in words.all() {
        assert!(!t.to_string().contains(w), "the answer word {w} must not appear in the reply");
    }
}

#[test]
fn a_wrong_error_case_is_judged_by_exit_code_when_there_is_no_error_selector() {
    let tree = Tree::new("l-exitcode");
    let wizard = tree.wizard();
    let draft = without_error_selector(GOOD);
    assert!(!draft.contains("headless.output.error"), "the error selector is really gone");
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
    assert_eq!(t["fixtures"], json!(["headless_success.jsonl", "headless_sentence.jsonl", "headless_garbage.txt"]));
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
    assert!(e["excerpt"].as_str().unwrap().contains("\"text\":\"<word-1>\""), "{e}");
    let words = wizard.read(&id, |s| s.words.clone()).unwrap();
    assert!(!again.to_string().contains(&words.one), "the real answer must not leak through the replay");
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
        env.limits.probe_timeout = Duration::from_millis(300);
        env.limits.live_timeout = Duration::from_millis(500);
    });
    let id = verified_with_binary(&wizard, "hang-agent");
    let started = std::time::Instant::now();
    let t = live(&wizard, &id);
    assert_eq!(t["ok"], false);
    assert_eq!(t["errors"][0]["code"], "LIVETEST_TIMEOUT");
    assert_eq!(stage(&t, "pong")["timed_out"], true);
    assert!(started.elapsed() < Duration::from_secs(50));
}

#[test]
fn a_program_that_lingers_after_its_last_event_is_stopped_and_still_passes() {
    let tree = Tree::new("l-linger");
    let wizard = tree.wizard_with(|env| env.limits.live_grace = Duration::from_millis(200));
    let id = verified_with_binary(&wizard, "linger-agent");
    let started = std::time::Instant::now();
    let t = live(&wizard, &id);
    assert_eq!(t["ok"], true, "{}", t["text"]);
    assert!(started.elapsed() < Duration::from_secs(150), "three stages would take three minutes if it waited for exit");
}

#[test]
fn the_live_test_always_runs_in_the_work_folder_even_if_the_draft_asks_for_the_apps_folder() {
    let tree = Tree::new("l-cwd");
    let wizard = tree.wizard();
    let draft = GOOD.replace("task_separator = \"--\"", "task_separator = \"--\"
cwd = \"inherit\"");
    let id = verified(&wizard, "fake-agent", &draft);
    let t = live(&wizard, &id);
    assert_eq!(t["ok"], true, "{}", t["text"]);
    let work = wizard.read(&id, |s| s.work_dir.file_name().unwrap().to_string_lossy().into_owned()).unwrap();
    assert!(stage(&t, "pong")["stdout"].as_str().unwrap().contains(&work), "{}", stage(&t, "pong")["stdout"]);
}

#[test]
fn only_the_program_the_person_typed_ever_runs_whatever_other_names_the_draft_lists() {
    let tree = Tree::new("l-names");
    let wizard = tree.wizard();
    // `silent-agent` exists on the search path and would print nothing; the typed program must be used.
    let id = verified(&wizard, "fake-agent", &GOOD.replace("names = [\"fake-agent\"]", "names = [\"fake-agent\", \"silent-agent\"]"));
    let t = live(&wizard, &id);
    assert_eq!(t["ok"], true, "{}", t["text"]);
    assert_eq!(stage(&t, "pong")["result"], "<word-1>");
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
    let words = wizard.read(&id, |s| s.words.clone()).unwrap();
    assert!(sample.contains(&format!("\"text\":\"{}\"", words.one)), "stored samples keep the real output for replay");
}

fn profile(text: &str) -> DeclarativeProfile {
    DeclarativeProfile::new(crate::tui::declarative::schema::parse_definition(text).unwrap()).unwrap()
}

#[test]
fn judge_accepts_the_word_with_trailing_punctuation_and_quotes() {
    let p = profile(GOOD);
    let out = |text: &str| format!("{{\"event\":\"start\",\"session\":\"fk-1\"}}\n{{\"event\":\"end\",\"session\":\"fk-1\",\"text\":{}}}\n", serde_json::to_string(text).unwrap());
    for answer in ["pong-qwerty", "Pong-Qwerty.", "  PONG-QWERTY!\n", "\"pong-qwerty\"", "`pong-qwerty`"] {
        let j = judge(&p, Stage::Pong, "pong-qwerty", &out(answer), Some(0), None, true);
        assert!(j.passed(), "{answer:?}: {:?}", j.problems);
    }
    for answer in ["pong-other", "ping", "the word is pong-qwerty", ""] {
        let j = judge(&p, Stage::Pong, "pong-qwerty", &out(answer), Some(0), None, true);
        assert!(!j.passed(), "{answer:?} must not pass");
    }
}

#[test]
fn judge_checks_the_session_and_the_resume_id() {
    let p = profile(GOOD);
    let body = "{\"event\":\"end\",\"session\":\"fk-1\",\"text\":\"pong-second\"}\n";
    assert!(judge(&p, Stage::Resume, "pong-second", body, Some(0), Some("fk-1"), true).passed());
    let moved = judge(&p, Stage::Resume, "pong-second", body, Some(0), Some("fk-9"), true);
    assert!(moved.problems[0].contains("does not continue"), "{:?}", moved.problems);
    let no_session = "{\"event\":\"end\",\"text\":\"pong-one\"}\n";
    let j = judge(&p, Stage::Pong, "pong-one", no_session, Some(0), None, true);
    assert!(j.problems.iter().any(|m| m.contains("no session id")), "{:?}", j.problems);
    assert!(judge(&p, Stage::Pong, "pong-one", no_session, Some(0), None, false).passed(), "no session selector: nothing to find");
}

#[test]
fn judge_wants_an_error_and_no_answer_for_a_wrong_model() {
    let p = profile(GOOD);
    let error = "{\"event\":\"error\",\"message\":\"Model not found: x.\"}\n";
    let j = judge(&p, Stage::BadModel, "", error, Some(1), None, true);
    assert!(j.passed(), "{:?}", j.problems);
    assert_eq!(j.classified.as_ref().unwrap().0, "AGENT_REPORTED_ERROR");
    // an answer despite the wrong model is wrong
    let answered = "{\"event\":\"end\",\"session\":\"s\",\"text\":\"pong-one\"}\n";
    assert!(!judge(&p, Stage::BadModel, "", answered, Some(0), None, true).passed());
    // nothing recognisable and a clean exit is wrong too (not recognised as an error)
    assert!(!judge(&p, Stage::BadModel, "", "", Some(0), None, true).passed());
}

#[test]
fn stage_args_render_the_same_command_the_run_used() {
    let def = crate::tui::declarative::schema::parse_definition(GOOD).unwrap();
    let words = TestWords { one: "pong-aaaaaa".into(), two: "pong-bbbbbb".into(), sky: "cccccc".into() };
    assert_eq!(
        stage_args(&def, Stage::Resume, "W", Some("fk-7"), &words),
        ["run", "--format", "json", "-s", "fk-7", "--", "Reply with exactly the word: pong-bbbbbb"]
    );
    assert_eq!(stage_args(&def, Stage::BadModel, "W", None, &words)[3], "-m");
    assert_eq!(
        stage_args(&def, Stage::Sentence, "W", None, &words),
        ["run", "--format", "json", "--", "Reply with exactly: the sky is cccccc."]
    );
    assert_eq!(mask_words("pong-aaaaaa / the sky is cccccc. / pong-bbbbbb", &words), "<word-1> / the sky is <word-3>. / <word-2>");
}

#[test]
fn tested_helper_leaves_a_tested_session() {
    let tree = Tree::new("l-helper");
    let wizard = tree.wizard();
    let id = tested(&wizard, "fake-agent", GOOD);
    assert_eq!(wizard.read(&id, |s| s.state.as_str()).unwrap(), "tested");
    let _ = start;
}

#[test]
fn verify_shows_the_exact_commands_the_live_test_will_run() {
    let tree = Tree::new("l-planned");
    let wizard = tree.wizard();
    let id = start(&wizard, "fake-agent");
    ok(&wizard, "probe", json!({"session": id}));
    let v = ok(&wizard, "verify", json!({"session": id, "toml": GOOD}));
    let planned = &v["planned"];
    assert!(planned["program"]["path"].as_str().unwrap().to_lowercase().contains("fake-agent"));
    let stages = planned["stages"].as_array().unwrap();
    assert_eq!(stages.iter().map(|s| s["name"].as_str().unwrap()).collect::<Vec<_>>(), ["pong", "sentence", "resume", "bad_model"]);
    assert_eq!(stages[0]["args"], json!(["run", "--format", "json", "--", "<test message>"]));
    assert_eq!(stages[1]["args"], json!(["run", "--format", "json", "--", "<test message>"]));
    assert_eq!(stages[2]["args"], json!(["run", "--format", "json", "-s", "<session id>", "--", "<test message>"]));
    assert_eq!(stages[3]["args"], json!(["run", "--format", "json", "-m", BAD_MODEL, "--", "<test message>"]));
    assert_eq!(planned["fingerprint"].as_str().unwrap(), super::support::plan_of(&wizard, &id));
    // a draft that fails verify has no plan
    let bad = ok(&wizard, "verify", json!({"session": id, "toml": "schema_version = 1\nnonsense = 1\n"}));
    assert!(bad.get("planned").is_none());
    // stages that do not apply are not planned
    let minimal = ok(&wizard, "verify", json!({"session": id, "toml": MINIMAL}));
    assert_eq!(minimal["planned"]["stages"].as_array().unwrap().len(), 2);
}

#[test]
fn the_live_test_refuses_to_run_without_the_fingerprint_of_what_the_person_saw() {
    let tree = Tree::new("l-plan-refuse");
    let wizard = tree.wizard();
    let id = verified(&wizard, "fake-agent", GOOD);
    let missing = call(&wizard, "livetest", json!({"session": id, "confirm": true})).unwrap_err();
    assert_eq!((missing.status, missing.code), (400, "WIZARD_PLAN_REQUIRED"));
    for wrong in ["", "   ", "deadbeef", "0".repeat(64).as_str()] {
        let err = call(&wizard, "livetest", json!({"session": id, "confirm": true, "plan": wrong})).unwrap_err();
        assert!(err.code == "WIZARD_PLAN_REQUIRED" || err.code == "WIZARD_PLAN_MISMATCH", "{wrong:?}: {err}");
    }
    let err = call(&wizard, "livetest", json!({"session": id, "confirm": true, "plan": "0".repeat(64)})).unwrap_err();
    assert_eq!((err.status, err.code), (409, "WIZARD_PLAN_MISMATCH"));
    // nothing ran and the session is free again
    assert!(wizard.read(&id, |s| s.samples.is_empty() && s.busy.is_none()).unwrap());
    assert_eq!(wizard.read(&id, |s| s.state.as_str()).unwrap(), "verified");
    // the right fingerprint works
    assert_eq!(live(&wizard, &id)["ok"], true);
}

#[test]
fn a_fingerprint_from_an_earlier_draft_is_refused_after_the_arguments_change() {
    let tree = Tree::new("l-plan-stale");
    let wizard = tree.wizard();
    let id = verified(&wizard, "fake-agent", GOOD);
    let old = super::support::plan_of(&wizard, &id);
    // a fix round changes the argv (an extra model flag)
    let changed = GOOD.replace("argv = [\"run\", \"--format\", \"json\"]", "argv = [\"run\", \"--format\", \"json\", \"-m\", \"fake/slow\"]");
    let v = ok(&wizard, "verify", json!({"session": id, "toml": changed}));
    assert_eq!(v["ok"], true, "{}", v["text"]);
    let new = v["planned"]["fingerprint"].as_str().unwrap().to_string();
    assert_ne!(new, old, "the fingerprint follows the arguments");
    let err = call(&wizard, "livetest", json!({"session": id, "confirm": true, "plan": old})).unwrap_err();
    assert_eq!(err.code, "WIZARD_PLAN_MISMATCH");
    assert!(wizard.read(&id, |s| s.samples.is_empty()).unwrap(), "the program was not started");
    assert_eq!(call(&wizard, "livetest", json!({"session": id, "confirm": true, "plan": new})).unwrap()["ok"], true);
}

#[test]
fn a_change_that_does_not_touch_the_commands_keeps_the_same_fingerprint() {
    let tree = Tree::new("l-plan-same");
    let wizard = tree.wizard();
    let id = verified(&wizard, "fake-agent", GOOD);
    let first = super::support::plan_of(&wizard, &id);
    // only the label changes: the commands are the same, so no new confirmation is needed
    let v = ok(&wizard, "verify", json!({"session": id, "toml": GOOD.replace("Fake Agent", "Renamed")}));
    assert_eq!(v["planned"]["fingerprint"], first);
}

const ONE_WORD_REGEX: &str = r#"schema_version = 1
[agent]
id = "raw_agent"
label = "Raw"
experimental = true
[binary]
names = ["raw-agent"]
windows_extensions = ["exe"]
[headless]
argv = ["run"]
task_separator = "--"
[headless.output]
format = "plain_text"
[headless.output.result]
regex = '(?m)^\s*(\S+)\s*$'
pick = "last"
"#;

#[test]
fn a_one_word_regex_passes_the_first_stage_but_the_sentence_stage_catches_it() {
    let tree = Tree::new("l-sentence");
    let wizard = tree.wizard();
    let id = verified(&wizard, "raw-agent", ONE_WORD_REGEX);
    let t = live(&wizard, &id);
    assert_eq!(t["ok"], false);
    assert_eq!(stage(&t, "pong")["status"], "passed", "a one-word answer is read by any one-word regex");
    assert_eq!(stage(&t, "sentence")["status"], "failed");
    assert_eq!(stage(&t, "resume")["status"], "skipped");
    assert_eq!(t["errors"][0]["code"], "LIVETEST_SENTENCE_FAILED");
    let words = wizard.read(&id, |s| s.words.clone()).unwrap();
    assert!(!t.to_string().contains(&words.sky), "the answer is masked even in the failure text");
    assert!(t["text"].as_str().unwrap().contains("<word-3>"), "{}", t["text"]);
}

#[test]
fn a_draft_that_is_built_around_the_test_answers_is_rejected_by_verify() {
    let tree = Tree::new("l-gamed");
    let wizard = tree.wizard();
    let id = start(&wizard, "fake-agent");
    ok(&wizard, "probe", json!({"session": id}));
    let words = wizard.read(&id, |s| s.words.clone()).unwrap();
    let with_words = |extra: &str| format!("{GOOD}\n# {extra}\n");
    for leaked in [
        with_words(&words.one),
        with_words(&words.two),
        with_words(&words.one[5..]),
        with_words(&format!("the sky is {}.", words.sky)),
        with_words("pong"),
        with_words("Pong"),
        with_words("pong-[a-z]+"),
        GOOD.replace("path = \"text\"", "path = \"text\"\nregex = '^(pong2?)$'"),
    ] {
        let v = ok(&wizard, "verify", json!({"session": id, "toml": leaked}));
        assert_eq!(v["ok"], false, "{leaked}");
        let codes: Vec<&str> = v["errors"].as_array().unwrap().iter().map(|e| e["code"].as_str().unwrap()).collect();
        assert!(codes.contains(&"WIZARD_TEST_WORD"), "{codes:?} for {leaked}");
        for w in words.all() {
            assert!(!v.to_string().contains(w), "the error must not give the word away");
        }
    }
    // words that merely start with the same letters are fine
    assert_eq!(ok(&wizard, "verify", json!({"session": id, "toml": with_words("pongo is a game")}))["ok"], true);
    assert_eq!(ok(&wizard, "verify", json!({"session": id, "toml": GOOD}))["ok"], true);
}

#[test]
fn the_answers_are_random_per_session_and_look_like_pong_and_six_letters() {
    let tree = Tree::new("l-words");
    let wizard = tree.wizard();
    let a = start(&wizard, "fake-agent");
    let b = start(&wizard, "fake-agent");
    let wa = wizard.read(&a, |s| s.words.clone()).unwrap();
    let wb = wizard.read(&b, |s| s.words.clone()).unwrap();
    assert_ne!(wa, wb);
    for w in [&wa, &wb] {
        for one in [&w.one, &w.two] {
            assert!(regex::Regex::new(r"^pong-[a-z]{6}$").unwrap().is_match(one), "{one}");
        }
        assert!(regex::Regex::new(r"^[a-z]{6}$").unwrap().is_match(&w.sky), "{}", w.sky);
        assert_ne!(w.one, w.two);
        assert_eq!(w.sentence(), format!("the sky is {}.", w.sky));
    }
}

#[test]
fn the_sentence_must_match_word_for_word_but_not_in_case_quotes_or_the_final_stop() {
    let p = profile(GOOD);
    let out = |text: &str| format!("{{\"event\":\"end\",\"session\":\"s\",\"text\":{}}}\n", serde_json::to_string(text).unwrap());
    let want = "the sky is abcdef.";
    for answer in ["the sky is abcdef.", "The sky is abcdef", "\"the sky is abcdef.\"", " the  sky is abcdef!\n"] {
        assert!(judge(&p, Stage::Sentence, want, &out(answer), Some(0), None, true).passed(), "{answer:?}");
    }
    for answer in ["abcdef", "the sky is blue", "the sky is abcdef and green", ""] {
        assert!(!judge(&p, Stage::Sentence, want, &out(answer), Some(0), None, true).passed(), "{answer:?}");
    }
}

/// A draft for the fake CLI that the validator accepts but the shared rules reject: plain text with no
/// anchored result selector reads garbage as an answer.
pub(super) const UNANCHORED: &str = "schema_version = 1\n[agent]\nid = \"fake_agent\"\nlabel = \"F\"\nexperimental = true\n[binary]\nnames = [\"fake-agent\"]\nwindows_extensions = [\"exe\"]\n[headless]\nargv = [\"run\"]\ntask_separator = \"--\"\n[headless.output]\nformat = \"plain_text\"\n";

#[test]
fn a_draft_that_only_fails_the_shared_rules_is_still_live_tested() {
    let tree = Tree::new("l-conformance-first");
    let wizard = tree.wizard();
    let id = start(&wizard, "fake-agent");
    ok(&wizard, "probe", json!({"session": id}));
    let v = ok(&wizard, "verify", json!({"session": id, "toml": UNANCHORED}));
    assert_eq!(v["ok"], false);
    assert!(v["errors"].as_array().unwrap().iter().any(|e| e["stage"] == "conformance"), "{}", v["text"]);
    assert_eq!(v["can_livetest"], true, "the live test may run");
    assert!(v["planned"]["fingerprint"].is_string());
    assert_eq!(v["state"], "probed", "not verified");

    // the live test runs (with the usual fingerprint) and reports the real output AND the rule problems
    let t = live(&wizard, &id);
    assert_eq!(t["ok"], false);
    assert_eq!(t["state"], "probed");
    assert_eq!(stage(&t, "pong")["status"], "passed", "the plain answer is read");
    assert!(t["errors"].as_array().unwrap().iter().any(|e| e["stage"] == "conformance"), "{}", t["text"]);
    assert!(t["text"].as_str().unwrap().contains("[conformance]"), "{}", t["text"]);
    assert!(wizard.read(&id, |s| !s.samples.is_empty()).unwrap(), "the real output is kept");
    // it cannot be finished
    let err = call(&wizard, "finish", json!({"session": id, "provenance": super::support::provenance()})).unwrap_err();
    assert_eq!(err.code, "WIZARD_BAD_STATE");
}

#[test]
fn a_draft_that_fails_a_live_stage_and_the_shared_rules_reports_both_in_one_reply() {
    let tree = Tree::new("l-both");
    let wizard = tree.wizard();
    let id = start(&wizard, "raw-agent");
    ok(&wizard, "probe", json!({"session": id}));
    // raw-agent prints "Thinking..." before the answer: the unanchored selector reads both lines
    let draft = UNANCHORED.replace("fake-agent", "raw-agent").replace("fake_agent", "raw_agent");
    let v = ok(&wizard, "verify", json!({"session": id, "toml": draft}));
    assert_eq!(v["can_livetest"], true, "{}", v["text"]);
    let t = live(&wizard, &id);
    let text = t["text"].as_str().unwrap();
    assert!(text.contains("[livetest] LIVETEST_PONG_FAILED"), "{text}");
    assert!(text.contains("[conformance]"), "{text}");
    assert!(text.contains("Thinking..."), "the real output is shown: {text}");
}

#[test]
fn a_draft_that_does_not_validate_is_never_live_tested() {
    let tree = Tree::new("l-invalid-blocks");
    let wizard = tree.wizard();
    let id = start(&wizard, "fake-agent");
    ok(&wizard, "probe", json!({"session": id}));
    for bad in [super::support::NO_PATH, super::support::WRONG_BINARY, "schema_version = 1\nnonsense = 1\n", "not toml ["] {
        let v = ok(&wizard, "verify", json!({"session": id, "toml": bad}));
        assert_eq!(v["ok"], false);
        assert!(v.get("planned").is_none() && v.get("can_livetest").is_none(), "{bad}");
    }
    // nothing was stored, so the live test has nothing to run
    let err = call(&wizard, "livetest", json!({"session": id, "confirm": true, "plan": "0".repeat(64)})).unwrap_err();
    assert_eq!((err.status, err.code), (409, "WIZARD_BAD_STATE"));
    assert!(wizard.read(&id, |s| s.draft.is_none() && s.samples.is_empty()).unwrap());
}

#[test]
fn a_testable_but_unclean_draft_replaces_the_verified_one_and_drops_its_test() {
    let tree = Tree::new("l-replace");
    let wizard = tree.wizard();
    let id = tested(&wizard, "fake-agent", GOOD);
    assert_eq!(wizard.read(&id, |s| s.state.as_str()).unwrap(), "tested");
    let v = ok(&wizard, "verify", json!({"session": id, "toml": UNANCHORED}));
    assert_eq!(v["ok"], false);
    assert_eq!(v["state"], "probed");
    assert!(wizard.read(&id, |s| s.tested.is_none() && s.draft.as_ref().unwrap().text == UNANCHORED).unwrap());
    // the old draft's fingerprint no longer opens the live test
    let old = call(&wizard, "livetest", json!({"session": id, "confirm": true, "plan": "0".repeat(64)})).unwrap_err();
    assert_eq!(old.code, "WIZARD_PLAN_MISMATCH");
}

#[test]
fn a_clean_draft_still_goes_to_tested_and_finish_through_the_same_path() {
    let tree = Tree::new("l-clean");
    let wizard = tree.wizard();
    let id = verified(&wizard, "fake-agent", GOOD);
    let t = live(&wizard, &id);
    assert_eq!(t["ok"], true, "{}", t["text"]);
    assert_eq!(t["state"], "tested");
    assert_eq!(t["errors"], json!([]), "no rule or fixture problem is added for a clean draft");
}

#[test]
fn an_error_the_agent_reports_itself_is_marked_as_the_agents_own_problem() {
    let tree = Tree::new("l-agent-error");
    let wizard = tree.wizard();
    let id = verified_with_binary(&wizard, "broken-agent");
    let t = live(&wizard, &id);
    assert_eq!(t["ok"], false);
    let ae = &t["agent_error"];
    assert_eq!(ae["stage"], "pong");
    assert_eq!(ae["external"], true, "{t}");
    assert!(ae["message"].as_str().unwrap().starts_with("No endpoints found that support tool use"), "{ae}");
    // the stage carries it too, and the later stages did not run
    assert_eq!(stage(&t, "pong")["agent_side"], true);
    assert_eq!(stage(&t, "sentence")["status"], "skipped");
}

#[test]
fn a_program_that_exits_with_a_sign_in_or_quota_looking_error_is_the_agents_problem() {
    let tree = Tree::new("l-unauth");
    let wizard = tree.wizard();
    let id = verified_with_binary(&wizard, "unauth-agent");
    let t = live(&wizard, &id);
    let ae = &t["agent_error"];
    assert_eq!(ae["external"], true, "{t}");
    assert_eq!(ae["message"], "Error: not logged in. Run the login command first.", "only the first line");
}

#[test]
fn an_unexplained_crash_has_a_message_but_is_not_called_the_agents_setup() {
    let tree = Tree::new("l-crash");
    let wizard = tree.wizard();
    let id = verified_with_binary(&wizard, "crash-agent");
    let t = live(&wizard, &id);
    let ae = &t["agent_error"];
    assert_eq!(ae["external"], false, "nothing says sign-in, quota or provider: {t}");
    assert_eq!(ae["message"], "thread 'main' panicked at src/main.rs:12:5", "so a repeat can still be noticed");
}

#[test]
fn a_program_that_exits_with_an_error_code_and_printed_only_its_banner_has_no_agent_error() {
    // The first line it printed is the logo and the version: that is not an error text, and a repeat of it must
    // never stop the loop (the draft may still be wrong).
    let tree = Tree::new("l-banner-only");
    let wizard = tree.wizard();
    let id = verified_with_binary(&wizard, "banner-agent");
    let t = live(&wizard, &id);
    assert_eq!(t["ok"], false);
    assert_eq!(t["agent_error"], json!(null), "{t}");
    assert_eq!(stage(&t, "pong")["agent_side"], false);
}

#[test]
fn the_error_row_is_reported_not_the_banner_that_came_before_it() {
    let tree = Tree::new("l-banner-then-error");
    let wizard = tree.wizard();
    let id = verified_with_binary(&wizard, "bannererr-agent");
    let t = live(&wizard, &id);
    let ae = &t["agent_error"];
    assert_eq!(ae["external"], true, "{t}");
    assert_eq!(ae["message"], "Error: No endpoints found for this model. Check your provider settings.", "{ae}");
}

#[test]
fn a_wrong_selector_or_a_good_draft_is_not_an_agent_error() {
    let tree = Tree::new("l-no-agent-error");
    let wizard = tree.wizard();
    let id = verified(&wizard, "fake-agent", WRONG_PATH);
    assert_eq!(live(&wizard, &id)["agent_error"], json!(null), "the program answered fine; the selector is wrong");
    let tree2 = Tree::new("l-no-agent-error-2");
    let wizard2 = tree2.wizard();
    let id2 = verified(&wizard2, "fake-agent", GOOD);
    let t = live(&wizard2, &id2);
    assert_eq!(t["ok"], true);
    assert_eq!(t["agent_error"], json!(null), "the expected wrong-model error is not one");
}

#[test]
fn the_agent_error_survives_a_second_live_test_of_the_same_draft() {
    // Retry: the same draft and the same fingerprint run again, with no new verify.
    let tree = Tree::new("l-retry");
    let wizard = tree.wizard();
    let id = verified_with_binary(&wizard, "broken-agent");
    assert_eq!(live(&wizard, &id)["agent_error"]["external"], true);
    assert_eq!(live(&wizard, &id)["agent_error"]["external"], true);
    assert_eq!(wizard.read(&id, |s| s.state.as_str()).unwrap(), "verified");
}

#[test]
fn the_test_drafts_are_checked_out_with_unix_line_endings() {
    // .gitattributes pins them (a CRLF checkout once made a multi-line replace do nothing on CI)
    for (name, text) in [("good", GOOD), ("wrong_path", WRONG_PATH), ("minimal", MINIMAL)] {
        assert!(!text.contains('\r'), "{name}.toml has CRLF line endings; see .gitattributes");
    }
}

#[test]
fn the_helper_and_the_whole_wizard_work_the_same_with_windows_line_endings() {
    let crlf = GOOD.replace("\r\n", "\n").replace('\n', "\r\n");
    assert!(crlf.contains("\r\n"));
    // the old multi-line replace on CRLF text changed nothing: this is the bug the helper removes
    assert!(crlf.replace("[headless.output.error]\nwhere", "").contains("headless.output.error"));
    assert!(!without_error_selector(&crlf).contains("headless.output.error"));
    assert_eq!(without_error_selector(&crlf), without_error_selector(GOOD));

    // and a CRLF draft goes through verify, the live test and finish byte for byte
    let tree = Tree::new("l-crlf");
    let wizard = tree.wizard();
    let id = verified(&wizard, "fake-agent", &crlf);
    let t = live(&wizard, &id);
    assert_eq!(t["ok"], true, "{}", t["text"]);
    super::support::ok(&wizard, "finish", json!({"session": id, "provenance": super::support::provenance()}));
    assert_eq!(std::fs::read_to_string(tree.agents.join("fake_agent.toml")).unwrap(), crlf);
}

#[test]
fn a_stage_says_so_when_the_program_was_stopped_after_its_last_event() {
    let tree = Tree::new("l-stopped-early");
    let wizard = tree.wizard_with(|env| env.limits.live_grace = Duration::from_millis(200));
    let id = verified_with_binary(&wizard, "linger-agent");
    let t = live(&wizard, &id);
    assert_eq!(t["ok"], true, "{}", t["text"]);
    assert_eq!(stage(&t, "pong")["stopped_early"], true, "it was stopped after its done event, not left to exit");
    assert_eq!(stage(&t, "pong")["exit"], 0);
    // a program that exits on its own keeps its real exit code and is not marked stopped
    // the program exits on its own: a long grace, so a loaded machine cannot have the wizard stop it first
    let tree2 = Tree::new("l-exits-itself");
    let wizard2 = tree2.wizard_with(|env| env.limits.live_grace = Duration::from_secs(30));
    let id2 = verified(&wizard2, "fake-agent", GOOD);
    let t2 = live(&wizard2, &id2);
    assert_eq!(stage(&t2, "pong")["stopped_early"], false);
    assert_eq!(stage(&t2, "bad_model")["stopped_early"], false);
    assert_eq!(stage(&t2, "bad_model")["exit"], 1, "the real exit code of the wrong-model run");
}
