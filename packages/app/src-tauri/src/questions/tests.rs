use super::routes::{self, RouteReply};
use super::*;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

fn choices() -> Vec<Choice> {
    vec![
        Choice { id: "yes".into(), label: "Yes".into() },
        Choice { id: "no".into(), label: "No".into() },
    ]
}

fn ask(store: &QuestionStore, now: u64) -> Question {
    store
        .create(
            NewQuestion { question: "Deploy now?".into(), choices: choices(), ..Default::default() },
            now,
        )
        .unwrap()
}

#[test]
fn create_records_the_question_with_defaults() {
    let store = QuestionStore::new();
    let q = ask(&store, 1_000);
    assert_eq!(q.state, QuestionState::Pending);
    assert_eq!(q.created_at_ms, 1_000);
    assert_eq!(q.expires_at_ms, 1_000 + DEFAULT_TIMEOUT_S * 1000);
    // With choices, free text is off unless asked for.
    assert!(!q.allow_free_text);
    assert_eq!(q.urgency, "normal");
    assert_eq!(store.list_pending(2_000), vec![q]);

    // Without choices, free text defaults on.
    let free = store.create(NewQuestion { question: "Which branch?".into(), ..Default::default() }, 0).unwrap();
    assert!(free.allow_free_text);
}

#[test]
fn create_keeps_a_chosen_uuid_and_refuses_a_bad_or_used_one() {
    let store = QuestionStore::new();
    let chosen = uuid::Uuid::new_v4().to_string();
    let q = store.create_as(NewQuestion { question: "q".into(), ..Default::default() }, Some(&chosen), 0).unwrap();
    assert_eq!(q.id, chosen);
    assert!(store.get(&chosen, 0).is_some());
    let again = store.create_as(NewQuestion { question: "q".into(), ..Default::default() }, Some(&chosen), 0);
    assert_eq!(again.unwrap_err().code, "INVALID_ARGUMENT");
    let bad = store.create_as(NewQuestion { question: "q".into(), ..Default::default() }, Some("not-a-uuid"), 0);
    assert_eq!(bad.unwrap_err().code, "INVALID_ARGUMENT");
}

#[test]
fn create_rejects_bad_input() {
    let store = QuestionStore::new();
    let bad = |new: NewQuestion| store.create(new, 0).unwrap_err().code;
    assert_eq!(bad(NewQuestion { question: "  ".into(), ..Default::default() }), "INVALID_ARGUMENT");
    let many: Vec<Choice> =
        (0..7).map(|i| Choice { id: format!("c{i}"), label: format!("C{i}") }).collect();
    assert_eq!(bad(NewQuestion { question: "q".into(), choices: many, ..Default::default() }), "INVALID_ARGUMENT");
    let dup = vec![Choice { id: "a".into(), label: "A".into() }, Choice { id: "a".into(), label: "B".into() }];
    assert_eq!(bad(NewQuestion { question: "q".into(), choices: dup, ..Default::default() }), "INVALID_ARGUMENT");
    assert_eq!(bad(NewQuestion { question: "q".into(), timeout_s: Some(3601), ..Default::default() }), "INVALID_ARGUMENT");
    assert_eq!(bad(NewQuestion { question: "q".into(), timeout_s: Some(0), ..Default::default() }), "INVALID_ARGUMENT");
    assert_eq!(bad(NewQuestion { question: "q".into(), urgency: Some("loud".into()), ..Default::default() }), "INVALID_ARGUMENT");
    // No choices and no free text leaves nothing to answer with.
    assert_eq!(
        bad(NewQuestion { question: "q".into(), allow_free_text: Some(false), ..Default::default() }),
        "INVALID_ARGUMENT"
    );
}

