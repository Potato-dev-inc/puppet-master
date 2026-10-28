//! The answer sequence with a fake screen and a counting writer.

use super::testing::*;
use super::*;
use crate::attention::caps::DeviceCaps;
use crate::attention::model::{Lane, PaneRef};

fn all() -> Caller {
    caller("anna", DeviceCaps::all())
}

fn watch_only() -> Caller {
    caller("view", DeviceCaps { can_answer: false, can_type: false, can_control: false })
}

fn fresh() -> FakeEnv {
    FakeEnv::showing("pane-1", rec("pid-1", "Run command: npm test (in shop)", "low"), 3)
}

fn outcome(reply: &Reply) -> &str {
    reply.1["outcome"].as_str().unwrap_or("")
}

#[test]
fn a_matching_answer_types_exactly_one_key_and_leaves_a_receipt() {
    let env = fresh();
    let reply = answer(&env, &all(), &env.item_id(), &env.request("allow_once", Some("k1")));
    assert_eq!(reply.0, 200);
    assert_eq!(outcome(&reply), "answered");
    assert_eq!(reply.1["choice_label"], "Allowed once");
    assert_eq!(env.typed.lock().clone(), vec![("pane-1".to_string(), "allow_once".to_string())]);
    // The tombstone says who: this phone, by name.
    let notes = env.notes.lock();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].0, "approval:pane-1");
    assert_eq!(notes[0].1.by.as_deref(), Some("phone"));
    assert_eq!(notes[0].1.device_name.as_deref(), Some("anna's phone"));
    assert_eq!(env.audits.lock().clone(), vec!["A phone answered a card: Allowed once".to_string()]);
    assert_eq!(env.locks_taken.load(std::sync::atomic::Ordering::SeqCst), 1, "checked and typed under the answer lock");
}

#[test]
fn deny_reads_denied() {
    let env = fresh();
    let reply = answer(&env, &all(), &env.item_id(), &env.request("deny", None));
    assert_eq!(reply.1["choice_label"], "Denied");
}

#[test]
fn a_fingerprint_that_is_not_the_cards_is_changed_and_types_nothing() {
    let env = fresh();
    for tweak in 0..3 {
        let mut request = env.request("allow_once", None);
        match tweak {
            0 => request.fingerprint = Some("0000000000000000".into()),
            1 => request.occurrence = Some(2),
            _ => request.prompt_id = Some("other".into()),
        }
        let reply = answer(&env, &all(), &env.item_id(), &request);
        assert_eq!(outcome(&reply), "changed", "tweak {tweak}");
    }
    assert_eq!(env.writes(), 0);
}

#[test]
fn a_screen_that_changed_after_the_card_was_drawn_is_changed_or_gone() {
    // The command on screen is another one now (same pane, new prompt id).
    let env = fresh();
    *env.live.lock() = Some(Live { rec: rec("pid-2", "Run command: rm -rf build", "high"), observed_pos: Some(10), agent: AgentProcess::Alive });
    assert_eq!(outcome(&answer(&env, &all(), &env.item_id(), &env.request("allow_once", None))), "changed");
    // The same command asked a second time is a new occurrence: an old card never answers it.
    let env2 = FakeEnv::showing("pane-1", rec("pid-1", "Run command: npm test (in shop)", "low"), 3);
    *env2.occurrence.lock() = 4;
    assert_eq!(outcome(&answer(&env2, &all(), &env2.item_id(), &env2.request("allow_once", None))), "changed");
    // The risk read changed under the same command: what you saw is what you answer.
    let env3 = FakeEnv::showing("pane-1", rec("pid-1", "Run command: npm test (in shop)", "low"), 3);
    *env3.live.lock() = Some(Live { rec: rec("pid-1", "Run command: npm test (in shop)", "high"), observed_pos: Some(10), agent: AgentProcess::Alive });
    assert_eq!(outcome(&answer(&env3, &all(), &env3.item_id(), &env3.request("allow_once", None))), "changed");
    // No prompt on screen at all.
    let env4 = fresh();
    *env4.live.lock() = None;
    assert_eq!(answer(&env4, &all(), &env4.item_id(), &env4.request("allow_once", None)).1["outcome"], "gone");
    for e in [&env, &env2, &env3, &env4] {
        assert_eq!(e.writes(), 0, "nothing is typed into a screen the card did not show");
    }
}

