//! Tests for `notify.rs`: which events go out, the buttons, the windows, the gate and the golden texts.

use super::*;
use crate::attention::model::Kind as Item;
use crate::channels::config::PairedChat;
use crate::channels::grants::GrantError;
use crate::channels::refs::WINDOW_TTL_MS;
use crate::mobile_pairing::Caps;
use crate::prompt_risk::RiskLevel;
use crate::push::triggers::PromptRisk;
use serde_json::json;

const SECRET: &str = "sk-ant-api03-TESTKEYTESTKEY123456";
const HOUR_MS: u64 = 3_600_000;

/// The links the notifier reads, owned by the test.
struct World {
    refs: Refs,
    grants: Grants,
    bucket: Bucket,
}

impl World {
    fn new() -> Self {
        Self {
            refs: Refs::default(),
            grants: Grants::default(),
            bucket: Bucket::new(20, 60_000),
        }
    }

    fn links(&mut self) -> Links<'_> {
        Links {
            refs: &self.refs,
            grants: &self.grants,
            outbound: &mut self.bucket,
        }
    }
}

/// A paired chat with the default caps (watch, answer, type).
fn cfg() -> TelegramConfig {
    TelegramConfig {
        enabled: true,
        enabled_at_ms: Some(0),
        paired: Some(PairedChat {
            chat_id: 5_001,
            user_id: 1_001,
            username: Some("tester".into()),
            first_name: Some("Tester".into()),
            paired_at_ms: 0,
        }),
        ..TelegramConfig::default()
    }
}

fn candidate(kind: Kind, prompt: Option<PromptInfo>, item: Option<ItemRef>) -> Candidate {
    Candidate {
        kind,
        key: "pane-1".to_string(),
        pane_id: Some("pane-1".to_string()),
        agent_label: "Claude Code".to_string(),
        heading: Some("auth refactor".to_string()),
        prompt,
        item,
        detail: None,
    }
}

fn approval(text: &str, actionable: bool, level: RiskLevel, headline: &str) -> PromptInfo {
    PromptInfo {
        prompt_id: "p1".to_string(),
        text: text.to_string(),
        actionable,
        risk: Some(PromptRisk {
            level,
            headline: headline.to_string(),
        }),
    }
}

fn item() -> ItemRef {
    ItemRef {
        id: "item-1".to_string(),
        occurrence: 3,
        fingerprint: "fp1".to_string(),
    }
}

fn question(choices: &[(&str, &str)]) -> Question {
    let choices: Vec<_> = choices
        .iter()
        .map(|(id, label)| json!({"id": id, "label": label}))
        .collect();
    serde_json::from_value(json!({
        "id": "q1",
        "question": "Which branch should I use?",
        "choices": choices,
        "allow_free_text": false,
        "urgency": "normal",
        "created_at_ms": 0,
        "expires_at_ms": 900_000,
        "state": "pending",
    }))
    .expect("question fixture")
}

fn text_of(note: &Note) -> String {
    note.out.text.as_str().to_string()
}

fn labels(note: &Note) -> Vec<Vec<String>> {
    note.out
        .keyboard
        .iter()
        .map(|row| row.iter().map(|b| b.label.clone()).collect())
        .collect()
}

/// No token, e-mail address or home path may reach the chat.
fn assert_clean(text: &str) {
    assert!(!text.contains("sk-ant"), "token leaked: {text}");
    assert!(!text.contains("you@example.com"), "e-mail leaked: {text}");
    assert!(!text.contains("\\you\\"), "home path leaked: {text}");
}

#[test]
fn nothing_goes_out_when_the_chat_is_off_or_not_paired() {
    let mut world = World::new();
    let n = Notifier::default();
    let c = candidate(Kind::Error, None, None);
    let off = TelegramConfig {
        enabled: false,
        ..cfg()
    };
    assert!(n.offer(&c, None, &off, world.links(), 1).is_none());
    let unpaired = TelegramConfig {
        paired: None,
        ..cfg()
    };
    assert!(n.offer(&c, None, &unpaired, world.links(), 1).is_none());
}