#[test]
fn answer_with_a_choice_then_second_answer_is_idempotent() {
    let store = QuestionStore::new();
    let q = ask(&store, 0);
    let first = store.answer(&q.id, Some("yes"), None, "desktop", 10).unwrap();
    assert!(first.accepted);
    assert_eq!(first.question.state, QuestionState::Answered);
    // The second answer (even a different one) loses and returns the stored resolution.
    let second = store.answer(&q.id, Some("no"), None, "phone", 20).unwrap();
    assert!(!second.accepted);
    assert_eq!(second.question.answer.as_ref().unwrap().choice_id.as_deref(), Some("yes"));
    assert_eq!(second.question.answer.as_ref().unwrap().answered_by, "desktop");
    assert!(store.list_pending(30).is_empty());
}

#[test]
fn invalid_choice_is_rejected_and_the_question_stays_pending() {
    let store = QuestionStore::new();
    let q = ask(&store, 0);
    let err = store.answer(&q.id, Some("maybe"), None, "desktop", 1).unwrap_err();
    assert_eq!(err.code, "INVALID_CHOICE");
    assert_eq!(err.status(), 400);
    // Free text is refused when the question only takes its choices.
    assert_eq!(store.answer(&q.id, None, Some("whatever"), "desktop", 1).unwrap_err().code, "FREE_TEXT_NOT_ALLOWED");
    // Exactly one of choice_id / text.
    assert!(store.answer(&q.id, Some("yes"), Some("x"), "desktop", 1).is_err());
    assert!(store.answer(&q.id, None, None, "desktop", 1).is_err());
    assert_eq!(store.get(&q.id, 2).unwrap().state, QuestionState::Pending);
    assert_eq!(store.answer("missing", Some("yes"), None, "desktop", 1).unwrap_err().status(), 404);
}

#[test]
fn free_text_answers_are_trimmed_and_length_capped() {
    let store = QuestionStore::new();
    let q = store.create(NewQuestion { question: "Which branch?".into(), ..Default::default() }, 0).unwrap();
    let too_long = "x".repeat(MAX_ANSWER_TEXT_CHARS + 1);
    assert_eq!(store.answer(&q.id, None, Some(&too_long), "desktop", 1).unwrap_err().code, "ANSWER_TOO_LONG");
    assert!(store.answer(&q.id, None, Some("   "), "desktop", 1).is_err());
    let done = store.answer(&q.id, None, Some("  main  "), "phone", 2).unwrap();
    assert_eq!(done.question.answer.unwrap().text.as_deref(), Some("main"));
}

#[test]
fn a_question_times_out_and_cannot_be_answered_afterwards() {
    let store = QuestionStore::new();
    let q = store
        .create(NewQuestion { question: "q".into(), timeout_s: Some(5), ..Default::default() }, 1_000)
        .unwrap();
    assert_eq!(store.list_pending(5_999).len(), 1);
    assert!(store.list_pending(6_000).is_empty());
    let late = store.answer(&q.id, None, Some("too late"), "desktop", 6_001).unwrap();
    assert!(!late.accepted);
    assert_eq!(late.question.state, QuestionState::TimedOut);
    assert_eq!(tool_result(&late.question)["reason"], "timeout");
}

#[test]
fn dismiss_cancels_and_is_idempotent() {
    let store = QuestionStore::new();
    let q = ask(&store, 0);
    let first = store.dismiss(&q.id, 5).unwrap();
    assert!(first.accepted);
    assert_eq!(first.question.state, QuestionState::Cancelled);
    assert!(!store.dismiss(&q.id, 6).unwrap().accepted);
    // A dismissed question cannot be answered.
    assert!(!store.answer(&q.id, Some("yes"), None, "desktop", 7).unwrap().accepted);
    assert_eq!(tool_result(&store.get(&q.id, 8).unwrap())["reason"], "cancelled");
    assert!(store.list_pending(8).is_empty());
}

