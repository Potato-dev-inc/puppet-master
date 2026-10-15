//! The store with a virtual clock: order, identity, occurrences, tombstones, expiry, caps.

use crate::attention::model::*;
use crate::attention::store::{item_id, PromptRec, Store};

fn pane(id: &str, status: &str) -> PaneFact {
    PaneFact {
        id: id.into(),
        status: status.into(),
        heading: Some(format!("heading {id}")),
        agent_label: Some("Claude Code".into()),
        project: Some("shop".into()),
    }
}

fn facts(panes: Vec<PaneFact>) -> Facts {
    Facts { panes, ..Facts::default() }
}

fn rec(prompt_id: &str, level: &str) -> PromptRec {
    PromptRec {
        prompt_id: Some(prompt_id.into()),
        kind: Some("command".into()),
        summary: Some("Run command: ls".into()),
        risk_level: Some(level.into()),
        headline: Some("Read files".into()),
        ..PromptRec::default()
    }
}

fn open_keys(store: &Store) -> Vec<String> {
    store.snapshot().items.iter().filter(|i| i.state == State::Open).map(|i| i.key.clone()).collect()
}

#[test]
fn blocking_items_are_ordered_by_rank_then_age_then_id() {
    let mut store = Store::default();
    store.note_status("low", "waiting_input", 3_000);
    store.note_status("high", "waiting_input", 9_000);
    store.note_status("old", "waiting_input", 1_000);
    store.note_status("err", "error", 500);
    store.on_prompt("low", true, Some(rec("a", "low")));
    store.on_prompt("high", true, Some(rec("b", "high")));
    store.on_prompt("old", true, Some(rec("c", "medium")));
    let mut f = facts(vec![pane("low", "waiting_input"), pane("high", "waiting_input"), pane("old", "waiting_input"), pane("err", "error")]);
    f.questions = vec![QuestionFact { id: "q".into(), created_at_ms: Some(100), ..QuestionFact::default() }];
    assert!(store.recompute(&f, 10_000));
    // High risk first (rank 0), then the medium one (rank 1), then the question and the low-risk
    // approval (rank 2, oldest first: the question is older), then the error (rank 4).
    assert_eq!(open_keys(&store), ["approval:high", "approval:old", "question:q", "approval:low", "error:err"]);
}

#[test]
fn counts_only_blocking_open_items() {
    let mut store = Store::default();
    let mut f = facts(vec![pane("a", "waiting_input"), pane("b", "error")]);
    f.questions = vec![QuestionFact { id: "q".into(), ..QuestionFact::default() }];
    store.recompute(&f, 1_000);
    assert_eq!(store.counts(), Counts { blocking: 3, ready: 0 });
}

#[test]
fn the_revision_only_moves_when_the_snapshot_changes() {
    let mut store = Store::default();
    let f = facts(vec![pane("a", "waiting_input")]);
    assert!(store.recompute(&f, 1_000));
    let rev = store.snapshot().rev;
    assert_eq!(rev, 1);
    assert!(!store.recompute(&f, 1_050), "same facts, same snapshot");
    assert_eq!(store.snapshot().rev, rev);
    assert!(store.recompute(&facts(vec![pane("a", "waiting_input"), pane("b", "error")]), 1_100));
    assert_eq!(store.snapshot().rev, rev + 1);
}

#[test]
fn the_same_command_asked_twice_is_two_occurrences_with_two_ids() {
    let mut store = Store::default();
    let f = facts(vec![pane("a", "waiting_input")]);
    store.on_prompt("a", true, Some(rec("same-hash", "low")));
    store.recompute(&f, 1_000);
    let first = store.snapshot().items[0].clone();
    assert_eq!(first.prompt.as_ref().unwrap().occurrence, 1);
    // The pane answers, runs on, then asks the identical question.
    store.on_prompt("a", false, None);
    store.recompute(&facts(vec![pane("a", "running")]), 2_000);
    store.on_prompt("a", true, Some(rec("same-hash", "low")));
    store.recompute(&f, 3_000);
    let open: Vec<&Item> = store.snapshot().items.iter().filter(|i| i.state == State::Open).collect();
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].prompt.as_ref().unwrap().occurrence, 2);
    assert_ne!(open[0].id, first.id, "an old answer must not land on the new question");
    assert_eq!(first.id, item_id(Kind::Approval, "a", 1));
}