#[test]
fn low_risk_approval_gets_allow_once_and_deny_in_one_row() {
    let mut world = World::new();
    let n = Notifier::default();
    let p = approval("npm test", true, RiskLevel::Low, "Run a test command");
    let c = candidate(Kind::WaitingInput, Some(p), Some(item()));
    let note = n
        .offer(&c, None, &cfg(), world.links(), 1_000)
        .expect("sent");
    assert_eq!(
        labels(&note),
        vec![vec!["Allow once…".to_string(), "Deny".to_string()]]
    );
    let data: Vec<&str> = note.out.keyboard[0]
        .iter()
        .map(|b| b.data.as_str())
        .collect();
    for d in &data {
        assert!(d.starts_with("g:") && d.len() <= 64, "bad button data {d}");
    }
    assert_eq!(note.remember, Some(Target::Pane("pane-1".to_string())));
    // The first tap opens the confirm step; Deny answers at once. Both are single-use grants.
    let allow = world.grants.redeem(&data[0][2..], 2_000);
    assert_eq!(
        allow,
        Ok(GrantAction::ConfirmAllow {
            item_id: "item-1".into(),
            prompt_id: "p1".into(),
            occurrence: 3,
            fingerprint: "fp1".into()
        })
    );
    // Redeeming one button drops the card's other buttons (grants.rs rule), so Deny is now dead.
    assert_eq!(
        world.grants.redeem(&data[1][2..], 2_000),
        Err(GrantError::Unknown)
    );

    // Deny on its own, from a fresh offer.
    let mut fresh = World::new();
    let note = Notifier::default()
        .offer(&c_low(), None, &cfg(), fresh.links(), 1_000)
        .expect("sent");
    let deny_data = note.out.keyboard[0][1].data.clone();
    assert_eq!(
        fresh.grants.redeem(&deny_data[2..], 2_000),
        Ok(GrantAction::Answer {
            item_id: "item-1".into(),
            choice: Some("deny".into()),
            prompt_id: Some("p1".into()),
            occurrence: Some(3),
            fingerprint: Some("fp1".into()),
        })
    );
}

fn c_low() -> Candidate {
    candidate(
        Kind::WaitingInput,
        Some(approval(
            "npm test",
            true,
            RiskLevel::Low,
            "Run a test command",
        )),
        Some(item()),
    )
}

#[test]
fn approval_buttons_expire_after_ten_minutes() {
    let mut world = World::new();
    let n = Notifier::default();
    let c = candidate(
        Kind::WaitingInput,
        Some(approval("npm test", true, RiskLevel::Low, "x")),
        Some(item()),
    );
    let note = n.offer(&c, None, &cfg(), world.links(), 0).expect("sent");
    let deny = note.out.keyboard[0][1].data.clone();
    assert_eq!(
        world.grants.redeem(&deny[2..], ANSWER_TTL_MS),
        Err(GrantError::Expired)
    );
}

#[test]
fn approval_without_the_answer_cap_has_no_buttons() {
    let mut world = World::new();
    let n = Notifier::default();
    let watch_only = TelegramConfig {
        caps: Caps::none().with(Cap::Watch, true),
        ..cfg()
    };
    let c = candidate(
        Kind::WaitingInput,
        Some(approval("npm test", true, RiskLevel::Low, "x")),
        Some(item()),
    );
    let note = n
        .offer(&c, None, &watch_only, world.links(), 1)
        .expect("sent");
    assert!(note.out.keyboard.is_empty());
    assert!(
        text_of(&note).ends_with("Open Puppet Master to answer."),
        "{}",
        text_of(&note)
    );
}