#[test]
fn wait_resolved_wakes_when_answered_from_another_thread() {
    let store = Arc::new(QuestionStore::new());
    let q = ask(&store, now_ms());
    let waiter = {
        let store = store.clone();
        let id = q.id.clone();
        std::thread::spawn(move || store.wait_resolved(&id, Duration::from_secs(10)))
    };
    std::thread::sleep(Duration::from_millis(50));
    store.answer(&q.id, Some("no"), None, "phone", now_ms()).unwrap();
    let woken = waiter.join().unwrap().unwrap();
    assert_eq!(woken.state, QuestionState::Answered);
    assert_eq!(tool_result(&woken)["choice_id"], "no");
    assert_eq!(tool_result(&woken)["answered_by"], "phone");
}

#[test]
fn wait_resolved_returns_pending_after_the_slice_and_unknown_as_none() {
    let store = QuestionStore::new();
    let q = ask(&store, now_ms());
    let still = store.wait_resolved(&q.id, Duration::from_millis(30)).unwrap();
    assert_eq!(still.state, QuestionState::Pending);
    assert_eq!(tool_result(&still)["resolved"], false);
    assert!(store.wait_resolved("nope", Duration::from_millis(10)).is_none());
}

#[test]
fn wait_resolved_notices_the_questions_own_timeout() {
    let store = QuestionStore::new();
    let q = store
        .create(NewQuestion { question: "q".into(), timeout_s: Some(1), ..Default::default() }, now_ms())
        .unwrap();
    let done = store.wait_resolved(&q.id, Duration::from_secs(5)).unwrap();
    assert_eq!(done.state, QuestionState::TimedOut);
}

// ---- In-process router transcript: auth gate, then the questions routes. -------------------

/// Mirrors the order in `bridge/router.rs::route`: pairing gate, MCP session gate, then the
/// questions routes. (`route` itself needs a Tauri `AppHandle`, which unit tests cannot build.)
fn call(
    method: &str,
    path: &str,
    body: Value,
    headers: &str,
    peer_loopback: bool,
    session: Option<&str>,
    notified: &AtomicUsize,
) -> RouteReply {
    let segments: Vec<&str> = path.trim_matches('/').split('/').collect();
    crate::mobile_pairing::authorize_bridge_request(headers, peer_loopback, &segments, method)?;
    crate::mcp_sessions::authorize(session, method, path, &body)
        .map_err(|e| (403, json!({ "code": e.code, "error": e.message })))?;
    let via_phone = crate::mobile_pairing::requires_pairing_token(headers, peer_loopback);
    routes::handle(method, &segments, &body, session, via_phone, &|| {
        notified.fetch_add(1, Ordering::SeqCst);
    })
    .unwrap_or_else(|| Err((404, json!({ "error": "not found" }))))
}

fn paired_phone_token() -> String {
    use base64::Engine;
    use ed25519_dalek::SigningKey;
    let store = crate::mobile_pairing::pairing_store().unwrap_or_else(|| {
        let path = std::env::temp_dir().join(format!("pm-questions-pairing-{}.json", uuid::Uuid::new_v4()));
        crate::mobile_pairing::init_pairing_store(path).unwrap()
    });
    let mut guard = store.lock();
    let session = guard.create_pairing_session("http://127.0.0.1:1".into());
    let key = SigningKey::from_bytes(&[7_u8; 32]).verifying_key();
    let request = serde_json::from_value(json!({
        "pairing_code": session.pairing_code,
        "device_name": "test phone",
        "device_public_key": base64::engine::general_purpose::STANDARD.encode(key.as_bytes()),
    }))
    .unwrap();
    guard.pair_device(request).unwrap().device_token
}

