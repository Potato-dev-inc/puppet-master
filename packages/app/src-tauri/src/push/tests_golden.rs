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

/// One FCM phone that may answer ("dev-a"), one that may not ("dev-w", watch only), an iPhone
/// ("dev-i", Apple's push service) and an Android phone with "Hide details on lock screen" on ("dev-h").
fn hub(name: &str) -> PushHub {
    let dir = Arc::new(FakeDirectory::new(&[("tok-a", "dev-a"), ("tok-w", "dev-w"), ("tok-i", "dev-i"), ("tok-h", "dev-h")]));
    dir.cannot_answer.lock().push("dev-w".into());
    let hub = hub_with(name, dir);
    for (device, endpoint) in [
        ("dev-a", "https://fcm.googleapis.com/fcm/send/a"),
        ("dev-w", "https://fcm.googleapis.com/fcm/send/w"),
        ("dev-i", "https://web.push.apple.com/i"),
        ("dev-h", "https://fcm.googleapis.com/fcm/send/h"),
    ] {
        let (sub, _, _) = test_subscription(endpoint);
        hub.store().lock().upsert(device, sub).unwrap();
    }
    hub.store().lock().set_hide_details("dev-h", true).unwrap();
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
        item: None,
        detail: None,
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
        item: None,
        detail: None,
    };
    check("question", &payload_for(&plan("g-question", &c), "dev-a"));
}

#[test]
fn error() {
    let c = Candidate { kind: Kind::Error, key: "pane-2".into(), pane_id: Some("pane-2".into()), agent_label: "PowerShell".into(), heading: Some("build".into()), prompt: None, item: None, detail: None };
    check("error", &payload_for(&plan("g-error", &c), "dev-a"));
}