#[test]
fn an_item_that_leaves_becomes_a_tombstone_for_fifteen_seconds_and_never_counts() {
    let mut store = Store::default();
    store.on_prompt("a", true, Some(rec("p", "low")));
    store.recompute(&facts(vec![pane("a", "waiting_input")]), 1_000);
    assert_eq!(store.counts().blocking, 1);
    store.note_status("a", "running", 5_000);
    store.on_prompt("a", false, None);
    store.recompute(&facts(vec![pane("a", "running")]), 5_000);
    assert_eq!(store.counts().blocking, 0);
    let tomb = &store.snapshot().items[0];
    assert_eq!(tomb.state, State::Resolved);
    assert_eq!(tomb.resolution.as_ref().unwrap().state, "gone");
    assert_eq!(tomb.resolution.as_ref().unwrap().reason.as_deref(), Some("moved_on"));
    assert_eq!(store.next_deadline(), Some(5_000 + TOMBSTONE_MS));
    store.recompute(&facts(vec![pane("a", "running")]), 5_000 + TOMBSTONE_MS - 1);
    assert_eq!(store.snapshot().items.len(), 1, "still there just before the deadline");
    store.recompute(&facts(vec![pane("a", "running")]), 5_000 + TOMBSTONE_MS);
    assert!(store.snapshot().items.is_empty());
}

#[test]
fn a_closed_pane_says_pane_closed_and_an_answer_note_says_who_answered() {
    let mut store = Store::default();
    store.on_prompt("a", true, Some(rec("p", "low")));
    store.on_prompt("b", true, Some(rec("q", "low")));
    store.recompute(&facts(vec![pane("a", "waiting_input"), pane("b", "waiting_input")]), 1_000);
    store.note_resolution(
        "approval:b",
        Resolution { state: "answered".into(), by: Some("phone".into()), choice_label: Some("Allow once".into()), at_ms: 2_000, ..Resolution::default() },
    );
    store.on_prompt("b", false, None);
    store.recompute(&facts(vec![pane("b", "running")]), 2_000);
    let tombs: Vec<&Item> = store.snapshot().items.iter().filter(|i| i.state == State::Resolved).collect();
    assert_eq!(tombs.len(), 2);
    let a = tombs.iter().find(|t| t.key == "approval:a").unwrap();
    assert_eq!(a.resolution.as_ref().unwrap().reason.as_deref(), Some("pane_closed"));
    let b = tombs.iter().find(|t| t.key == "approval:b").unwrap();
    assert_eq!(b.resolution.as_ref().unwrap().by.as_deref(), Some("phone"));
    assert_eq!(b.resolution.as_ref().unwrap().choice_label.as_deref(), Some("Allow once"));
}

#[test]
fn a_question_expires_at_its_deadline_with_an_expired_tombstone() {
    let mut store = Store::default();
    let mut f = facts(vec![]);
    f.questions = vec![QuestionFact {
        id: "q".into(),
        expires_at_ms: Some(10_000),
        text: Some("Which database?".into()),
        ..QuestionFact::default()
    }];
    store.recompute(&f, 1_000);
    assert_eq!(store.snapshot().items[0].title, "Which database?");
    assert_eq!(store.next_deadline(), Some(10_000));
    // The question store stops listing it once expired.
    store.recompute(&facts(vec![]), 10_000);
    let tomb = &store.snapshot().items[0];
    assert_eq!(tomb.resolution.as_ref().unwrap().state, "expired");
    assert_eq!(store.counts().blocking, 0);
}

#[test]
fn a_pane_has_one_pane_state_item_trust_gate_then_approval_then_error_then_stuck() {
    let mut store = Store::default();
    // An errored pane that the loop guard also flags: one problem, one item.
    let mut f = facts(vec![pane("e", "error"), pane("w", "waiting_input"), pane("s", "running")]);
    f.stuck_pane_ids = vec!["e".into(), "w".into(), "s".into()];
    store.recompute(&f, 1_000);
    let mut keys = open_keys(&store);
    keys.sort();
    assert_eq!(keys, ["approval:w", "error:e", "stuck:s"]);
    assert_eq!(store.counts().blocking, 3);
}

