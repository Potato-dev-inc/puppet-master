//! Golden push payloads: the exact JSON a phone receives for each case, with the one-time token
//! replaced by `"<token>"`. Compared as `serde_json::Value`, so key order and whitespace do not matter.
//! A change to what a lock screen says is a change to a file in `golden/`, reviewed like any other.
//!
//! Regenerate after an intended change: `PM_UPDATE_GOLDEN=1 cargo test --lib push::tests_golden`,
//! then read the diff.

use super::hub::testing::{hub_with, FakeDirectory};
use super::hub::{Candidate, PushHub};
use super::store::test_subscription;
use super::triggers::{Kind, PromptInfo, PromptRisk};
use crate::prompt_risk::RiskLevel;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;

fn golden_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/push/golden").join(format!("{name}.json"))
}

/// One FCM phone that may answer ("dev-a") and one that may not ("dev-w", watch only).
fn hub(name: &str) -> PushHub {
    let dir = Arc::new(FakeDirectory::new(&[("tok-a", "dev-a"), ("tok-w", "dev-w")]));
    dir.cannot_answer.lock().push("dev-w".into());
    let hub = hub_with(name, dir);
    let (a, _, _) = test_subscription("https://fcm.googleapis.com/fcm/send/a");
    let (w, _, _) = test_subscription("https://fcm.googleapis.com/fcm/send/w");
    hub.store().lock().upsert("dev-a", a).unwrap();
    hub.store().lock().upsert("dev-w", w).unwrap();
    hub
}

fn payload_for(deliveries: &[super::hub::Delivery], device: &str) -> Value {
    let delivery = deliveries.iter().find(|d| d.device_id == device).unwrap_or_else(|| panic!("no delivery for {device}"));
    let mut payload: Value = serde_json::from_slice(&delivery.payload).unwrap();
    if let Some(token) = payload.pointer_mut("/act/token") {
        *token = json!("<token>");
    }
    payload
}

fn check(name: &str, actual: &Value) {
    let path = golden_path(name);
    if std::env::var("PM_UPDATE_GOLDEN").is_ok() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, serde_json::to_string_pretty(actual).unwrap() + "\n").unwrap();
    }
    let text = std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("missing golden {}", path.display()));
    let expected: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(&expected, actual, "golden {name} differs; actual:\n{}", serde_json::to_string_pretty(actual).unwrap());
}

fn prompt(level: Option<(RiskLevel, &str)>, text: &str) -> PromptInfo {
    let risk = level.map(|(level, headline)| PromptRisk { level, headline: headline.into() });
    PromptInfo {
        prompt_id: "pid-1".into(),
        text: text.into(),
        actionable: risk.as_ref().is_some_and(|r| r.level == RiskLevel::Low),
        risk,
    }
}

fn waiting(heading: Option<&str>, prompt: Option<PromptInfo>) -> Candidate {
    Candidate {
        kind: Kind::WaitingInput,
        key: "pane-1".into(),
        pane_id: Some("pane-1".into()),
        agent_label: "Claude Code".into(),
        heading: heading.map(str::to_string),
        prompt,
    }
}

fn plan(name: &str, candidate: &Candidate) -> Vec<super::hub::Delivery> {
    hub(name).plan(candidate, 1_000).1
}

#[test]
fn approval_low_android() {
    let c = waiting(Some("auth refactor"), Some(prompt(Some((RiskLevel::Low, "Run tests")), "Run command: npm test (in shop)")));
    let payload = payload_for(&plan("g-low", &c), "dev-a");
    assert!(payload.get("act").is_some(), "a calm command gets Allow / Deny on a phone that may answer");
    check("approval_low_android", &payload);
}

#[test]
fn approval_medium_android() {
    let c = waiting(Some("auth refactor"), Some(prompt(Some((RiskLevel::Medium, "Install packages")), "Run command: npm install left-pad")));
    let payload = payload_for(&plan("g-medium", &c), "dev-a");
    assert!(payload.get("act").is_none(), "Check first opens the card; no lock-screen Allow");
    assert!(payload["body"].as_str().unwrap().starts_with("Check first · Install packages\n"));
    check("approval_medium_android", &payload);
}

