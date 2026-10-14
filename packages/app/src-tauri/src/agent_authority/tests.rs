//! Agent authority, driven the way the bridge drives it: a mock MCP session (a unique connection
//! id) calls the gate; a "person" answers through the questions store, as the Inbox does.

use super::config::{with_config, Config};
use super::consent::{self, CHOICE_ALWAYS, CHOICE_DENY, CHOICE_ONCE};
use super::gate::{check_answer, check_take_over};
use crate::mcp_sessions::{self, Relation};
use crate::questions::{self, now_ms, Question, QuestionState};
use serde_json::{json, Value};

const TODAY: Config = Config { ask_before_agent_control: false, agents_may_merge: true };
const ASKING: Config = Config { ask_before_agent_control: true, agents_may_merge: true };

fn id(tag: &str) -> String {
    format!("{tag}-{}", uuid::Uuid::new_v4())
}

/// The consent card an agent's refused call raised.
fn card(consent_id: &str) -> Question {
    questions::store().get(consent_id, now_ms()).expect("the card is in the store")
}

fn consent_id_of(error: &crate::operations::OperationError) -> String {
    error.context["consent_id"].as_str().expect("CONSENT_PENDING names its card").to_string()
}

fn person_answers(consent_id: &str, choice: &str) {
    let resolution = questions::store().answer(consent_id, Some(choice), None, "desktop", now_ms()).unwrap();
    assert!(resolution.accepted);
}

/// Audit lines about one pane written by this test run (the log is shared by parallel tests).
fn audit_lines_for(pane: &str) -> Vec<crate::audit::Entry> {
    crate::audit::global().recent(500).into_iter().filter(|e| e.detail["pane_id"] == json!(pane)).collect()
}

// ---- A on: adopting a pane the person opened ----

#[test]
fn adopting_a_user_pane_with_a_on_raises_a_card_and_the_call_says_consent_pending() {
    let (agent, pane) = (id("claude-session"), id("user-pane"));
    with_config(ASKING, || {
        let error = check_take_over(&agent, Some(&pane), None, None).unwrap_err();
        assert_eq!(error.code, "CONSENT_PENDING");
        assert!(error.recoverable, "the agent may ask again");
        assert!(error.message.contains("A person has to say yes first"), "{}", error.message);
        assert!(error.message.contains("run_agent"), "the message offers what to do instead: {}", error.message);
        assert_eq!(error.retry_after_ms, Some(10_000));
        assert_eq!(error.context["retry_after_ms"], 10_000);

        // The card is an ordinary question with consent details, high urgency, no free text.
        let id = consent_id_of(&error);
        let q = card(&id);
        assert_eq!(q.state, QuestionState::Pending);
        assert_eq!(q.question, "Allow another agent to control that pane?", "no connection ids or pane ids in the words");
        assert_eq!(q.urgency, "high");
        assert!(!q.allow_free_text);
        let labels: Vec<&str> = q.choices.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, ["Allow once", "Always for this pane", "Deny"]);
        let info = q.consent.expect("marked as a consent card");
        assert_eq!((info.action.as_str(), info.pane_id.as_deref()), ("take_over", Some(pane.as_str())));
        assert!(q.pane_id.is_none(), "the target pane is not the one waiting");

        // It goes through the push pipeline as a question: opens the app, no Allow button.
        assert!(crate::push::NOTIFIED_QUESTIONS.lock().contains(&id));

        // Asking again does not stack a second card.
        let again = check_take_over(&agent, Some(&pane), None, None).unwrap_err();
        assert_eq!(consent_id_of(&again), id);
    });
}

#[test]
fn allow_once_lets_exactly_one_call_through() {
    let (agent, pane) = (id("s"), id("user-pane"));
    with_config(ASKING, || {
        let first = consent_id_of(&check_take_over(&agent, Some(&pane), None, None).unwrap_err());
        person_answers(&first, CHOICE_ONCE);
        check_take_over(&agent, Some(&pane), None, None).expect("the person allowed it");
        // The permit is used up. If the take-over did not happen, the next call is a new question.
        let second = check_take_over(&agent, Some(&pane), None, None).unwrap_err();
        assert_eq!(second.code, "CONSENT_PENDING");
        assert_ne!(consent_id_of(&second), first, "a used-up permit asks again");
    });
}