#[test]
fn router_transcript_list_answer_dismiss_with_auth() {
    let notified = AtomicUsize::new(0);
    let local = "Host: 127.0.0.1\r\n";
    let token = paired_phone_token();
    let phone = format!("Host: pm.example.com\r\nAuthorization: Bearer {token}\r\n");
    let stranger = "Host: pm.example.com\r\nAuthorization: Bearer not-a-token\r\n";
    let mcp = "agent-connection-1";

    // An MCP session asks (blocks briefly via wait_ms) and gets an unresolved reply with an id.
    let (status, asked) = call(
        "POST",
        "/questions/ask",
        json!({ "question": "Pick a database", "choices": [{"id":"pg","label":"Postgres"},{"id":"sq","label":"SQLite"}],
                "agent": "Planner", "wait_ms": 10 }),
        local,
        true,
        Some(mcp),
        &notified,
    )
    .unwrap();
    assert_eq!((status, asked["resolved"].clone()), (200, json!(false)));
    let id = asked["question_id"].as_str().unwrap().to_string();
    assert!(notified.load(Ordering::SeqCst) >= 1, "creating a question publishes a change");

    // The desktop webview (no session header) lists it.
    let (status, listed) = call("GET", "/questions", json!({}), local, true, None, &notified).unwrap();
    assert_eq!(status, 200);
    let mine = listed["questions"].as_array().unwrap().iter().find(|q| q["id"] == id.as_str()).unwrap();
    assert_eq!(mine["agent"], "Planner");
    assert_eq!(mine["choices"][0]["id"], "pg");

    // Auth: a remote caller with no token and one with a bad token are refused.
    let no_token = call("GET", "/questions", json!({}), "Host: pm.example.com\r\n", false, None, &notified).unwrap_err();
    assert_eq!(no_token.0, 401);
    let bad = call("GET", "/questions", json!({}), stranger, false, None, &notified).unwrap_err();
    assert_eq!(bad.0, 401);
    // An MCP session cannot list or answer: the agent must not answer its own question.
    let denied = call("GET", "/questions", json!({}), local, true, Some(mcp), &notified).unwrap_err();
    assert_eq!(denied.0, 403);
    let denied = call(
        "POST",
        &format!("/questions/{id}/answer"),
        json!({ "choice_id": "pg" }),
        local,
        true,
        Some(mcp),
        &notified,
    )
    .unwrap_err();
    assert_eq!(denied.0, 403);

    // A bad choice is a 400 and leaves the question open.
    let bad_choice = call(
        "POST",
        &format!("/questions/{id}/answer"),
        json!({ "choice_id": "mongo" }),
        &phone,
        false,
        None,
        &notified,
    )
    .unwrap_err();
    assert_eq!((bad_choice.0, bad_choice.1["code"].clone()), (400, json!("INVALID_CHOICE")));

    // The paired phone answers; it is recorded as the phone.
    let (status, done) = call(
        "POST",
        &format!("/questions/{id}/answer"),
        json!({ "choice_id": "sq" }),
        &phone,
        false,
        None,
        &notified,
    )
    .unwrap();
    assert_eq!(status, 200);
    assert_eq!(done["accepted"], true);
    assert_eq!(done["question"]["answer"]["answered_by"], "phone");

    // A second answer from the desktop loses (idempotent, 200, accepted=false).
    let (status, again) = call(
        "POST",
        &format!("/questions/{id}/answer"),
        json!({ "choice_id": "pg" }),
        local,
        true,
        None,
        &notified,
    )
    .unwrap();
    assert_eq!((status, again["accepted"].clone()), (200, json!(false)));
    assert_eq!(again["question"]["answer"]["choice_id"], "sq");

    // The agent's next poll sees the answer as the tool result.
    let (_, result) =
        call("POST", "/questions/ask", json!({ "question_id": id, "wait_ms": 10 }), local, true, Some(mcp), &notified)
            .unwrap();
    assert_eq!(
        result,
        json!({ "resolved": true, "question_id": id, "answered": true, "answered_by": "phone", "choice_id": "sq" })
    );

    // Dismiss: ask again, dismiss from the desktop, the agent sees "cancelled".
    let (_, second) = call(
        "POST",
        "/questions/ask",
        json!({ "question": "Continue?", "choices": [{"id":"go","label":"Go"}], "wait_ms": 1 }),
        local,
        true,
        Some(mcp),
        &notified,
    )
    .unwrap();
    let id2 = second["question_id"].as_str().unwrap().to_string();
    let (status, dismissed) =
        call("POST", &format!("/questions/{id2}/dismiss"), json!({}), local, true, None, &notified).unwrap();
    assert_eq!((status, dismissed["accepted"].clone()), (200, json!(true)));
    let (_, result) =
        call("POST", "/questions/ask", json!({ "question_id": id2, "wait_ms": 10 }), local, true, Some(mcp), &notified)
            .unwrap();
    assert_eq!(result["answered"], false);
    assert_eq!(result["reason"], "cancelled");
    let missing = call("POST", "/questions/nope/dismiss", json!({}), local, true, None, &notified).unwrap_err();
    assert_eq!(missing.0, 404);
}