#[test]
fn a_pane_that_printed_while_the_route_took_its_checkpoint_gets_one_more_look_at_the_same_prompt() {
    // The permit refused the first write (output arrived after the prompt was read). The screen is read
    // again; it is the same prompt, so the key goes out once, under a new permit.
    let env = fresh();
    *env.deliver_result.lock() = Some(Delivered::Stale);
    let reply = answer(&env, &all(), &env.item_id(), &env.request("allow_once", None));
    assert_eq!(outcome(&reply), "answered");
    assert_eq!(env.writes(), 1, "typed exactly once");
}

#[test]
fn a_screen_that_keeps_changing_under_the_key_is_changed_and_types_nothing() {
    let env = fresh();
    *env.always_stale.lock() = true;
    let reply = answer(&env, &all(), &env.item_id(), &env.request("allow_once", None));
    assert_eq!(outcome(&reply), "changed");
    assert_eq!(env.writes(), 0);
    assert_eq!(env.notes.lock().len(), 0, "nothing was answered, so no receipt");
}

#[test]
fn a_dead_agent_gets_no_key() {
    let env = fresh();
    *env.live.lock() = Some(Live { rec: rec("pid-1", "Run command: npm test (in shop)", "low"), observed_pos: Some(10), agent: AgentProcess::Exited });
    let reply = answer(&env, &all(), &env.item_id(), &env.request("allow_once", None));
    assert_eq!(outcome(&reply), "gone");
    assert_eq!(env.writes(), 0);
}

#[test]
fn a_retry_with_the_same_key_gets_the_first_answer_and_never_presses_twice() {
    let env = fresh();
    let first = answer(&env, &all(), &env.item_id(), &env.request("allow_once", Some("tap-1")));
    assert_eq!(outcome(&first), "answered");
    // The reply was lost on a flaky tunnel; "Try again" re-sends the same key.
    let again = answer(&env, &all(), &env.item_id(), &env.request("allow_once", Some("tap-1")));
    assert_eq!(outcome(&again), "already_answered");
    assert_eq!(again.1["by"], "you");
    assert_eq!(again.1["choice_label"], "Allowed once");
    assert_eq!(env.writes(), 1, "the write count stays 1");
}

#[test]
fn a_retry_after_a_failed_write_really_retries() {
    let env = fresh();
    *env.deliver_result.lock() = Some(Delivered::Failed { code: "PROMPT_REPLY_FAILED".into(), message: "write failed".into() });
    let first = answer(&env, &all(), &env.item_id(), &env.request("allow_once", Some("tap-1")));
    assert_eq!(outcome(&first), "failed");
    assert_eq!(env.writes(), 0);
    let second = answer(&env, &all(), &env.item_id(), &env.request("allow_once", Some("tap-1")));
    assert_eq!(outcome(&second), "answered", "a failure is not remembered");
    assert_eq!(env.writes(), 1);
}

#[test]
fn a_key_belongs_to_the_device_that_made_it() {
    let env = fresh();
    answer(&env, &all(), &env.item_id(), &env.request("allow_once", Some("shared")));
    // Another phone sending the same key does not read anna's answer from the ledger; the item is a
    // tombstone by now and says who answered.
    let other = answer(&env, &caller("ben", DeviceCaps::all()), &env.item_id(), &env.request("allow_once", Some("shared")));
    assert_eq!(outcome(&other), "already_answered");
    assert_eq!(other.1["by"], "phone");
    assert_eq!(other.1["device_name"], "anna's phone");
    assert_eq!(env.writes(), 1);
}