#[test]
fn a_question_about_a_pane_is_its_own_item_next_to_the_pane_state() {
    let mut store = Store::default();
    let mut f = facts(vec![pane("a", "waiting_input")]);
    f.questions = vec![QuestionFact { id: "q".into(), pane_id: Some("a".into()), ..QuestionFact::default() }];
    store.recompute(&f, 1_000);
    assert_eq!(store.counts().blocking, 2);
}

#[test]
fn a_folder_trust_screen_is_a_trust_gate_with_its_own_words_and_leads_the_list() {
    let mut store = Store::default();
    store.on_prompt(
        "t",
        true,
        Some(PromptRec {
            prompt_id: Some("pt".into()),
            kind: Some("workspace_trust".into()),
            choices: vec![ChoiceFact { id: "allow_once".into(), label: "Trust this folder".into(), broad: false }],
            ..PromptRec::default()
        }),
    );
    store.on_prompt("l", true, Some(rec("pl", "low")));
    store.on_prompt("si", true, Some(PromptRec { prompt_id: Some("ps".into()), kind: Some("startup_screen".into()), ..PromptRec::default() }));
    store.recompute(&facts(vec![pane("l", "waiting_input"), pane("t", "waiting_input"), pane("si", "waiting_input")]), 1_000);
    let items = &store.snapshot().items;
    let trust = items.iter().find(|i| i.pane.as_ref().unwrap().id == "t").unwrap();
    assert_eq!(trust.kind, Kind::TrustGate);
    assert_eq!(trust.rank, 0);
    assert_eq!(trust.title, "Trust this folder?");
    assert_eq!(trust.sentence.as_deref(), Some("Claude Code wants to work in shop. It can read and change files there."));
    assert_eq!(trust.key, "approval:t", "clients that match on the old key keep working");
    let signin = items.iter().find(|i| i.pane.as_ref().unwrap().id == "si").unwrap();
    assert_eq!(signin.kind, Kind::TrustGate);
    assert_eq!(signin.title, "Claude Code needs you on your PC", "a sign-in screen has no choices to offer");
    assert!(signin.prompt.as_ref().unwrap().choices.is_empty());
    assert!(items.iter().position(|i| i.id == trust.id).unwrap() < items.iter().position(|i| i.pane.as_ref().unwrap().id == "l").unwrap());
    assert_eq!(store.counts().blocking, 3);
}

fn raised(key: &str, kind: Kind) -> Raised {
    Raised { key: key.into(), kind, pane_id: None, title: "x".into(), sentence: None, expires_at_ms: None, actions: vec![] }
}

#[test]
fn raising_the_same_key_again_updates_in_place_and_keeps_since() {
    let mut store = Store::default();
    store.raise(Raised { sentence: Some("20 of 20 turns".into()), ..raised("budget:run1", Kind::Budget) }, 1_000);
    store.recompute(&facts(vec![]), 1_000);
    store.raise(Raised { sentence: Some("25 of 25 turns".into()), ..raised("budget:run1", Kind::Budget) }, 9_000);
    store.recompute(&facts(vec![]), 9_000);
    let items = &store.snapshot().items;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].since_ms, Some(1_000));
    assert_eq!(items[0].sentence.as_deref(), Some("25 of 25 turns"));
    assert_eq!(items[0].lane, Lane::Blocking);
    assert!(store.clear("budget:run1", None));
    store.recompute(&facts(vec![]), 9_500);
    assert_eq!(store.counts().blocking, 0);
}

#[test]
fn a_consent_request_nobody_answers_expires_after_ten_minutes() {
    let mut store = Store::default();
    store.raise(raised("consent:r1", Kind::Consent), 1_000);
    store.recompute(&facts(vec![]), 1_000);
    assert_eq!(store.counts().blocking, 1);
    assert_eq!(store.snapshot().items[0].rank, 0, "consent leads");
    store.recompute(&facts(vec![]), 1_000 + CONSENT_TTL_MS);
    assert_eq!(store.counts().blocking, 0);
    assert_eq!(store.snapshot().items[0].resolution.as_ref().unwrap().state, "expired");
}