#[test]
fn deny_stops_the_agent_and_does_not_stack_new_cards() {
    let (agent, pane) = (id("s"), id("user-pane"));
    with_config(ASKING, || {
        let card_id = consent_id_of(&check_take_over(&agent, Some(&pane), None, None).unwrap_err());
        person_answers(&card_id, CHOICE_DENY);
        for _ in 0..3 {
            let error = check_take_over(&agent, Some(&pane), None, None).unwrap_err();
            assert_eq!(error.code, "CONSENT_DENIED");
            assert!(!error.recoverable);
            assert!(error.message.contains("The person said no"));
        }
        let pending: Vec<_> = questions::store().list_pending(now_ms()).into_iter().filter(|q| q.consent.as_ref().and_then(|c| c.pane_id.as_deref()) == Some(pane.as_str())).collect();
        assert!(pending.is_empty(), "no new card after a no");
    });
}

#[test]
fn dismissing_the_card_counts_as_no() {
    let (agent, pane) = (id("s"), id("user-pane"));
    with_config(ASKING, || {
        let card_id = consent_id_of(&check_take_over(&agent, Some(&pane), None, None).unwrap_err());
        assert!(questions::store().dismiss(&card_id, now_ms()).unwrap().accepted);
        assert_eq!(check_take_over(&agent, Some(&pane), None, None).unwrap_err().code, "CONSENT_DENIED");
    });
}

/// Review R12 3.1: "always" was keyed by the pane alone, so after the person allowed agent A, any
/// other agent B (or a prompt-injected helper of someone else) took the pane over with no card.
#[test]
fn always_for_this_pane_covers_the_agent_that_asked_and_its_helpers_on_that_pane_only() {
    let (a, b, helper, pane, other_pane) = (id("a"), id("b"), id("a-helper"), id("user-pane"), id("user-pane"));
    // A is a known connection with a helper the bridge linked to it.
    mcp_sessions::register_owned_pane(&a, &id("a-own-pane")).unwrap();
    mcp_sessions::link_child_session(&helper, &a).unwrap();
    with_config(ASKING, || {
        let card_id = consent_id_of(&check_take_over(&a, Some(&pane), None, None).unwrap_err());
        person_answers(&card_id, CHOICE_ALWAYS);
        check_take_over(&a, Some(&pane), None, None).expect("always");
        check_take_over(&a, Some(&pane), None, None).expect("always means every time");
        assert!(consent::always_allowed(&a, &pane));
        check_take_over(&helper, Some(&pane), None, None).expect("a helper of A is in A's lineage");

        let from_b = check_take_over(&b, Some(&pane), None, None).unwrap_err();
        assert_eq!(from_b.code, "CONSENT_PENDING", "another agent gets its own card, not A's grant");
        assert_ne!(consent_id_of(&from_b), card_id);
        assert!(!consent::always_allowed(&b, &pane));
        assert_eq!(check_take_over(&a, Some(&other_pane), None, None).unwrap_err().code, "CONSENT_PENDING", "another pane still asks");
    });
}

/// The grant written when the card is answered (`on_resolved`, before the agent calls again) is
/// the asking agent's too.
#[test]
fn always_recorded_at_answer_time_is_the_asking_agents_only() {
    let (a, b, pane) = (id("a"), id("b"), id("user-pane"));
    with_config(ASKING, || {
        let card_id = consent_id_of(&check_take_over(&a, Some(&pane), None, None).unwrap_err());
        person_answers(&card_id, CHOICE_ALWAYS);
        assert!(consent::always_allowed(&a, &pane), "recorded when the person answered");
        assert!(!consent::always_allowed(&b, &pane));
        assert_eq!(check_take_over(&b, Some(&pane), None, None).unwrap_err().code, "CONSENT_PENDING");
    });
}

// ---- A off: today's behaviour ----

#[test]
fn a_off_changes_nothing_no_card_no_refusal() {
    let (agent, pane) = (id("s"), id("user-pane"));
    with_config(TODAY, || {
        check_take_over(&agent, Some(&pane), None, None).expect("today an agent may take any pane with grant=true");
        let cards: Vec<_> = questions::store().list_pending(now_ms()).into_iter().filter(|q| q.consent.as_ref().and_then(|c| c.pane_id.as_deref()) == Some(pane.as_str())).collect();
        assert!(cards.is_empty(), "no card raised");
        // It is still written down: that is the audit log's take_over line.
        let lines = audit_lines_for(&pane);
        assert!(lines.iter().any(|e| e.kind == "take_over" && e.who == "agent" && e.summary.contains("setting is off")), "{lines:?}");
    });
}

// ---- lineage ----