#[test]
fn two_devices_race_one_answers_and_the_other_is_told_by_whom() {
    let env = fresh();
    let first = answer(&env, &caller("anna", DeviceCaps::all()), &env.item_id(), &env.request("deny", Some("a")));
    let second = answer(&env, &caller("ben", DeviceCaps::all()), &env.item_id(), &env.request("allow_once", Some("b")));
    assert_eq!(outcome(&first), "answered");
    assert_eq!(outcome(&second), "already_answered");
    assert_eq!(second.1["by"], "phone");
    assert_eq!(second.1["choice_label"], "Denied");
    assert_eq!(env.writes(), 1);
    // The same phone with a new key (a second tap) is told it was itself.
    let third = answer(&env, &caller("anna", DeviceCaps::all()), &env.item_id(), &env.request("deny", Some("c")));
    assert_eq!(third.1["by"], "you");
}

#[test]
fn an_item_answered_at_the_pc_says_so() {
    let env = fresh();
    env.note_resolution(
        "approval:pane-1",
        Resolution { state: "answered".into(), by: Some("desktop".into()), choice_label: Some("Allowed once".into()), at_ms: 5, ..Resolution::default() },
    );
    let reply = answer(&env, &all(), &env.item_id(), &env.request("allow_once", None));
    assert_eq!(outcome(&reply), "already_answered");
    assert_eq!(reply.1["by"], "pc");
    assert_eq!(reply.1["choice_label"], "Allowed once");
    assert_eq!(env.writes(), 0);
}

#[test]
fn an_item_that_left_without_an_answer_is_gone_with_its_reason() {
    let env = fresh();
    env.items.lock()[0].state = State::Resolved;
    env.items.lock()[0].resolution = Some(Resolution { state: "gone".into(), reason: Some("moved_on".into()), at_ms: 5, ..Resolution::default() });
    let reply = answer(&env, &all(), &env.item_id(), &env.request("allow_once", None));
    assert_eq!((outcome(&reply), reply.1["reason"].as_str()), ("gone", Some("moved_on")));
    assert_eq!(outcome(&answer(&env, &all(), "no-such-item", &env.request("allow_once", None))), "gone");
}

#[test]
fn a_device_without_the_answer_ability_is_refused_and_nothing_is_typed() {
    let env = fresh();
    let reply = answer(&env, &watch_only(), &env.item_id(), &env.request("allow_once", Some("k")));
    assert_eq!(reply.0, 403);
    assert_eq!(reply.1["code"], "DEVICE_CAP_REQUIRED");
    assert_eq!(reply.1["cap"], "answer");
    assert_eq!(env.writes(), 0);
    assert_eq!(env.locks_taken.load(std::sync::atomic::Ordering::SeqCst), 0, "refused before anything else");
}

#[test]
fn a_broad_choice_is_refused_from_a_phone() {
    let env = fresh();
    let reply = answer(&env, &all(), &env.item_id(), &env.request("allow_all", None));
    assert_eq!(reply.0, 403);
    assert_eq!(reply.1["error"], "broad_not_from_phone");
    assert_eq!(env.writes(), 0);
}

#[test]
fn a_request_without_what_the_card_showed_is_not_answered() {
    let env = fresh();
    let mut request = env.request("allow_once", None);
    request.fingerprint = None;
    assert_eq!(answer(&env, &all(), &env.item_id(), &request).0, 400);
    assert_eq!(answer(&env, &all(), &env.item_id(), &Request { choice: Some("allow_once".into()), ..Request::default() }).0, 400);
    assert_eq!(answer(&env, &all(), &env.item_id(), &env.request("sudo", None)).0, 400, "a choice the prompt does not offer");
    let mut missing = env.request("allow_once", None);
    missing.choice = None;
    assert_eq!(answer(&env, &all(), &env.item_id(), &missing).0, 400);
    assert_eq!(env.writes(), 0);
}