#[test]
fn high_risk_approval_has_no_buttons() {
    let mut world = World::new();
    let n = Notifier::default();
    let p = approval(
        "rm -rf ../old",
        false,
        RiskLevel::High,
        "Delete a folder outside this project",
    );
    let c = candidate(Kind::WaitingInput, Some(p), Some(item()));
    let note = n.offer(&c, None, &cfg(), world.links(), 1).expect("sent");
    assert!(note.out.keyboard.is_empty());
    assert!(text_of(&note).ends_with("Open Puppet Master to answer."));
}

#[test]
fn approval_is_not_offered_buttons_when_redaction_changed_the_command() {
    let mut world = World::new();
    let n = Notifier::default();
    let command = format!("curl -H {SECRET} example");
    let p = approval(&command, true, RiskLevel::Low, "Run a command");
    let c = candidate(Kind::WaitingInput, Some(p), Some(item()));
    let note = n.offer(&c, None, &cfg(), world.links(), 1).expect("sent");
    assert!(note.out.keyboard.is_empty());
    assert_clean(&text_of(&note));
}

#[test]
fn approval_that_is_not_actionable_has_no_buttons() {
    let mut world = World::new();
    let n = Notifier::default();
    let p = approval("npm test", false, RiskLevel::Low, "Run a test command");
    let c = candidate(Kind::WaitingInput, Some(p), Some(item()));
    let note = n.offer(&c, None, &cfg(), world.links(), 1).expect("sent");
    assert!(note.out.keyboard.is_empty());
}

#[test]
fn short_mode_hides_agent_text_and_buttons() {
    let mut world = World::new();
    let n = Notifier::default();
    let short = TelegramConfig {
        short_mode: true,
        ..cfg()
    };
    let p = approval("npm test", true, RiskLevel::Low, "Run a test command");
    let c = candidate(Kind::WaitingInput, Some(p), Some(item()));
    let note = n.offer(&c, None, &short, world.links(), 1).expect("sent");
    assert!(note.out.keyboard.is_empty());
    assert!(!text_of(&note).contains("npm test"), "{}", text_of(&note));
    assert!(
        !text_of(&note).contains("Run a test command"),
        "{}",
        text_of(&note)
    );
}

#[test]
fn watch_off_hides_agent_text_too() {
    let mut world = World::new();
    let n = Notifier::default();
    let no_watch = TelegramConfig {
        caps: Caps::none().with(Cap::Answer, true),
        ..cfg()
    };
    let p = approval("npm test", true, RiskLevel::Low, "Run a test command");
    let c = candidate(Kind::WaitingInput, Some(p), Some(item()));
    let note = n
        .offer(&c, None, &no_watch, world.links(), 1)
        .expect("sent");
    assert!(note.out.keyboard.is_empty());
    assert!(!text_of(&note).contains("npm test"));
}

#[test]
fn trust_consent_budget_and_limit_never_have_buttons() {
    for kind in [Kind::TrustGate, Kind::Consent, Kind::Budget, Kind::Limit] {
        let mut world = World::new();
        let n = Notifier::default();
        let mut c = candidate(kind, None, Some(item()));
        c.detail = Some("shop".to_string());
        let note = n.offer(&c, None, &cfg(), world.links(), 1).expect("sent");
        assert!(note.out.keyboard.is_empty(), "{kind:?}");
    }
}

#[test]
fn question_has_one_row_per_choice_up_to_six() {
    let mut world = World::new();
    let n = Notifier::default();
    let q = question(&[
        ("c1", "One"),
        ("c2", "Two"),
        ("c3", "Three"),
        ("c4", "Four"),
        ("c5", "Five"),
        ("c6", "Six"),
        ("c7", "Seven"),
        ("c8", "Eight"),
    ]);
    let mut c = candidate(
        Kind::Question,
        Some(PromptInfo {
            prompt_id: String::new(),
            text: q.question.clone(),
            actionable: false,
            risk: None,
        }),
        Some(item()),
    );
    c.heading = None;
    let note = n
        .offer(&c, Some(&q), &cfg(), world.links(), 1)
        .expect("sent");
    assert_eq!(note.out.keyboard.len(), MAX_CHOICES);
    assert!(note.out.keyboard.iter().all(|row| row.len() == 1));
    let first = note.out.keyboard[0][0].data.clone();
    assert_eq!(
        world.grants.redeem(&first[2..], 2),
        Ok(GrantAction::Answer {
            item_id: "item-1".into(),
            choice: Some("c1".into()),
            prompt_id: None,
            occurrence: None,
            fingerprint: None
        })
    );
}