#[test]
fn a_new_question_triggers_one_push_and_polling_or_cancelling_triggers_none() {
    let notified = AtomicUsize::new(0);
    let local = "Host: 127.0.0.1\r\n";
    let count = |id: &str| crate::push::NOTIFIED_QUESTIONS.lock().iter().filter(|x| x.as_str() == id).count();
    let (_, first) = call(
        "POST",
        "/questions/ask",
        json!({ "question": "Push me once?", "wait_ms": 1 }),
        local,
        true,
        Some("agent-push-1"),
        &notified,
    )
    .unwrap();
    let id = first["question_id"].as_str().unwrap().to_string();
    assert_eq!(count(&id), 1, "creating the question pushes once");
    // The shim's long-poll repeats the call with the id: no new question, no new push.
    for _ in 0..3 {
        call("POST", "/questions/ask", json!({ "question_id": id, "wait_ms": 1 }), local, true, Some("agent-push-1"), &notified).unwrap();
    }
    assert_eq!(count(&id), 1);
    // Listing, answering and cancelling never push.
    call("GET", "/questions", json!({}), local, true, None, &notified).unwrap();
    call("POST", &format!("/questions/{id}/answer"), json!({ "text": "ok" }), local, true, None, &notified).unwrap();
    assert_eq!(count(&id), 1);
    // A second question is its own notification.
    let (_, second) = call("POST", "/questions/ask", json!({ "question": "And me?", "wait_ms": 1 }), local, true, Some("agent-push-1"), &notified).unwrap();
    let id2 = second["question_id"].as_str().unwrap().to_string();
    assert_eq!((count(&id), count(&id2)), (1, 1));
    // A rejected question (empty) pushes nothing. Other tests push concurrently, so compare the
    // ids this call could have created (none) rather than the global list length.
    let before: Vec<String> = crate::push::NOTIFIED_QUESTIONS.lock().clone();
    assert!(call("POST", "/questions/ask", json!({ "question": "  " }), local, true, Some("agent-push-1"), &notified).is_err());
    let open_for_session = |list: &serde_json::Value| -> Vec<String> {
        list["questions"].as_array().into_iter().flatten()
            .filter(|q| q["session_id"].as_str() == Some("agent-push-1") || q["owner"].as_str() == Some("agent-push-1"))
            .filter_map(|q| q["id"].as_str().or(q["question_id"].as_str()).map(str::to_string))
            .collect()
    };
    let (_, listed) = call("GET", "/questions", json!({}), local, true, None, &notified).unwrap();
    let new_ids: Vec<String> = crate::push::NOTIFIED_QUESTIONS.lock().iter().filter(|x| !before.contains(x)).cloned().collect();
    for mine in open_for_session(&listed) {
        assert!(!new_ids.contains(&mine), "the rejected question must not push");
    }
    assert_eq!((count(&id), count(&id2)), (1, 1));
}

// ---- Person-only cards: an issue card (`issue_start`, `issue_push`) ----------------------------

fn issue_card(store: &QuestionStore, kind: &str, text: &str) -> Question {
    store
        .create(NewQuestion { question: text.into(), choices: choices(), kind: Some(kind.into()), ..Default::default() }, now_ms())
        .unwrap()
}