#[test]
fn an_answer_key_is_short_and_printable() {
    let env = fresh();
    for key in ["", &"k".repeat(81), "has space", "bad\u{7}"] {
        let reply = answer(&env, &all(), &env.item_id(), &env.request("allow_once", Some(key)));
        assert_eq!(reply.0, 400, "{key:?}");
    }
    assert_eq!(env.writes(), 0);
}

#[test]
fn the_ledger_forgets_old_entries_and_never_grows_past_its_cap() {
    let mut ledger = Ledger::default();
    for n in 0..(LEDGER_CAP + 50) {
        ledger.put("d", &format!("k{n}"), "Allowed once".into(), n as u64);
    }
    assert!(ledger.entries.len() <= LEDGER_CAP);
    assert!(ledger.get("d", "k0", 10).is_none(), "the oldest went first");
    assert!(ledger.get("d", &format!("k{}", LEDGER_CAP + 49), 1_000).is_some());
    assert!(ledger.get("d", &format!("k{}", LEDGER_CAP + 49), 1_000 + LEDGER_TTL_MS + 600).is_none(), "ten minutes");
}

// ---- cards that are not a pane's prompt --------------------------------------------------------

fn question_item(kind: Kind, subject: &str) -> Item {
    Item {
        id: format!("q-{subject}"),
        key: format!("{}:{subject}", kind.as_str()),
        kind,
        lane: Lane::Blocking,
        rank: 2,
        state: State::Open,
        since_ms: Some(1),
        expires_at_ms: None,
        pane: Some(PaneRef { id: "pane-1".into(), heading: "h".into(), agent: None, project: None }),
        title: "Which database?".into(),
        sentence: None,
        risk: None,
        prompt: None,
        actions: vec![],
        resolution: None,
    }
}

#[test]
fn a_question_goes_to_the_questions_store_and_never_types_into_a_pane() {
    let env = fresh();
    env.items.lock().push(question_item(Kind::Question, "q1"));
    let request = Request { choice: Some("pg".into()), ..Request::default() };
    *env.question_result.lock() = Some(QuestionResult::Answered { label: Some("Postgres".into()) });
    let reply = answer(&env, &all(), "q-q1", &request);
    assert_eq!(outcome(&reply), "answered");
    assert_eq!(reply.1["choice_label"], "Postgres");
    assert_eq!(env.writes(), 0);
    assert_eq!(env.notes.lock()[0].0, "question:q1");
    // Somebody was faster.
    env.items.lock().push(question_item(Kind::Question, "q2"));
    *env.question_result.lock() = Some(QuestionResult::AlreadyAnswered { by: "desktop".into(), label: Some("SQLite".into()) });
    let late = answer(&env, &all(), "q-q2", &request);
    assert_eq!((outcome(&late), late.1["by"].as_str()), ("already_answered", Some("pc")));
    // Neither a choice nor text, or both.
    env.items.lock().push(question_item(Kind::Question, "q3"));
    assert_eq!(answer(&env, &all(), "q-q3", &Request::default()).0, 400);
    assert_eq!(answer(&env, &all(), "q-q3", &Request { choice: Some("a".into()), text: Some("b".into()), ..Request::default() }).0, 400);
}

#[test]
fn a_budget_or_limit_card_needs_the_control_ability_too() {
    let env = fresh();
    env.items.lock().push(question_item(Kind::Budget, "b1"));
    env.items.lock().push(question_item(Kind::Consent, "c1"));
    let request = Request { choice: Some("add".into()), ..Request::default() };
    let answer_only = caller("anna", DeviceCaps { can_answer: true, can_type: false, can_control: false });
    let reply = answer(&env, &answer_only, "q-b1", &request);
    assert_eq!((reply.0, reply.1["cap"].as_str()), (403, Some("control")));
    // A consent card is a decision about an agent: `answer` is enough.
    assert_eq!(outcome(&answer(&env, &answer_only, "q-c1", &Request { choice: Some("deny".into()), ..Request::default() })), "answered");
    assert_eq!(outcome(&answer(&env, &all(), "q-b1", &request)), "answered");
}