#[test]
fn a_pane_the_agent_started_is_its_own_and_never_asks() {
    let (agent, pane) = (id("orchestrator"), id("pane"));
    mcp_sessions::register_owned_pane(&agent, &pane).unwrap();
    assert_eq!(mcp_sessions::lineage_of_pane(&agent, &pane), Relation::Own);
    with_config(ASKING, || {
        check_take_over(&agent, Some(&pane), None, None).expect("own pane");
        assert!(audit_lines_for(&pane).is_empty(), "an agent working with its own helpers is not an event");
    });
}

#[test]
fn a_helpers_pane_is_in_the_parents_lineage_but_not_the_other_way() {
    let (parent, child, stranger) = (id("parent"), id("child"), id("stranger"));
    let (parent_pane, child_pane) = (id("pane"), id("pane"));
    mcp_sessions::session_identity(&parent);
    mcp_sessions::session_identity(&child);
    mcp_sessions::session_identity(&stranger);
    mcp_sessions::link_child_session(&child, &parent).unwrap();
    mcp_sessions::register_owned_pane(&parent, &parent_pane).unwrap();
    mcp_sessions::register_owned_pane(&child, &child_pane).unwrap();

    assert_eq!(mcp_sessions::lineage_of_pane(&parent, &child_pane), Relation::Own, "the parent may control its helper's panes");
    assert_eq!(mcp_sessions::lineage_of_pane(&child, &parent_pane), Relation::OtherAgent, "a helper does not own its parent's panes");
    assert_eq!(mcp_sessions::lineage_of_pane(&stranger, &child_pane), Relation::OtherAgent);

    with_config(ASKING, || {
        check_take_over(&parent, Some(&child_pane), None, None).expect("descendant's pane");
        assert_eq!(check_take_over(&child, Some(&parent_pane), None, None).unwrap_err().code, "CONSENT_PENDING");
        assert_eq!(check_take_over(&stranger, Some(&child_pane), None, None).unwrap_err().code, "CONSENT_PENDING");
    });
}

#[test]
fn a_helper_link_cannot_make_a_loop_or_point_at_nothing() {
    let (a, b) = (id("a"), id("b"));
    mcp_sessions::session_identity(&a);
    mcp_sessions::session_identity(&b);
    assert!(mcp_sessions::link_child_session(&a, &a).is_err());
    assert!(mcp_sessions::link_child_session(&a, &id("unknown")).is_err());
    mcp_sessions::link_child_session(&b, &a).unwrap();
    assert!(mcp_sessions::link_child_session(&a, &b).is_err(), "a loop");
}

#[test]
fn a_pane_the_person_opened_is_in_nobodys_lineage_even_after_an_agent_adopted_it() {
    let (agent, pane) = (id("s"), id("user-pane"));
    mcp_sessions::take_over_pane(&agent, &pane, true).unwrap();
    assert_eq!(mcp_sessions::relation_to_pane(&agent, &pane), Relation::Controls);
    assert_eq!(mcp_sessions::lineage_of_pane(&agent, &pane), Relation::UserOpened);
}

#[test]
fn another_agents_pane_asks_too() {
    let (owner, other, pane) = (id("owner"), id("other"), id("pane"));
    mcp_sessions::register_owned_pane(&owner, &pane).unwrap();
    with_config(ASKING, || {
        assert_eq!(check_take_over(&other, Some(&pane), None, None).unwrap_err().code, "CONSENT_PENDING");
    });
}

// ---- answering prompts ----

#[test]
fn an_agent_answering_above_low_risk_outside_its_lineage_needs_a_card() {
    let (agent, pane) = (id("s"), id("user-pane"));
    with_config(ASKING, || {
        // Low risk goes through.
        check_answer(Some(&agent), &pane, "p1", Some("low"), None, None).expect("low risk");
        // Medium and high, and a prompt with no known risk, ask.
        for (n, level) in [Some("medium"), Some("high"), None].into_iter().enumerate() {
            let prompt = format!("prompt-{n}");
            let error = check_answer(Some(&agent), &pane, &prompt, level, Some("Delete a folder"), None).unwrap_err();
            assert_eq!(error.code, "CONSENT_PENDING", "{level:?}");
            let q = card(&consent_id_of(&error));
            let words: Vec<&str> = q.choices.iter().map(|c| c.label.as_str()).collect();
            assert_eq!(words, ["Allow once", "Deny"], "no 'always' for answering");
            assert!(q.context.as_deref().is_some_and(|c| c.contains("Delete a folder")));
            assert_eq!(q.consent.unwrap().action, "answer_prompt");
        }
        // The person allows prompt-0 once: that prompt only.
        let first = check_answer(Some(&agent), &pane, "prompt-0", Some("medium"), None, None).unwrap_err();
        person_answers(&consent_id_of(&first), CHOICE_ONCE);
        check_answer(Some(&agent), &pane, "prompt-0", Some("medium"), None, None).expect("allowed once");
        assert_eq!(check_answer(Some(&agent), &pane, "prompt-1", Some("medium"), None, None).unwrap_err().code, "CONSENT_PENDING");
    });
}