#[test]
fn question_without_its_card_has_no_buttons() {
    let mut world = World::new();
    let n = Notifier::default();
    let q = question(&[("yes", "Yes")]);
    let c = candidate(
        Kind::Question,
        Some(PromptInfo {
            prompt_id: String::new(),
            text: q.question.clone(),
            actionable: false,
            risk: None,
        }),
        None,
    );
    let note = n
        .offer(&c, Some(&q), &cfg(), world.links(), 1)
        .expect("sent");
    assert!(note.out.keyboard.is_empty());
    assert!(text_of(&note).ends_with("Open Puppet Master to answer."));
}

#[test]
fn duplicate_prompt_is_not_sent_twice() {
    let mut world = World::new();
    let n = Notifier::default();
    let c = candidate(
        Kind::WaitingInput,
        Some(approval("npm test", true, RiskLevel::Low, "x")),
        Some(item()),
    );
    assert!(n.offer(&c, None, &cfg(), world.links(), 0).is_some());
    assert!(n.offer(&c, None, &cfg(), world.links(), 40_000).is_none());
}

#[test]
fn thirty_second_gate_holds_a_second_prompt_and_summarises_it_later() {
    let mut world = World::new();
    let n = Notifier::default();
    let first = candidate(
        Kind::WaitingInput,
        Some(approval("npm test", true, RiskLevel::Low, "x")),
        Some(item()),
    );
    assert!(n.offer(&first, None, &cfg(), world.links(), 0).is_some());
    let mut second_prompt = approval("npm run build", true, RiskLevel::Low, "x");
    second_prompt.prompt_id = "p2".to_string();
    let second = candidate(Kind::WaitingInput, Some(second_prompt), Some(item()));
    assert!(n
        .offer(&second, None, &cfg(), world.links(), 10_000)
        .is_none());
    // Too early: the held message is 30 s old only at 40 s.
    assert!(n.flush_held(world.links().outbound, 30_000).is_none());
    let summary = n
        .flush_held(world.links().outbound, 40_000)
        .expect("summary");
    assert_eq!(
        text_of(&summary),
        "1 more thing needs you. Send /status to see them."
    );
    assert!(
        n.flush_held(world.links().outbound, 50_000).is_none(),
        "summary goes once"
    );
}

#[test]
fn run_finished_goes_out_only_with_notify_runs_or_a_window() {
    let mut world = World::new();
    let n = Notifier::default();
    let c = candidate(Kind::RunFinished, None, None);
    assert!(n.offer(&c, None, &cfg(), world.links(), 1).is_none());
    world.refs.open_window(&Target::Pane("pane-1".into()), 1);
    let note = n
        .offer(&c, None, &cfg(), world.links(), 2)
        .expect("window open");
    assert_eq!(text_of(&note), "auth refactor · Done\nClaude Code finished");
    assert_eq!(note.remember, Some(Target::Pane("pane-1".into())));

    // notify_runs alone is enough, with no window. A fresh notifier: the first send holds the 30 s gate.
    let mut other = World::new();
    let fresh = Notifier::default();
    let runs = TelegramConfig {
        notify_runs: true,
        ..cfg()
    };
    let c2 = candidate(Kind::RunFailed, None, None);
    assert!(fresh.offer(&c2, None, &runs, other.links(), 5).is_some());
}