#[test]
fn an_error_or_a_stuck_item_has_nothing_to_answer() {
    let env = fresh();
    let mut item = question_item(Kind::Error, "pane-1");
    item.id = "err".into();
    env.items.lock().push(item);
    assert_eq!(answer(&env, &all(), "err", &Request { choice: Some("x".into()), ..Request::default() }).0, 400);
}

#[test]
fn a_lock_screen_button_goes_through_the_same_checks_and_is_audited_as_one() {
    let env = fresh();
    let lock = Caller { via: Via::LockScreen, ..all() };
    // The notification was about the first occurrence; the pane has asked again since.
    let mut stale = env.request("allow_once", Some("lock-1"));
    stale.occurrence = Some(2);
    assert_eq!(outcome(&answer(&env, &lock, &env.item_id(), &stale)), "changed");
    assert_eq!(env.writes(), 0, "a stale grant types nothing");
    let reply = answer(&env, &lock, &env.item_id(), &env.request("allow_once", Some("lock-1")));
    assert_eq!(outcome(&reply), "answered");
    assert_eq!(env.audits.lock().clone(), vec!["A phone answered from its lock screen: Allowed once".to_string()]);
    // A button on a phone that lost its answer ability since the notification went out is refused.
    let revoked = Caller { via: Via::LockScreen, ..watch_only() };
    assert_eq!(answer(&env, &revoked, &env.item_id(), &env.request("allow_once", None)).0, 403);
}

// ---- a chat answer (Via::Telegram) -------------------------------------------------------------

fn chat() -> Caller {
    Caller { device_id: "telegram".into(), device_name: "Telegram".into(), caps: DeviceCaps::all(), via: Via::Telegram }
}

#[test]
fn a_chat_answer_is_refused_for_a_trust_gate_a_consent_card_a_budget_and_a_limit() {
    let env = fresh();
    for (kind, subject) in [(Kind::TrustGate, "t1"), (Kind::Consent, "c1"), (Kind::Budget, "b1"), (Kind::Limit, "l1")] {
        env.items.lock().push(question_item(kind, subject));
        let reply = answer(&env, &chat(), &format!("q-{subject}"), &Request { choice: Some("allow_once".into()), ..Request::default() });
        assert_eq!((reply.0, reply.1["error"].as_str()), (403, Some("not_from_chat")), "{kind:?}");
    }
    assert_eq!(env.writes(), 0);
    assert!(env.notes.lock().is_empty(), "nothing was answered, so no receipt");
    assert!(env.audits.lock().is_empty(), "nothing was answered, so no audit line");
}

#[test]
fn a_chat_answer_is_refused_for_an_approval_above_low_risk_or_with_no_risk_read() {
    for risk in [Some("medium"), Some("high"), None] {
        let mut prompt = rec("pid-1", "Run command: npm test (in shop)", "low");
        prompt.risk_level = risk.map(str::to_string);
        let env = FakeEnv::showing("pane-1", prompt, 3);
        let reply = answer(&env, &chat(), &env.item_id(), &env.request("allow_once", None));
        assert_eq!((reply.0, reply.1["error"].as_str()), (403, Some("not_from_chat")), "{risk:?}");
        assert_eq!(env.writes(), 0, "{risk:?}");
        assert!(env.audits.lock().is_empty(), "{risk:?}");
    }
}