#[test]
fn done() {
    let c = Candidate { kind: Kind::RunFinished, key: "pane-3".into(), pane_id: Some("pane-3".into()), agent_label: "Codex".into(), heading: Some("Codex 2".into()), prompt: None, item: None, detail: None };
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

fn item(id: &str) -> Option<super::hub::ItemRef> {
    Some(super::hub::ItemRef { id: id.into(), occurrence: 7, fingerprint: "f3c1f3c1f3c1f3c1".into() })
}

fn info(text: &str) -> Option<PromptInfo> {
    Some(PromptInfo { prompt_id: String::new(), text: text.into(), actionable: false, risk: None })
}

#[test]
fn approval_low_ios() {
    let c = waiting(Some("auth refactor"), Some(prompt(Some((RiskLevel::Low, "Run tests")), "Run command: npm test (in shop)")));
    let payload = payload_for(&plan("g-low-ios", &c), "dev-i");
    assert!(payload.get("act").is_none(), "Apple shows no action buttons: no grant that nothing can use");
    assert_eq!(payload["body"], "Low risk · Run tests\nClaude Code", "two lines: the risk and who, no command");
    check("approval_low_ios", &payload);
}

#[test]
fn approval_hide_details() {
    let c = waiting(Some("fix the payroll bug"), Some(prompt(Some((RiskLevel::Low, "Run tests")), "Run command: npm test (in shop)")));
    let payload = payload_for(&plan("g-hide", &c), "dev-h");
    assert!(payload.get("act").is_none(), "no buttons, so nothing that answers can be tapped from a lock screen");
    let text = payload.to_string();
    assert!(!text.contains("payroll") && !text.contains("npm test") && !text.contains("Run tests"), "the pane's title, the command and the headline stay off the lock screen: {text}");
    check("approval_hide_details", &payload);
}

#[test]
fn question_ios() {
    let c = Candidate {
        kind: Kind::Question,
        key: "q-1".into(),
        pane_id: None,
        agent_label: "Claude Code".into(),
        heading: None,
        prompt: info("Keep the old /login route as a redirect, or remove it?"),
        item: item("Qk2x9aLm"),
        detail: None,
    };
    let payload = payload_for(&plan("g-question-ios", &c), "dev-i");
    assert_eq!(payload["body"], "Question from Claude Code");
    assert_eq!(payload["item_id"], "Qk2x9aLm", "the tap opens this card");
    check("question_ios", &payload);
}

#[test]
fn trust_gate() {
    let c = Candidate {
        kind: Kind::TrustGate,
        key: "pane-1".into(),
        pane_id: Some("pane-1".into()),
        agent_label: "Claude Code".into(),
        heading: Some("auth refactor".into()),
        prompt: Some(PromptInfo { prompt_id: "pid-t".into(), text: "Trust this workspace?".into(), actionable: false, risk: None }),
        item: item("T7rustAb"),
        detail: Some("shop".into()),
    };
    let payload = payload_for(&plan("g-trust", &c), "dev-a");
    assert!(payload.get("act").is_none(), "a folder is never trusted from a lock screen");
    assert_eq!(payload["body"], "Trust this folder? · shop\nClaude Code");
    check("trust_gate", &payload);
}

#[test]
fn trust_gate_signin() {
    let c = Candidate {
        kind: Kind::TrustGate,
        key: "pane-1".into(),
        pane_id: Some("pane-1".into()),
        agent_label: "Claude Code".into(),
        heading: Some("auth refactor".into()),
        prompt: None,
        item: None,
        detail: None,
    };
    let payload = payload_for(&plan("g-signin", &c), "dev-a");
    assert_eq!(payload["body"], "Claude Code needs you on your PC");
    check("trust_gate_signin", &payload);
}

#[test]
fn consent() {
    let c = Candidate {
        kind: Kind::Consent,
        key: "q-c1".into(),
        pane_id: None,
        agent_label: "Codex".into(),
        heading: None,
        prompt: info("Allow Codex to control Claude Code?"),
        item: item("C0nsentX"),
        detail: Some("Codex wants to control Claude Code".into()),
    };
    let payload = payload_for(&plan("g-consent", &c), "dev-a");
    assert!(payload.get("act").is_none() && payload.get("target").is_none());
    check("consent", &payload);
    let hidden = payload_for(&plan("g-consent-h", &c), "dev-h");
    assert_eq!(hidden["body"], "Codex wants to control another agent");
}

#[test]
fn budget() {
    let c = Candidate {
        kind: Kind::Budget,
        key: "q-b1".into(),
        pane_id: None,
        agent_label: "Claude Code".into(),
        heading: None,
        prompt: info("Claude Code used its 20 turns."),
        item: item("B9dgetXy"),
        detail: Some("20 of 20 turns".into()),
    };
    let payload = payload_for(&plan("g-budget", &c), "dev-a");
    assert_eq!(payload["body"], "Budget used up · 20 of 20 turns\nClaude Code");
    check("budget", &payload);
}

#[test]
fn limit() {
    let c = Candidate {
        kind: Kind::Limit,
        key: "q-l1".into(),
        pane_id: None,
        agent_label: "Claude Code".into(),
        heading: None,
        prompt: info("Claude Code ran out of usage."),
        item: item("L1mitZzZ"),
        detail: Some("14:05".into()),
    };
    let payload = payload_for(&plan("g-limit", &c), "dev-a");
    assert_eq!(payload["body"], "Usage limit · resets 14:05\nClaude Code");
    check("limit", &payload);
}

/// Every first line of every needs-you body, on every phone, is at most 56 characters.
#[test]
fn no_first_line_is_longer_than_fifty_six_characters() {
    use super::triggers::{copy_for_view, View, MAX_FIRST_LINE_CHARS};
    let long = "z".repeat(200);
    let kinds = [
        (Kind::TrustGate, Some(long.as_str())),
        (Kind::Consent, Some(long.as_str())),
        (Kind::Budget, Some(long.as_str())),
        (Kind::Limit, Some(long.as_str())),
        (Kind::Question, None),
    ];
    for (kind, detail) in kinds {
        for view in [View::Full, View::Ios, View::Hidden] {
            let prompt = PromptInfo { prompt_id: "p".into(), text: long.clone(), actionable: false, risk: None };
            let words = copy_for_view(kind, "Claude Code", Some("auth refactor"), Some(&prompt), detail, view);
            let first = words.body.lines().next().unwrap();
            assert!(first.chars().count() <= MAX_FIRST_LINE_CHARS, "{kind:?} {view:?}: {first}");
        }
    }
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