#[test]
fn answering_in_the_agents_own_pane_or_by_the_person_never_asks() {
    let (agent, own_pane, user_pane) = (id("s"), id("pane"), id("user-pane"));
    mcp_sessions::register_owned_pane(&agent, &own_pane).unwrap();
    with_config(ASKING, || {
        check_answer(Some(&agent), &own_pane, "p", Some("high"), None, None).expect("its own helper");
        check_answer(None, &user_pane, "p", Some("high"), None, None).expect("the person (no session) is never gated");
    });
    with_config(TODAY, || {
        check_answer(Some(&agent), &user_pane, "p", Some("high"), None, None).expect("A off is today's behaviour");
    });
}

#[test]
fn allow_for_a_pane_does_not_unlock_answering_risky_prompts() {
    let (agent, pane) = (id("s"), id("user-pane"));
    with_config(ASKING, || {
        let card_id = consent_id_of(&check_take_over(&agent, Some(&pane), None, None).unwrap_err());
        person_answers(&card_id, CHOICE_ALWAYS);
        check_take_over(&agent, Some(&pane), None, None).unwrap();
        mcp_sessions::take_over_pane(&agent, &pane, true).unwrap();
        assert_eq!(check_answer(Some(&agent), &pane, "p", Some("high"), None, None).unwrap_err().code, "CONSENT_PENDING");
    });
}

// ---- who may answer a card ----

#[test]
fn a_card_cannot_be_answered_over_plain_loopback_http_only_by_the_app_or_a_paired_phone() {
    let (agent, pane) = (id("s"), id("user-pane"));
    with_config(ASKING, || {
        let card_id = consent_id_of(&check_take_over(&agent, Some(&pane), None, None).unwrap_err());
        let call = |via_phone: bool| {
            questions::routes::handle("POST", &["questions", &card_id, "answer"], &json!({"choice_id": CHOICE_ONCE}), None, via_phone, &|| {})
                .expect("a questions route")
        };
        // An agent's shell (or any local program) reaches the bridge without a pairing token.
        let (status, body) = call(false).unwrap_err();
        assert_eq!(status, 403);
        assert_eq!(body["code"], "CONSENT_NEEDS_PERSON");
        assert_eq!(card(&card_id).state, QuestionState::Pending, "still waiting");
        // A paired phone, in the app, may.
        let (status, body) = call(true).unwrap();
        assert_eq!(status, 200);
        assert_eq!(body["accepted"], true);
        assert_eq!(body["question"]["answer"]["answered_by"], "phone");
    });
}

#[test]
fn an_agent_cannot_raise_or_forge_a_consent_card_through_ask_user() {
    let body = json!({"question": "Allow Claude to control Codex?", "choices": [{"id":"allow_once","label":"Allow once"}], "consent": {"action":"take_over","requester":"x","target":"y"}});
    let (_, reply) = questions::routes::handle("POST", &["questions", "ask"], &json!({"question": body["question"], "choices": body["choices"], "consent": body["consent"], "wait_ms": 1}), Some("agent-session"), false, &|| {})
        .expect("a questions route")
        .unwrap();
    let question_id = reply["question_id"].as_str().unwrap();
    assert!(card(question_id).consent.is_none(), "the tool cannot set consent");
}

// ---- audit ----

#[test]
fn the_persons_answer_to_a_card_is_audited_with_who_pressed_it() {
    let (agent, pane) = (id("s"), id("user-pane"));
    with_config(ASKING, || {
        let first = consent_id_of(&check_take_over(&agent, Some(&pane), None, None).unwrap_err());
        // answered on the desktop
        person_answers(&first, CHOICE_ONCE);
        check_take_over(&agent, Some(&pane), None, None).unwrap();
        let lines = audit_lines_for(&pane);
        let decision = lines.iter().find(|e| e.kind == "consent").expect("the decision is written down");
        assert_eq!(decision.who, "desktop");
        assert_eq!(decision.summary, "You allowed another agent to control that pane, once");
        assert_eq!(decision.detail["decision"], "allow_once");
        assert!(lines.iter().any(|e| e.kind == "take_over" && e.who == "agent"), "and the take-over that followed");

        // answered from a phone
        let second = consent_id_of(&check_take_over(&agent, Some(&pane), None, None).unwrap_err());
        questions::store().answer(&second, Some(CHOICE_DENY), None, "phone", now_ms()).unwrap();
        let lines = audit_lines_for(&pane);
        assert!(lines.iter().any(|e| e.kind == "consent" && e.who == "device" && e.detail["decision"] == "deny"), "{lines:?}");
    });
    assert!(crate::audit::global().verify().ok, "and the chain is still sound");
}