#[test]
fn a_chat_answer_is_allowed_for_a_low_risk_approval_and_is_audited_as_from_telegram() {
    let env = fresh();
    let reply = answer(&env, &chat(), &env.item_id(), &env.request("allow_once", Some("tg-1")));
    assert_eq!(outcome(&reply), "answered");
    assert_eq!(reply.1["choice_label"], "Allowed once");
    assert_eq!(env.typed.lock().clone(), vec![("pane-1".to_string(), "allow_once".to_string())]);
    assert_eq!(env.audits.lock().clone(), vec!["Answered from Telegram: Allowed once".to_string()]);
    let notes = env.notes.lock();
    assert_eq!(notes[0].1.by.as_deref(), Some("phone"));
    assert_eq!(notes[0].1.device_name.as_deref(), Some("Telegram"));
}

#[test]
fn a_chat_answer_is_allowed_for_a_question() {
    let env = fresh();
    env.items.lock().push(question_item(Kind::Question, "q1"));
    *env.question_result.lock() = Some(QuestionResult::Answered { label: Some("Postgres".into()) });
    let reply = answer(&env, &chat(), "q-q1", &Request { choice: Some("pg".into()), ..Request::default() });
    assert_eq!(outcome(&reply), "answered");
    assert_eq!(env.writes(), 0, "a question never types into a pane");
    assert_eq!(env.audits.lock().clone(), vec!["Answered from Telegram: Postgres".to_string()]);
}

#[test]
fn the_card_and_lock_screen_wording_of_the_audit_line_is_unchanged() {
    let env = fresh();
    answer(&env, &all(), &env.item_id(), &env.request("allow_once", None));
    let lock_env = fresh();
    answer(&lock_env, &Caller { via: Via::LockScreen, ..all() }, &lock_env.item_id(), &lock_env.request("allow_once", None));
    assert_eq!(env.audits.lock().clone(), vec!["A phone answered a card: Allowed once".to_string()]);
    assert_eq!(lock_env.audits.lock().clone(), vec!["A phone answered from its lock screen: Allowed once".to_string()]);
}

#[test]
fn a_retry_after_the_tombstone_is_gone_is_still_told_it_went_through_and_never_presses_again() {
    // The answer went through, the reply was lost, and the phone only retries after the card has left the list
    // (the tombstone lives 15 s): the ledger still knows the key.
    let env = fresh();
    let first = answer(&env, &all(), &env.item_id(), &env.request("allow_once", Some("tap-1")));
    assert_eq!(outcome(&first), "answered");
    let item_id = env.item_id();
    env.items.lock().clear();
    let retry = answer(&env, &all(), &item_id, &env.request_with("allow_once", Some("tap-1"), "pid-1", 3, "x"));
    assert_eq!((outcome(&retry), retry.1["by"].as_str(), retry.1["choice_label"].as_str()), ("already_answered", Some("you"), Some("Allowed once")));
    assert_eq!(env.writes(), 1);
    // Another phone, or a new tap with a new key, gets no such answer: the item is simply gone.
    assert_eq!(outcome(&answer(&env, &all(), &item_id, &env.request_with("allow_once", Some("tap-2"), "pid-1", 3, "x"))), "gone");
    assert_eq!(outcome(&answer(&env, &caller("ben", DeviceCaps::all()), &item_id, &env.request_with("allow_once", Some("tap-1"), "pid-1", 3, "x"))), "gone");
}

#[test]
fn the_pc_answered_this_occurrence_a_moment_ago_so_the_phone_is_told_and_nothing_is_typed_or_recorded() {
    // `answer_once` saw the person's click on the PC first (the dialog had not redrawn yet): the route
    // reports a repeat. The phone hears "already answered on your PC"; no key, no ledger entry, no line.
    let env = fresh();
    *env.deliver_result.lock() = Some(Delivered::Repeat);
    let reply = answer(&env, &all(), &env.item_id(), &env.request("allow_once", Some("k9")));
    assert_eq!((reply.0, outcome(&reply), reply.1["by"].as_str()), (200, "already_answered", Some("pc")), "{reply:?}");
    assert!(env.typed.lock().is_empty());
    assert!(env.audits.lock().is_empty());
    assert!(env.notes.lock().is_empty());
}