#[test]
fn an_issue_card_may_wait_a_day_and_no_longer() {
    let store = QuestionStore::new();
    for kind in ["issue_start", "issue_push"] {
        let day = NewQuestion {
            question: format!("{kind} day"),
            choices: choices(),
            kind: Some(kind.into()),
            timeout_s: Some(24 * 3600),
            ..Default::default()
        };
        assert!(store.create(day.clone(), 0).is_ok(), "{kind}: 24 hours is accepted");
        let too_long = NewQuestion { timeout_s: Some(24 * 3600 + 1), ..day };
        let refused = store.create(too_long, 0).unwrap_err();
        assert_eq!(refused.code, "INVALID_ARGUMENT", "{kind}: 25 hours is refused");
    }
    // A plain question still has the one-hour cap.
    let plain = NewQuestion { question: "plain".into(), choices: choices(), timeout_s: Some(3601), ..Default::default() };
    assert!(store.create(plain, 0).is_err());
}

#[test]
fn an_agent_cannot_ask_for_an_issue_card_over_ask_user() {
    // `ask_user` never sets a kind: a question an agent asks cannot become a person-only card.
    let body = json!({ "question": "Start on #9 with Codex?", "choices": [{"id": "start", "label": "Start"}], "kind": "issue_start", "wait_ms": 1 });
    let (status, reply) = routes::handle("POST", &["questions", "ask"], &body, Some("agent-kind-1"), false, &|| {})
        .unwrap()
        .unwrap();
    assert_eq!(status, 200);
    let id = reply["question_id"].as_str().unwrap().to_string();
    assert_eq!(store().get(&id, now_ms()).unwrap().kind, None);
    // So the same answer from a plain loopback request is the agent's own question: it is not refused here.
    let (status, _) = routes::handle("POST", &["questions", &id, "answer"], &json!({ "choice_id": "start" }), None, false, &|| {})
        .unwrap()
        .unwrap();
    assert_eq!(status, 200);
}

#[test]
fn only_the_person_answers_or_dismisses_an_issue_card_over_the_bridge() {
    let card = issue_card(store(), "issue_start", "Start on #42 \"Login button does nothing\" with Codex? [loopback]");
    let answer = json!({ "choice_id": "yes" });
    // Plain loopback HTTP (an agent's shell, a companion, any local program): refused, the card stays pending.
    let (status, body) = routes::handle("POST", &["questions", &card.id, "answer"], &answer, None, false, &|| {})
        .unwrap()
        .unwrap_err();
    assert_eq!((status, body["code"].as_str()), (403, Some("ISSUE_NEEDS_PERSON")));
    assert_eq!(store().get(&card.id, now_ms()).unwrap().state, QuestionState::Pending);
    let (status, body) = routes::handle("POST", &["questions", &card.id, "dismiss"], &json!({}), None, false, &|| {})
        .unwrap()
        .unwrap_err();
    assert_eq!((status, body["code"].as_str()), (403, Some("ISSUE_NEEDS_PERSON")));
    assert_eq!(store().get(&card.id, now_ms()).unwrap().state, QuestionState::Pending);
    // A paired phone is the person: it may answer.
    let (status, _) = routes::handle("POST", &["questions", &card.id, "answer"], &answer, None, true, &|| {})
        .unwrap()
        .unwrap();
    assert_eq!(status, 200);
    assert_eq!(store().get(&card.id, now_ms()).unwrap().state, QuestionState::Answered);
}

#[test]
fn a_phone_may_dismiss_an_issue_card_and_loopback_dismiss_of_other_cards_is_unchanged() {
    let card = issue_card(store(), "issue_push", "Draft PR ready for #7? [dismiss]");
    let (status, _) = routes::handle("POST", &["questions", &card.id, "dismiss"], &json!({}), None, true, &|| {})
        .unwrap()
        .unwrap();
    assert_eq!(status, 200);
    // A plain question still dismisses over loopback, as before.
    let plain = ask(store(), now_ms());
    let (status, _) = routes::handle("POST", &["questions", &plain.id, "dismiss"], &json!({}), None, false, &|| {})
        .unwrap()
        .unwrap();
    assert_eq!(status, 200);
}

// ---- Person-only cards: a CI card (`ci_fix`, `ci_push`) ----------------------------------------