// ---- the settings ----

#[test]
fn the_shipped_defaults_are_todays_behaviour() {
    // The founder has not decided. To switch to the recommended defaults change the two constants in
    // `agent_authority/mod.rs` (A to true, B to false) and update this test with them.
    let defaults = Config::default();
    assert!(!defaults.ask_before_agent_control, "A defaults off");
    assert!(defaults.agents_may_merge, "B defaults on");
    assert_eq!(defaults.ask_before_agent_control, super::DEFAULT_ASK_BEFORE_AGENT_CONTROL);
    assert_eq!(defaults.agents_may_merge, super::DEFAULT_AGENTS_MAY_MERGE);
}

#[test]
fn a_saved_choice_beats_the_default_and_a_broken_file_means_the_defaults() {
    use super::config::parse;
    assert_eq!(parse(r#"{"ask_before_agent_control":true,"agents_may_merge":false}"#), Config { ask_before_agent_control: true, agents_may_merge: false });
    assert_eq!(parse(r#"{"agents_may_merge":false}"#), Config { ask_before_agent_control: super::DEFAULT_ASK_BEFORE_AGENT_CONTROL, agents_may_merge: false }, "a missing key takes its default");
    assert_eq!(parse("not json"), Config::default());
    assert_eq!(parse(""), Config::default());
}

#[test]
fn the_two_settings_are_not_in_the_phone_patchable_public_settings() {
    // A paired phone can patch the public-settings mirror over the bridge. These switches decide
    // what an agent may do without asking, so they live in their own file and Tauri command.
    let merged = crate::settings_store::merge_public_settings(&json!({}), &json!({"ask_before_agent_control": true, "agents_may_merge": false}));
    assert!(merged.get("ask_before_agent_control").is_none() && merged.get("agents_may_merge").is_none());
}

// ---- no ungated way in ----

/// Every call site of the lease primitives that can hand an agent a pane it did not start. A new
/// caller must go through `agent_authority::gate` first; this list is the review checkpoint.
#[test]
fn no_ungated_take_over_callers() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found: Vec<String> = Vec::new();
    fn walk(dir: &std::path::Path, root: &std::path::Path, found: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, root, found);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let rel = path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
                if rel.contains("tests") || rel.starts_with("mcp_sessions/") || rel.starts_with("agent_authority/") {
                    continue;
                }
                let text = std::fs::read_to_string(&path).unwrap();
                let hits = text.matches("mcp_sessions::take_over_pane(").count() + text.matches("mcp_sessions::take_over_agent(").count();
                for _ in 0..hits {
                    found.push(rel.clone());
                }
            }
        }
    }
    walk(&src, &src, &mut found);
    found.sort();
    assert_eq!(
        found,
        [
            // each is preceded by agent_authority::gate::check_take_over (or, for the model switch,
            // is the session reclaiming a run it already owns)
            "agent_runs/headless/bind.rs",
            "agent_runs/headless/bind.rs",
            "bridge/handoff.rs",
            "bridge/handoff.rs",
            "bridge/routes/panes.rs",
        ],
        "a new caller of take_over_pane/take_over_agent: call agent_authority::gate::check_take_over before it and add it here"
    );
}

/// Keep the unused-warning quiet for the helper types a future caller will use.
#[allow(dead_code)]
fn _shapes(_: Value) {}

#[test]
fn the_card_names_the_agent_host_when_the_connection_said_what_it_is() {
    let (agent, pane) = (id("s"), id("user-pane"));
    mcp_sessions::note_session_client(&agent, "Claude Code <script>");
    with_config(ASKING, || {
        let q = card(&consent_id_of(&check_take_over(&agent, Some(&pane), None, None).unwrap_err()));
        assert_eq!(q.question, "Allow Claude Code script to control that pane?", "plain characters only");
        assert_eq!(q.agent.as_deref(), Some("Claude Code script"));
    });
}