#[test]
fn approval_high_android() {
    let c = waiting(Some("auth refactor"), Some(prompt(Some((RiskLevel::High, "Delete a folder outside this project")), "Run command: rm -rf ../build-cache")));
    let payload = payload_for(&plan("g-high", &c), "dev-a");
    assert!(payload.get("act").is_none());
    check("approval_high_android", &payload);
}

#[test]
fn approval_unrated() {
    let c = waiting(None, Some(prompt(None, "")));
    let payload = payload_for(&plan("g-unrated", &c), "dev-a");
    assert!(payload.get("act").is_none(), "no risk read fails closed");
    check("approval_unrated", &payload);
}

#[test]
fn approval_long_command() {
    let long = format!("Run command: echo {}", "x".repeat(200));
    let c = waiting(Some("auth refactor"), Some(prompt(Some((RiskLevel::Low, "Run a command")), &long)));
    let payload = payload_for(&plan("g-long", &c), "dev-a");
    assert!(payload.get("act").is_none(), "a cut command is never allowed from a lock screen");
    check("approval_long_command", &payload);
}

#[test]
fn approval_watch_only() {
    let c = waiting(Some("auth refactor"), Some(prompt(Some((RiskLevel::Low, "Run tests")), "Run command: npm test (in shop)")));
    let payload = payload_for(&plan("g-watch", &c), "dev-w");
    assert!(payload.get("act").is_none(), "a phone without the answer ability gets no buttons");
    check("approval_watch_only", &payload);
}

#[test]
fn question() {
    let c = Candidate {
        kind: Kind::Question,
        key: "q-1".into(),
        pane_id: None,
        agent_label: "Claude Code".into(),
        heading: None,
        prompt: Some(PromptInfo { prompt_id: String::new(), text: "Keep the old /login route as a redirect, or remove it?".into(), actionable: false, risk: None }),
    };
    check("question", &payload_for(&plan("g-question", &c), "dev-a"));
}

#[test]
fn error() {
    let c = Candidate { kind: Kind::Error, key: "pane-2".into(), pane_id: Some("pane-2".into()), agent_label: "PowerShell".into(), heading: Some("build".into()), prompt: None };
    check("error", &payload_for(&plan("g-error", &c), "dev-a"));
}

#[test]
fn done() {
    let c = Candidate { kind: Kind::RunFinished, key: "pane-3".into(), pane_id: Some("pane-3".into()), agent_label: "Codex".into(), heading: Some("Codex 2".into()), prompt: None };
    check("done", &payload_for(&plan("g-done", &c), "dev-a"));
}

#[test]
fn bidi_title() {
    let c = waiting(Some("fix\u{202E}gnp.exe login"), Some(prompt(Some((RiskLevel::High, "Delete files")), "Run command: rm a")));
    let payload = payload_for(&plan("g-bidi", &c), "dev-a");
    assert!(!payload["title"].as_str().unwrap().contains('\u{202E}'));
    check("bidi_title", &payload);
}

#[test]
fn coalesced_two() {
    let h = hub("g-coalesced");
    let mut first = waiting(Some("auth refactor"), Some(prompt(Some((RiskLevel::High, "Delete a folder outside this project")), "Run command: rm -rf ../build-cache")));
    first.key = "pane-1".into();
    h.plan(&first, 1_000);
    let mut second = first.clone();
    second.prompt.as_mut().unwrap().prompt_id = "pid-2".into();
    assert_eq!(h.plan(&second, 5_000).0, super::triggers::Verdict::RateLimited);
    let due = h.take_due(31_000);
    let (verdict, deliveries) = h.plan_held(&due[0], 2, 31_000);
    assert_eq!(verdict, super::triggers::Verdict::Send);
    let payload = payload_for(&deliveries, "dev-a");
    assert!(payload.get("act").is_none());
    check("coalesced_two", &payload);
}

/// Every golden file is exercised: a stray file means a case was dropped.
#[test]
fn no_golden_file_is_orphaned() {
    let dir = golden_path("x").parent().unwrap().to_path_buf();
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().trim_end_matches(".json").to_string())
        .collect();
    names.sort();
    let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/push/tests_golden.rs")).unwrap();
    for name in names {
        assert!(src.contains(&format!("check(\"{name}\"")), "golden/{name}.json is not used by any test");
    }
}
