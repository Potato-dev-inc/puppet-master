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