#[test]
fn companion_reply_goes_out_only_inside_the_companions_window() {
    let mut world = World::new();
    let n = Notifier::default();
    let c = cfg();
    assert!(n
        .companion_reply("c1", "Mochi", "The build is green.", &c, world.links(), 0)
        .is_none());
    world.refs.open_window(&Target::Companion("c1".into()), 0);
    let note = n
        .companion_reply(
            "c1",
            "Mochi",
            "The build is green.",
            &c,
            world.links(),
            1_000,
        )
        .expect("in window");
    assert_eq!(text_of(&note), "Mochi replied:\nThe build is green.");
    assert_eq!(note.remember, Some(Target::Companion("c1".into())));
    // Another companion has no window.
    assert!(n
        .companion_reply("c2", "Pico", "Hi", &c, world.links(), 1_000)
        .is_none());
    // The window closes after 15 minutes.
    assert!(n
        .companion_reply("c1", "Mochi", "Late", &c, world.links(), WINDOW_TTL_MS)
        .is_none());
}

#[test]
fn companion_reply_in_short_mode_has_no_agent_text_and_no_watch_means_nothing() {
    let mut world = World::new();
    let n = Notifier::default();
    world.refs.open_window(&Target::Companion("c1".into()), 0);
    let short = TelegramConfig {
        short_mode: true,
        ..cfg()
    };
    let note = n
        .companion_reply("c1", "Mochi", "secret plans", &short, world.links(), 1)
        .expect("short");
    assert_eq!(
        text_of(&note),
        "Mochi replied. Open Puppet Master to read it."
    );
    let no_watch = TelegramConfig {
        caps: Caps::none().with(Cap::Answer, true),
        ..cfg()
    };
    assert!(n
        .companion_reply("c1", "Mochi", "hi", &no_watch, world.links(), 1)
        .is_none());
}

#[test]
fn turn_finished_goes_out_only_inside_the_panes_window() {
    let mut world = World::new();
    let n = Notifier::default();
    let c = cfg();
    assert!(n
        .turn_finished(
            "pane-1",
            "Claude Code",
            Some("auth refactor"),
            "Ran the tests, 42 passed.",
            &c,
            world.links(),
            0
        )
        .is_none());
    world.refs.open_window(&Target::Pane("pane-1".into()), 0);
    let note = n
        .turn_finished(
            "pane-1",
            "Claude Code",
            Some("auth refactor"),
            "Ran the tests, 42 passed.",
            &c,
            world.links(),
            1_000,
        )
        .expect("in window");
    assert_eq!(
        text_of(&note),
        "Done · auth refactor: Ran the tests, 42 passed."
    );
    assert_eq!(note.remember, Some(Target::Pane("pane-1".into())));
    // The same pane again inside 30 s: the gate holds it.
    assert!(n
        .turn_finished(
            "pane-1",
            "Claude Code",
            None,
            "Again.",
            &c,
            world.links(),
            2_000
        )
        .is_none());
}

#[test]
fn a_full_bucket_holds_urgent_messages_and_counts_them() {
    let mut world = World::new();
    let n = Notifier::default();
    let tiny = Bucket::new(1, 60_000);
    world.bucket = tiny;
    let c = candidate(Kind::Error, None, None);
    assert!(n.offer(&c, None, &cfg(), world.links(), 0).is_some());
    let second = candidate(Kind::TrustGate, None, None);
    let mut other = candidate(Kind::Consent, None, None);
    other.key = "pane-2".to_string();
    assert!(n
        .offer(&other, None, &cfg(), world.links(), 1_000)
        .is_none());
    assert!(n
        .offer(&second, None, &cfg(), world.links(), 2_000)
        .is_none());
    let summary = n
        .flush_held(world.links().outbound, 60_000)
        .expect("bucket refilled");
    assert_eq!(
        text_of(&summary),
        "2 more things need you. Send /status to see them."
    );
}