#[test]
fn a_done_item_is_ready_not_counted_and_expires_after_a_day() {
    let mut store = Store::default();
    store.raise(raised("done:p1", Kind::Done), 1_000);
    store.recompute(&facts(vec![]), 1_000);
    assert_eq!(store.counts(), Counts { blocking: 0, ready: 1 });
    store.recompute(&facts(vec![]), 1_000 + DONE_TTL_MS);
    assert_eq!(store.counts(), Counts { blocking: 0, ready: 0 });
}

#[test]
fn the_cap_drops_the_oldest_ready_items_and_never_a_blocking_one() {
    let mut store = Store::default();
    for i in 0..(MAX_ITEMS + 20) {
        let kind = if i % 2 == 0 { Kind::Done } else { Kind::Budget };
        store.raise(raised(&format!("k{i}"), kind), 1_000 + i as u64);
    }
    store.recompute(&facts(vec![]), 5_000);
    let items = &store.snapshot().items;
    assert_eq!(items.len(), MAX_ITEMS);
    assert_eq!(items.iter().filter(|i| i.lane == Lane::Blocking).count(), (MAX_ITEMS + 20) / 2, "no blocking item was dropped");
}

#[test]
fn item_ids_are_opaque_stable_and_carry_no_pane_id() {
    let id = item_id(Kind::Approval, "pane-123", 1);
    assert_eq!(id.len(), 10);
    assert!(!id.contains("pane"));
    assert_eq!(id, item_id(Kind::Approval, "pane-123", 1));
    assert_ne!(id, item_id(Kind::Approval, "pane-123", 2));
    assert!(id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
}

#[test]
fn the_snapshot_serialises_with_the_design_shape() {
    let mut store = Store::default();
    store.on_prompt("a", true, Some(rec("p1", "high")));
    store.recompute(&facts(vec![pane("a", "waiting_input")]), 1_000);
    let json = serde_json::to_value(store.snapshot()).unwrap();
    assert_eq!(json["v"], 1);
    assert_eq!(json["counts"]["blocking"], 1);
    let item = &json["items"][0];
    assert_eq!(item["kind"], "approval");
    assert_eq!(item["lane"], "blocking");
    assert_eq!(item["rank"], 0);
    assert_eq!(item["state"], "open");
    assert_eq!(item["risk"]["word"], "High risk");
    assert_eq!(item["pane"]["heading"], "heading a");
    assert_eq!(item["prompt"]["occurrence"], 1);
    assert!(item["resolution"].is_null());
}

#[test]
fn a_budget_pause_card_is_a_budget_item_with_the_cards_own_line() {
    let mut store = Store::default();
    let mut f = facts(vec![pane("a", "idle")]);
    f.questions = vec![QuestionFact {
        id: "b".into(),
        pane_id: Some("a".into()),
        expires_at_ms: Some(50_000),
        text: Some("Claude Code used its 20 turns.".into()),
        kind: Some("budget".into()),
        ..QuestionFact::default()
    }];
    store.recompute(&f, 1_000);
    let item = &store.snapshot().items[0];
    assert_eq!(item.key, "budget:b");
    assert_eq!(item.kind, Kind::Budget);
    assert_eq!(item.lane, Lane::Blocking);
    assert_eq!(item.title, "Budget used up");
    assert_eq!(item.sentence.as_deref(), Some("Claude Code used its 20 turns."));
    assert_eq!(item.expires_at_ms, Some(50_000));
    assert!(item.actions.is_empty(), "no lock-screen action for a budget card");
    assert_eq!(store.counts().blocking, 1);
}

#[test]
fn a_usage_limit_card_is_a_limit_item_with_the_cards_own_line() {
    let mut store = Store::default();
    let mut f = facts(vec![pane("a", "idle")]);
    f.questions = vec![QuestionFact {
        id: "l".into(),
        pane_id: Some("a".into()),
        expires_at_ms: Some(80_000),
        text: Some("Claude Code ran out of usage. It can continue at 5:00 PM.".into()),
        limit: Some(serde_json::json!({ "kind": "usage_limit", "agent_label": "Claude Code" })),
        ..QuestionFact::default()
    }];
    store.recompute(&f, 1_000);
    let item = &store.snapshot().items[0];
    assert_eq!(item.key, "limit:l");
    assert_eq!(item.kind, Kind::Limit);
    assert_eq!(item.lane, Lane::Blocking);
    assert_eq!(item.title, "Usage limit reached");
    assert_eq!(item.sentence.as_deref(), Some("Claude Code ran out of usage. It can continue at 5:00 PM."));
    assert_eq!(item.expires_at_ms, Some(80_000));
    assert!(item.actions.is_empty(), "no lock-screen action for a usage-limit card");
    assert_eq!(store.counts().blocking, 1);
}

fn command_rec(prompt_id: &str, level: &str) -> PromptRec {
    PromptRec {
        choices: vec![
            ChoiceFact { id: "allow_once".into(), label: "Allow once".into(), broad: false },
            ChoiceFact { id: "allow_all".into(), label: "Allow all commands".into(), broad: true },
            ChoiceFact { id: "deny".into(), label: "Deny".into(), broad: false },
        ],
        reasons: vec!["It deletes ../build-cache.".into()],
        ..rec(prompt_id, level)
    }
}

#[test]
fn the_card_carries_what_an_answer_checks_against_the_live_screen() {
    let mut store = Store::default();
    store.on_prompt("a", true, Some(command_rec("p1", "high")));
    store.recompute(&facts(vec![pane("a", "waiting_input")]), 1_000);
    let item = &store.snapshot().items[0];
    let prompt = item.prompt.as_ref().unwrap();
    // The fingerprint is exactly what the live screen is compared with at answer time.
    assert_eq!(prompt.fingerprint, command_rec("p1", "high").fingerprint(1));
    assert_ne!(prompt.fingerprint, command_rec("p1", "low").fingerprint(1), "the risk read is part of it");
    assert_ne!(prompt.fingerprint, command_rec("p1", "high").fingerprint(2), "so is the occurrence");
    assert_eq!(prompt.provenance, "screen");
    // High risk: Deny leads and is focused; the broad answer waits behind More options.
    assert_eq!(prompt.layout.primary.as_deref(), Some("deny"));
    assert_eq!(prompt.layout.focus.as_deref(), Some("deny"));
    assert_eq!(prompt.layout.more, ["allow_all"]);
    assert_eq!(item.risk.as_ref().unwrap().reasons, ["It deletes ../build-cache."]);
    let json = serde_json::to_value(store.snapshot()).unwrap();
    assert_eq!(json["items"][0]["prompt"]["layout"]["primary"], "deny");
    assert_eq!(json["items"][0]["prompt"]["fingerprint"].as_str().unwrap().len(), 16);
}

#[test]
fn a_calm_prompt_leads_with_the_agents_first_choice() {
    let mut store = Store::default();
    store.on_prompt("a", true, Some(command_rec("p1", "low")));
    store.recompute(&facts(vec![pane("a", "waiting_input")]), 1_000);
    let layout = &store.snapshot().items[0].prompt.as_ref().unwrap().layout;
    assert_eq!((layout.primary.as_deref(), layout.focus.as_deref(), layout.more.len()), (Some("allow_once"), None, 0));
}

#[test]
fn the_same_command_asked_again_has_a_new_fingerprint() {
    let mut store = Store::default();
    let f = facts(vec![pane("a", "waiting_input")]);
    store.on_prompt("a", true, Some(command_rec("same", "low")));
    store.recompute(&f, 1_000);
    let first = store.snapshot().items[0].prompt.as_ref().unwrap().fingerprint.clone();
    store.on_prompt("a", false, None);
    store.on_prompt("a", true, Some(command_rec("same", "low")));
    store.recompute(&f, 2_000);
    let second = store.snapshot().items[0].prompt.as_ref().unwrap().fingerprint.clone();
    assert_ne!(first, second, "an answer to the first card can never land on the second");
    assert_eq!(store.occurrence("a"), 2);
}