#[test]
fn a_ci_card_is_person_only_and_may_wait_a_day_and_no_longer() {
    assert!(PERSON_ONLY_KINDS.contains(&CI_KIND_FIX) && PERSON_ONLY_KINDS.contains(&CI_KIND_PUSH));
    let store = QuestionStore::new();
    for kind in [CI_KIND_FIX, CI_KIND_PUSH] {
        let day = NewQuestion {
            question: format!("{kind} day"),
            choices: choices(),
            kind: Some(kind.into()),
            timeout_s: Some(24 * 3600),
            ..Default::default()
        };
        assert!(store.create(day.clone(), 0).is_ok(), "{kind}: 24 hours is accepted");
        let too_long = NewQuestion { timeout_s: Some(24 * 3600 + 1), ..day };
        let refused = store.create(too_long, 0).unwrap_err();
        assert_eq!(refused.code, "INVALID_ARGUMENT", "{kind}: 25 hours is refused");
    }
}

#[test]
fn an_agent_cannot_ask_for_a_ci_card_over_ask_user() {
    // `ask_user` never sets a kind: an agent cannot raise a card that is only the person's to answer.
    for kind in [CI_KIND_FIX, CI_KIND_PUSH] {
        let body = json!({ "question": "CI failed on #61. Ask Codex to fix it?", "choices": [{"id": "fix", "label": "Ask Codex to fix"}], "kind": kind, "wait_ms": 1 });
        let (status, reply) = routes::handle("POST", &["questions", "ask"], &body, Some("agent-ci-kind"), false, &|| {})
            .unwrap()
            .unwrap();
        assert_eq!(status, 200, "{kind}");
        let id = reply["question_id"].as_str().unwrap().to_string();
        assert_eq!(store().get(&id, now_ms()).unwrap().kind, None, "{kind}");
    }
}

#[test]
fn only_the_person_answers_or_dismisses_a_ci_card_over_the_bridge() {
    for kind in [CI_KIND_FIX, CI_KIND_PUSH] {
        let card = issue_card(store(), kind, &format!("CI card {kind} [loopback]"));
        let answer = json!({ "choice_id": "yes" });
        // Plain loopback HTTP (an agent's shell, a companion, any local program): refused, the card stays pending.
        let (status, body) = routes::handle("POST", &["questions", &card.id, "answer"], &answer, None, false, &|| {})
            .unwrap()
            .unwrap_err();
        assert_eq!((status, body["code"].as_str()), (403, Some("CI_FIX_NEEDS_PERSON")), "{kind}");
        assert_eq!(store().get(&card.id, now_ms()).unwrap().state, QuestionState::Pending, "{kind}");
        let (status, body) = routes::handle("POST", &["questions", &card.id, "dismiss"], &json!({}), None, false, &|| {})
            .unwrap()
            .unwrap_err();
        assert_eq!((status, body["code"].as_str()), (403, Some("CI_FIX_NEEDS_PERSON")), "{kind}");
        assert_eq!(store().get(&card.id, now_ms()).unwrap().state, QuestionState::Pending, "{kind}");
        // A paired phone is the person: it may answer.
        let (status, _) = routes::handle("POST", &["questions", &card.id, "answer"], &answer, None, true, &|| {})
            .unwrap()
            .unwrap();
        assert_eq!(status, 200, "{kind}");
        assert_eq!(store().get(&card.id, now_ms()).unwrap().state, QuestionState::Answered, "{kind}");
    }
}

#[test]
fn a_phone_may_dismiss_a_ci_card() {
    let card = issue_card(store(), CI_KIND_PUSH, "Push the fix to #61? [dismiss]");
    let (status, _) = routes::handle("POST", &["questions", &card.id, "dismiss"], &json!({}), None, true, &|| {})
        .unwrap()
        .unwrap();
    assert_eq!(status, 200);
    assert_eq!(store().get(&card.id, now_ms()).unwrap().state, QuestionState::Cancelled);
}