#[test]
fn no_secret_or_personal_path_reaches_the_chat() {
    let text = format!("Key {SECRET} for you@example.com in C:\\Users\\you\\secret\\notes.txt");
    // A fresh notifier and links for each message, so the 30 s gate never hides one.
    for view_cfg in [
        cfg(),
        TelegramConfig {
            short_mode: true,
            ..cfg()
        },
    ] {
        let n = Notifier::default();
        let mut world = World::new();
        let p = approval(&text, true, RiskLevel::Low, &text);
        let c = candidate(Kind::WaitingInput, Some(p), Some(item()));
        let note = n
            .offer(&c, None, &view_cfg, world.links(), 1)
            .expect("sent");
        assert_clean(&text_of(&note));
    }
    let n = Notifier::default();
    let mut world = World::new();
    let q = question(&[("c1", "Use the key")]);
    let mut c = candidate(
        Kind::Question,
        Some(PromptInfo {
            prompt_id: String::new(),
            text: text.clone(),
            actionable: false,
            risk: None,
        }),
        Some(item()),
    );
    c.heading = None;
    let note = n
        .offer(&c, Some(&q), &cfg(), world.links(), 1)
        .expect("sent");
    assert_clean(&text_of(&note));
    let n = Notifier::default();
    let mut world = World::new();
    let mut c = candidate(Kind::TrustGate, None, None);
    c.detail = Some(text.clone());
    let note = n.offer(&c, None, &cfg(), world.links(), 1).expect("sent");
    assert_clean(&text_of(&note));
}

#[test]
fn the_open_in_app_sentence_matches_the_policy_wording() {
    let refusal = answerable(Item::Approval, Some(Level::High));
    assert_eq!(refusal, Answerable::Never(OPEN_TO_ANSWER));
}

// Golden texts: what the chat reads, one per kind. Each fixture is the exact message (no buttons).
fn golden(note: &Note) -> String {
    text_of(note)
}

#[test]
fn golden_approval_low_risk() {
    let mut world = World::new();
    let n = Notifier::default();
    let p = approval("npm test", true, RiskLevel::Low, "Run a test command");
    let c = candidate(Kind::WaitingInput, Some(p), Some(item()));
    let note = n.offer(&c, None, &cfg(), world.links(), 1).expect("sent");
    assert_eq!(
        golden(&note),
        include_str!("golden/approval_low.txt").trim_end()
    );
}

#[test]
fn golden_approval_high_risk() {
    let mut world = World::new();
    let n = Notifier::default();
    let p = approval(
        "rm -rf ../old",
        false,
        RiskLevel::High,
        "Delete a folder outside this project",
    );
    let c = candidate(Kind::WaitingInput, Some(p), Some(item()));
    let note = n.offer(&c, None, &cfg(), world.links(), 1).expect("sent");
    assert_eq!(
        golden(&note),
        include_str!("golden/approval_high.txt").trim_end()
    );
}

#[test]
fn golden_approval_short_mode() {
    let mut world = World::new();
    let n = Notifier::default();
    let short = TelegramConfig {
        short_mode: true,
        ..cfg()
    };
    let p = approval("npm test", true, RiskLevel::Low, "Run a test command");
    let c = candidate(Kind::WaitingInput, Some(p), Some(item()));
    let note = n.offer(&c, None, &short, world.links(), 1).expect("sent");
    assert_eq!(
        golden(&note),
        include_str!("golden/approval_short.txt").trim_end()
    );
}

#[test]
fn golden_trust_gate() {
    let mut world = World::new();
    let n = Notifier::default();
    let mut c = candidate(Kind::TrustGate, None, Some(item()));
    c.detail = Some("shop".to_string());
    let note = n.offer(&c, None, &cfg(), world.links(), 1).expect("sent");
    assert_eq!(
        golden(&note),
        include_str!("golden/trust_gate.txt").trim_end()
    );
}

