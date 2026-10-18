//! "How agent runs work": the verified flag, the saved choice, and what the choice does to a run.
//! Fake agents only: no CLI is started and no app data folder is read or written.

use super::store::{self, TestEnv};
use super::verify::{evaluate, Inputs};
use super::*;
use crate::agent_runs::AgentRunRequest;
use serde_json::json;
use sha2::{Digest, Sha256};

const WITH_HEADLESS: &str = include_str!("../tui/declarative/fixtures/definitions/kilo-like.toml");
const TUI_ONLY: &str = include_str!("../tui/declarative/fixtures/definitions/minimal-tui.toml");

fn sha(text: &str) -> String {
    Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// What the wizard writes into `<id>.wizard.json` (`wizard/finish.rs::provenance_json`).
fn wizard_json(id: &str, text: &str, stages: &[(&str, &str)]) -> String {
    json!({
        "wizard_schema": 1,
        "id": id,
        "definition_sha256": sha(text),
        "results": {
            "stages": stages.iter().map(|(n, s)| json!({"name": n, "status": s})).collect::<Vec<_>>(),
            "fixtures": {"passed": 5, "total": 5},
        },
    })
    .to_string()
}

const PASSED: &[(&str, &str)] = &[("pong", "passed"), ("sentence", "passed"), ("resume", "passed"), ("bad_model", "skipped")];

fn check(text: &str, wizard: Option<&str>, in_force: bool) -> Verification {
    evaluate(&Inputs { in_force, file_bytes: Some(text.as_bytes()), wizard_json: wizard })
}

// ---- headless_verified -------------------------------------------------------------------------

#[test]
fn a_headless_table_with_a_matching_passed_wizard_test_is_verified() {
    let wizard = wizard_json("kilolike", WITH_HEADLESS, PASSED);
    let verdict = check(WITH_HEADLESS, Some(&wizard), true);
    assert!(verdict.verified, "{verdict:?}");
    assert_eq!(verdict.reason, None);
}

#[test]
fn a_definition_changed_after_the_test_is_not_verified() {
    let wizard = wizard_json("kilolike", WITH_HEADLESS, PASSED);
    // Even a label edit changes the file that was tested.
    let edited = WITH_HEADLESS.replace("label = \"Kilo Code\"", "label = \"Kilo Code 2\"");
    assert_ne!(edited, WITH_HEADLESS);
    let verdict = check(&edited, Some(&wizard), true);
    assert!(!verdict.verified);
    assert_eq!(verdict.reason, Some("changed_since_test"));
    // An argv edit, the case that matters most.
    let argv = WITH_HEADLESS.replace("\"--format\", \"json\"", "\"--format\", \"json\", \"--yolo\"");
    assert_ne!(argv, WITH_HEADLESS);
    assert!(!check(&argv, Some(&wizard), true).verified);
}

#[test]
fn a_definition_nobody_tested_is_not_verified() {
    let verdict = check(WITH_HEADLESS, None, true);
    assert_eq!((verdict.verified, verdict.reason), (false, Some("not_tested")));
    assert!(!check(WITH_HEADLESS, Some("{ not json"), true).verified);
    assert!(!check(WITH_HEADLESS, Some("{}"), true).verified);
}

#[test]
fn a_failed_or_missing_live_stage_is_not_verified() {
    let failed = wizard_json("kilolike", WITH_HEADLESS, &[("pong", "passed"), ("sentence", "failed")]);
    assert!(!check(WITH_HEADLESS, Some(&failed), true).verified);
    let only_pong = wizard_json("kilolike", WITH_HEADLESS, &[("pong", "passed")]);
    assert!(!check(WITH_HEADLESS, Some(&only_pong), true).verified);
    let resume_failed = wizard_json(
        "kilolike",
        WITH_HEADLESS,
        &[("pong", "passed"), ("sentence", "passed"), ("resume", "failed")],
    );
    assert!(!check(WITH_HEADLESS, Some(&resume_failed), true).verified);
}

#[test]
fn a_test_recorded_for_another_agent_does_not_count() {
    let wizard = wizard_json("someone_else", WITH_HEADLESS, PASSED);
    assert!(!check(WITH_HEADLESS, Some(&wizard), true).verified);
}

#[test]
fn a_definition_without_a_headless_mode_is_never_verified() {
    let wizard = wizard_json("minimal", TUI_ONLY, PASSED);
    let verdict = check(TUI_ONLY, Some(&wizard), true);
    assert_eq!((verdict.verified, verdict.reason), (false, Some("no_headless_mode")));
}

#[test]
fn a_definition_that_is_not_in_force_is_not_verified() {
    let wizard = wizard_json("kilolike", WITH_HEADLESS, PASSED);
    let verdict = check(WITH_HEADLESS, Some(&wizard), false);
    assert_eq!((verdict.verified, verdict.reason), (false, Some("not_approved")));
}

#[test]
fn built_in_agents_report_support_but_no_verification() {
    let fields = agent_fields("claude", false, true);
    assert_eq!(fields["headless_supported"], true);
    assert_eq!(fields["headless_verified"], false);
    assert_eq!(fields["headless_verified_reason"], "built_in");
    assert_eq!(fields["run_mode"], "visible");
}

#[test]
fn the_listing_gets_the_new_fields_on_every_agent() {
    let listing = json!({"agents": [
        {"id": "claude", "dispatchable": true, "modes": {"headless": true, "visible": true}, "definition": null},
        {"id": "fake_custom", "dispatchable": true, "modes": {"headless": true, "visible": true}, "definition": {"state": "active"}},
        {"id": "fake_plain", "dispatchable": true, "modes": {"headless": false, "visible": true}, "definition": {"state": "active"}},
    ], "dispatchable_ids": ["claude"]});
    let mut env = TestEnv::default();
    env.verified.insert("fake_custom".into(), true);
    env.modes.insert("fake_custom".into(), RunMode::OneTask);
    let out = store::with_test_env(env, || decorate_listing(listing));
    let by_id = |id: &str| out["agents"].as_array().unwrap().iter().find(|a| a["id"] == id).unwrap().clone();
    assert_eq!(by_id("claude")["headless_supported"], true);
    assert_eq!(by_id("claude")["headless_verified"], false);
    assert_eq!(by_id("fake_custom")["headless_verified"], true);
    assert_eq!(by_id("fake_custom")["run_mode"], "one_task");
    assert_eq!(by_id("fake_plain")["headless_supported"], false);
    assert_eq!(by_id("fake_plain")["headless_verified"], false);
    assert_eq!(by_id("fake_plain")["run_mode"], "visible");
    assert_eq!(out["dispatchable_ids"], json!(["claude"]), "the rest of the body is untouched");
}

// ---- the saved choice ----------------------------------------------------------------------------

#[test]
fn the_default_is_a_visible_pane_for_every_agent() {
    assert_eq!(RunMode::default(), RunMode::Visible);
    assert_eq!(setting_for("anything_at_all"), RunMode::Visible);
    store::with_test_env(TestEnv::default(), || {
        assert_eq!(setting_for("fake_custom"), RunMode::Visible);
    });
}

#[test]
fn a_missing_garbled_or_older_file_means_visible() {
    assert_eq!(store::mode_in(&store::parse_text(""), "x"), RunMode::Visible);
    assert_eq!(store::mode_in(&store::parse_text("{ not json"), "x"), RunMode::Visible);
    assert_eq!(store::mode_in(&store::parse_text("{}"), "x"), RunMode::Visible);
    // A value a newer build wrote is ignored, the others still count.
    let text = r#"{"version":9,"agents":{"x":"warp_speed","y":"one_task"}}"#;
    assert_eq!(store::mode_in(&store::parse_text(text), "x"), RunMode::Visible);
    assert_eq!(store::mode_in(&store::parse_text(text), "y"), RunMode::OneTask);
    assert_eq!(store::mode_in(&store::parse_text(text), "Y"), RunMode::OneTask, "ids are not case sensitive");
}

#[test]
fn the_choice_is_saved_per_agent_and_choosing_the_default_removes_it() {
    let dir = std::env::temp_dir().join(format!("pm-run-mode-{}-store", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let target = dir.join(store::file_name());
    store::save_at(&target, "Alpha", RunMode::OneTask).unwrap();
    store::save_at(&target, "beta", RunMode::OneTask).unwrap();
    let file = store::read_file(&target);
    assert_eq!(store::mode_in(&file, "alpha"), RunMode::OneTask);
    assert_eq!(store::mode_in(&file, "beta"), RunMode::OneTask);
    assert_eq!(store::mode_in(&file, "gamma"), RunMode::Visible);
    store::save_at(&target, "alpha", RunMode::Visible).unwrap();
    let text = std::fs::read_to_string(&target).unwrap();
    assert!(!text.contains("alpha") && text.contains("beta"), "{text}");
    assert!(store::save_at(&target, "  ", RunMode::OneTask).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn one_task_mode_cannot_be_saved_for_an_agent_that_is_not_verified() {
    let mut env = TestEnv::default();
    env.verified.insert("fake_ok".into(), true);
    store::with_test_env(env, || {
        let refused = set_checked("fake_nope", RunMode::OneTask).unwrap_err();
        assert!(refused.starts_with("RUN_MODE_NOT_VERIFIED"), "{refused}");
        assert_eq!(setting_for("fake_nope"), RunMode::Visible);
        set_checked("fake_ok", RunMode::OneTask).unwrap();
        assert_eq!(setting_for("fake_ok"), RunMode::OneTask);
        // Going back to the visible pane is always allowed.
        set_checked("fake_nope", RunMode::Visible).unwrap();
    });
    assert_eq!(RunMode::parse("one_task"), Some(RunMode::OneTask));
    assert_eq!(RunMode::parse("headless"), None);
}

// ---- what the choice does to a run ---------------------------------------------------------------

fn request(agent: &str) -> AgentRunRequest {
    serde_json::from_value(json!({"task": "say hi", "agent_type": agent})).unwrap()
}

/// A run whose caller did not pass `headless` (the `/agents/run` route sets this flag).
fn unset(agent: &str) -> AgentRunRequest {
    let mut r = request(agent);
    r.headless_unset = true;
    r
}

fn env(agent: &str, mode: RunMode, verified: bool) -> TestEnv {
    let mut env = TestEnv::default();
    env.modes.insert(agent.into(), mode);
    env.verified.insert(agent.into(), verified);
    env
}

#[test]
fn a_run_without_headless_uses_the_agents_one_task_setting_when_verified() {
    let mut r = unset("claude");
    let decision = store::with_test_env(env("claude", RunMode::OneTask, true), || apply(&mut r));
    assert!(r.headless, "one-task mode runs headless");
    assert_eq!(decision.note, Some(RunModeNote::new("one_task", "agent_setting")));
    assert_eq!(decision.fallback, None);
}

#[test]
fn the_default_setting_changes_nothing() {
    let mut r = unset("claude");
    let decision = store::with_test_env(env("claude", RunMode::Visible, true), || apply(&mut r));
    assert!(!r.headless);
    assert_eq!(decision, Decision::default());
}

#[test]
fn an_explicit_headless_from_the_caller_always_wins() {
    // headless: false was passed (the route leaves headless_unset false): stays visible.
    let mut explicit_false = request("claude");
    let d = store::with_test_env(env("claude", RunMode::OneTask, true), || apply(&mut explicit_false));
    assert!(!explicit_false.headless);
    assert_eq!(d, Decision::default());
    // headless: true was passed: stays headless, the setting adds nothing to the receipt.
    let mut explicit_true = request("claude");
    explicit_true.headless = true;
    let d = store::with_test_env(env("claude", RunMode::Visible, true), || apply(&mut explicit_true));
    assert!(explicit_true.headless);
    assert_eq!(d, Decision::default());
    // Even an unset flag cannot turn an explicit true back into a pane.
    let mut both = unset("claude");
    both.headless = true;
    store::with_test_env(env("claude", RunMode::Visible, false), || apply(&mut both));
    assert!(both.headless);
}

#[test]
fn a_follow_up_turn_keeps_the_mode_of_its_run() {
    // The follow-up request is built with headless_unset=false and a turn index above zero: the
    // setting is not read, so changing it between turns never flips a conversation.
    let mut later = unset("claude");
    later.turn_index = 1;
    later.headless = true; // what send_agent derives from the earlier run having no pane
    let d = store::with_test_env(env("claude", RunMode::Visible, true), || apply(&mut later));
    assert!(later.headless);
    assert_eq!(d, Decision::default());
    let mut visible_later = unset("claude");
    visible_later.turn_index = 2;
    let d = store::with_test_env(env("claude", RunMode::OneTask, true), || apply(&mut visible_later));
    assert!(!visible_later.headless, "a conversation that began in a pane stays there");
    assert_eq!(d, Decision::default());
}

#[test]
fn adopting_a_pane_or_joining_a_mission_never_uses_the_setting() {
    let mut adopting = unset("claude");
    adopting.pane_id = Some("pane-1".into());
    store::with_test_env(env("claude", RunMode::OneTask, true), || apply(&mut adopting));
    assert!(!adopting.headless);
    let mut mission = unset("claude");
    mission.until_checks_pass = true;
    store::with_test_env(env("claude", RunMode::OneTask, true), || apply(&mut mission));
    assert!(!mission.headless);
}

#[test]
fn an_unverified_one_task_agent_falls_back_to_the_visible_pane_and_says_so() {
    let mut r = unset("claude");
    let decision = store::with_test_env(env("claude", RunMode::OneTask, false), || apply(&mut r));
    assert!(!r.headless, "nothing unverified is run headless");
    let fallback = decision.fallback.expect("the receipt carries the fallback");
    assert_eq!(
        serde_json::to_value(&fallback).unwrap(),
        json!({"from": "one_task", "to": "visible", "reason": "not_verified"})
    );
    assert_eq!(decision.note, Some(RunModeNote::new("visible", "agent_setting")));
}

#[test]
fn an_agent_with_no_headless_mode_at_all_also_falls_back() {
    let d = decide(RunMode::OneTask, true, false);
    assert!(!d.headless);
    assert_eq!(d.fallback.map(|f| f.reason), Some("not_verified".to_string()));
}

#[test]
fn the_decision_is_pure_over_setting_and_verification() {
    assert_eq!(decide(RunMode::Visible, true, true), Decision::default());
    assert_eq!(decide(RunMode::Visible, false, true), Decision::default());
    let go = decide(RunMode::OneTask, true, true);
    assert!(go.headless && go.fallback.is_none());
    let stay = decide(RunMode::OneTask, false, true);
    assert!(!stay.headless && stay.fallback.is_some());
}