#[test]
fn golden_consent() {
    let mut world = World::new();
    let n = Notifier::default();
    let mut c = candidate(Kind::Consent, None, Some(item()));
    c.detail = Some("Claude Code wants to control shop".to_string());
    let note = n.offer(&c, None, &cfg(), world.links(), 1).expect("sent");
    assert_eq!(golden(&note), include_str!("golden/consent.txt").trim_end());
}

#[test]
fn golden_budget() {
    let mut world = World::new();
    let n = Notifier::default();
    let mut c = candidate(Kind::Budget, None, Some(item()));
    c.detail = Some("20 of 20 turns".to_string());
    let note = n.offer(&c, None, &cfg(), world.links(), 1).expect("sent");
    assert_eq!(golden(&note), include_str!("golden/budget.txt").trim_end());
}

#[test]
fn golden_limit() {
    let mut world = World::new();
    let n = Notifier::default();
    let mut c = candidate(Kind::Limit, None, Some(item()));
    c.detail = Some("9 pm".to_string());
    let note = n.offer(&c, None, &cfg(), world.links(), 1).expect("sent");
    assert_eq!(golden(&note), include_str!("golden/limit.txt").trim_end());
}

#[test]
fn golden_error() {
    let mut world = World::new();
    let n = Notifier::default();
    let c = candidate(Kind::Error, None, None);
    let note = n.offer(&c, None, &cfg(), world.links(), 1).expect("sent");
    assert_eq!(golden(&note), include_str!("golden/error.txt").trim_end());
}

#[test]
fn golden_question() {
    let mut world = World::new();
    let n = Notifier::default();
    let q = question(&[("main", "main"), ("dev", "dev branch")]);
    let mut c = candidate(
        Kind::Question,
        Some(PromptInfo {
            prompt_id: String::new(),
            text: q.question.clone(),
            actionable: false,
            risk: None,
        }),
        Some(item()),
    );
    c.heading = None;
    let note = n
        .offer(&c, Some(&q), &cfg(), world.links(), 1)
        .expect("sent");
    assert_eq!(
        golden(&note),
        include_str!("golden/question.txt").trim_end()
    );
    assert_eq!(note.out.keyboard.len(), 2);
}

#[test]
fn golden_run_finished() {
    let mut world = World::new();
    let n = Notifier::default();
    let runs = TelegramConfig {
        notify_runs: true,
        ..cfg()
    };
    let c = candidate(Kind::RunFinished, None, None);
    let note = n.offer(&c, None, &runs, world.links(), 1).expect("sent");
    assert_eq!(
        golden(&note),
        include_str!("golden/run_finished.txt").trim_end()
    );
}

#[test]
fn golden_companion_reply() {
    let mut world = World::new();
    let n = Notifier::default();
    world.refs.open_window(&Target::Companion("c1".into()), 0);
    let note = n
        .companion_reply(
            "c1",
            "Mochi",
            "The build is green. 42 passed.",
            &cfg(),
            world.links(),
            1,
        )
        .expect("sent");
    assert_eq!(
        golden(&note),
        include_str!("golden/companion_reply.txt").trim_end()
    );
}

#[test]
fn golden_turn_done() {
    let mut world = World::new();
    let n = Notifier::default();
    world.refs.open_window(&Target::Pane("pane-1".into()), 0);
    let note = n
        .turn_finished(
            "pane-1",
            "Claude Code",
            Some("auth refactor"),
            "Ran the tests, 42 passed.",
            &cfg(),
            world.links(),
            1,
        )
        .expect("sent");
    assert_eq!(
        golden(&note),
        include_str!("golden/turn_done.txt").trim_end()
    );
}

#[test]
fn a_window_does_not_last_for_an_hour() {
    let mut world = World::new();
    world.refs.open_window(&Target::Companion("c1".into()), 0);
    let n = Notifier::default();
    assert!(n
        .companion_reply("c1", "Mochi", "late", &cfg(), world.links(), HOUR_MS)
        .is_none());
}
