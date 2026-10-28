//! The Team tools in the shim (`dispatch/team.rs`): each call is one bridge request with the
//! arguments untouched, `team_wait` is a long-poll in short slices, and the app's errors reach the
//! agent with their codes. The bridge is the in-process mock of `tests.rs`, which hands every
//! `/team/agent/*` request to [`mock`].

use super::*;
use std::collections::VecDeque;
use std::sync::Mutex;

#[derive(Debug, Clone)]
pub(super) struct Logged {
    pub path: String,
    pub body: Value,
    pub session: Option<String>,
}

#[derive(Default)]
struct Mock {
    log: Vec<Logged>,
    /// Answers for the next requests, oldest first; then a quiet `team_wait` / an echo.
    script: VecDeque<(u16, Value)>,
}

static MOCK: Mutex<Mock> = Mutex::new(Mock { log: Vec::new(), script: VecDeque::new() });
/// The tests share one mock bridge: run them one at a time.
static SERIAL: Mutex<()> = Mutex::new(());

/// The mock bridge's answer to `POST path` with `body`.
pub(super) fn mock(path: &str, head: &str, body: &Value) -> (u16, Value) {
    let session = head
        .lines()
        .find_map(|l| l.split_once(':').filter(|(k, _)| k.eq_ignore_ascii_case("x-puppet-master-session")).map(|(_, v)| v.trim().to_string()));
    let mut mock = MOCK.lock().unwrap();
    mock.log.push(Logged { path: path.to_string(), body: body.clone(), session });
    if let Some(next) = mock.script.pop_front() {
        return next;
    }
    drop(mock);
    if path == "/team/agent/wait" {
        // A real bridge blocks for `wait_ms`; a short nap keeps the loop from spinning.
        std::thread::sleep(Duration::from_millis(30));
        return (200, json!({ "matched": false, "since": body.get("since").cloned().unwrap_or(json!(1000)) }));
    }
    (200, json!({ "ok": true, "path": path, "echo": body }))
}

struct Run {
    _guard: std::sync::MutexGuard<'static, ()>,
}

fn start(script: Vec<(u16, Value)>) -> Run {
    ensure_questions_bridge();
    let guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    {
        let mut mock = MOCK.lock().unwrap();
        mock.log.clear();
        mock.script = script.into();
    }
    set_mcp_session_id("shim-team-test-session".to_string());
    mcp_sessions::set_mode("shim-team-test-session", "agent").unwrap();
    Run { _guard: guard }
}

impl Drop for Run {
    fn drop(&mut self) {
        MCP_SESSION_ID.with(|slot| *slot.borrow_mut() = None);
    }
}

fn log() -> Vec<Logged> {
    MOCK.lock().unwrap().log.clone()
}

fn error_of(result: Result<Value, String>) -> Value {
    let text = result.expect_err("an error result");
    serde_json::from_str(&text).unwrap_or_else(|_| panic!("a JSON error, got {text}"))
}

#[test]
fn roster_events_and_notify_are_one_request_each_with_the_arguments_untouched() {
    let _run = start(vec![]);
    let cases = [
        ("team_roster", "/team/agent/roster", json!({})),
        ("team_events", "/team/agent/events", json!({ "project": "billing", "kinds": ["pushed", "pr_merged"], "since": 5, "member": "Maya", "limit": 3 })),
        ("team_notify", "/team/agent/notify", json!({ "project": "billing", "kind": "pushed", "refs": { "branch": "feat/x" }, "note": "ready" })),
    ];
    for (tool, path, args) in cases {
        let result = call_tool(json!({ "name": tool, "arguments": args.clone() })).unwrap_or_else(|e| panic!("{tool}: {e}"));
        assert_eq!(result["structuredContent"]["path"], path, "{tool}");
        let sent = log().last().cloned().unwrap();
        assert_eq!(sent.path, path);
        assert_eq!(sent.body, args, "{tool}: the shim adds nothing and drops nothing");
        assert_eq!(sent.session.as_deref(), Some("shim-team-test-session"), "{tool} carries the MCP session");
    }
    assert_eq!(log().len(), 3);
}

#[test]
fn the_apps_errors_reach_the_agent_with_their_codes() {
    let team_off = json!({ "code": "TEAM_OFF", "error": "Team is off.", "message": "Team is off.", "recoverable": false, "context": {} });
    let removed = json!({ "code": "NOT_A_MEMBER", "error": "gone", "message": "gone", "recoverable": false, "context": {} });
    let local_only = json!({ "code": "TEAM_LOCAL_ONLY", "error": "no", "message": "no", "recoverable": false, "context": {} });
    let _run = start(vec![(409, team_off), (403, removed), (403, local_only), (409, json!({ "code": "NO_TEAM", "error": "x", "message": "x", "recoverable": false, "context": {} }))]);
    for (tool, code) in [("team_roster", "TEAM_OFF"), ("team_events", "NOT_A_MEMBER"), ("team_notify", "TEAM_LOCAL_ONLY")] {
        let error = error_of(call_tool(json!({ "name": tool, "arguments": { "project": "p", "kind": "note", "note": "n" } })));
        assert_eq!(error["code"], code, "{tool}");
        assert_eq!(error["recoverable"], false);
    }
    // team_wait stops at the first refusal instead of polling a team that is not there.
    let error = error_of(call_tool(json!({ "name": "team_wait", "arguments": { "timeout_ms": 5000 } })));
    assert_eq!(error["code"], "NO_TEAM");
    assert_eq!(log().iter().filter(|l| l.path == "/team/agent/wait").count(), 1);
}

#[test]
fn team_wait_polls_in_slices_keeps_its_starting_point_and_returns_the_event() {
    let event = json!({ "notice": "n", "matched": true, "event": { "id": "E1", "kind": "pr_merged", "teammate_data": { "pr": { "number": 142 } } }, "next_since": 4242 });
    let _run = start(vec![
        (200, json!({ "matched": false, "since": 777 })),
        (200, json!({ "matched": false, "since": 777 })),
        (200, event.clone()),
    ]);
    let filter = json!({ "kinds": ["pr_merged"], "pr": 142 });
    let result = call_tool(json!({ "name": "team_wait", "arguments": { "filter": filter, "timeout_ms": 20000 } })).unwrap();
    assert_eq!(result["structuredContent"], event);
    let waits: Vec<Logged> = log().into_iter().filter(|l| l.path == "/team/agent/wait").collect();
    assert_eq!(waits.len(), 3);
    // The first slice has no starting point: the app picks the moment the wait began and says so.
    assert!(waits[0].body.get("since").is_none(), "{:?}", waits[0].body);
    // Every later slice keeps it, so an event between two slices is not missed.
    assert_eq!(waits[1].body["since"], 777);
    assert_eq!(waits[2].body["since"], 777);
    for wait in &waits {
        assert_eq!(wait.body["filter"], filter);
        let slice = wait.body["wait_ms"].as_u64().unwrap();
        assert!((1..=4000).contains(&slice), "short bridge slices: {slice}");
    }
}

#[test]
fn an_explicit_since_is_sent_from_the_first_slice() {
    let _run = start(vec![(200, json!({ "matched": true, "event": {}, "next_since": 1 }))]);
    call_tool(json!({ "name": "team_wait", "arguments": { "filter": { "since": 0, "kinds": ["pushed"] } } })).unwrap();
    assert_eq!(log()[0].body["since"], 0, "since: 0 means \"take what is already there\"");
}

#[test]
fn team_wait_ends_in_a_recoverable_wait_timeout_that_says_where_to_continue() {
    let _run = start(vec![]);
    let started = std::time::Instant::now();
    let error = error_of(call_tool(json!({ "name": "team_wait", "arguments": { "filter": { "kinds": ["pushed"] }, "timeout_ms": 300 } })));
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(error["code"], "WAIT_TIMEOUT");
    assert_eq!(error["recoverable"], true);
    assert_eq!(error["since"], 1000, "the starting point the app gave");
    assert_eq!(error["next_action"]["tool"], "team_wait");
    assert_eq!(error["next_action"]["filter"]["since"], 1000);
    assert_eq!(error["next_action"]["filter"]["kinds"], json!(["pushed"]));
}

#[test]
fn team_wait_timeout_is_bounded_like_wait_agents() {
    assert_eq!(dispatch::team_wait_timeout_ms(&json!({})), 30_000);
    assert_eq!(dispatch::team_wait_timeout_ms(&json!({ "timeout_ms": 5_000 })), 5_000);
    assert_eq!(dispatch::team_wait_timeout_ms(&json!({ "timeout_ms": 10_000_000u64 })), 120_000, "max 120 s");
    assert_eq!(dispatch::team_wait_timeout_ms(&json!({ "timeout_ms": 0 })), 1);
}

#[test]
fn cancelling_team_wait_stops_it_quickly() {
    let _run = start(vec![]);
    let (cancel_tx, cancel_rx) = std::sync::mpsc::channel();
    let call = std::thread::spawn(move || {
        set_mcp_session_id("shim-team-test-session".to_string());
        call_tool_cancellable(json!({ "name": "team_wait", "arguments": { "timeout_ms": 120000 } }), Some(cancel_rx))
    });
    std::thread::sleep(Duration::from_millis(200));
    assert!(!call.is_finished(), "it waits");
    cancel_tx.send(()).unwrap();
    let error = error_of(call.join().unwrap());
    assert_eq!(error["code"], "REQUEST_CANCELLED");
}

#[test]
fn a_filter_that_is_not_an_object_is_refused_before_the_bridge() {
    let _run = start(vec![]);
    let error = call_tool(json!({ "name": "team_wait", "arguments": { "filter": "pushed" } })).unwrap_err();
    assert!(error.contains("filter must be an object"), "{error}");
    assert!(log().is_empty());
}

#[test]
fn the_tools_are_listed_in_all_mode_and_callable_by_name_in_agent_mode_only() {
    let _run = start(vec![]);
    let listed = |mode: &str| -> Vec<String> {
        mcp_sessions::set_mode("shim-team-test-session", mode).unwrap();
        let line = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {} }).to_string();
        let response: Value = serde_json::from_str(&handle_json_rpc_line(&line).unwrap()).unwrap();
        response["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect()
    };
    let team = ["team_roster", "team_events", "team_wait", "team_notify"];
    let agent = listed("agent");
    let all = listed("all");
    for name in team {
        assert!(!agent.iter().any(|n| n == name), "{name} is not listed by default");
        assert!(all.iter().any(|n| n == name), "{name} is listed in all mode");
        mcp_sessions::set_mode("shim-team-test-session", "agent").unwrap();
        assert!(ensure_tool_mode(name).is_ok(), "{name} is callable by name in agent mode");
        mcp_sessions::set_mode("shim-team-test-session", "shell").unwrap();
        assert!(ensure_tool_mode(name).is_err(), "{name} is not callable in shell mode");
    }
}

// ---- companion_recall (the same mock bridge) ----

#[test]
fn companion_recall_is_one_request_with_the_arguments_untouched_and_the_apps_refusal_reaches_the_agent() {
    let refusal = json!({ "code": "COMPANION_ONLY", "error": "Only a companion.", "message": "Only a companion.", "recoverable": false, "context": {} });
    let _run = start(vec![(200, json!({ "enabled": true, "results": [] })), (403, refusal)]);
    let args = json!({ "query": "green tea", "limit": 3 });
    let ok = call_tool(json!({ "name": "companion_recall", "arguments": args.clone() })).unwrap();
    assert_eq!(ok["structuredContent"]["enabled"], true);
    let sent = log().last().cloned().unwrap();
    assert_eq!((sent.path.as_str(), sent.body.clone()), ("/companion/recall", args));
    assert_eq!(sent.session.as_deref(), Some("shim-team-test-session"));
    // From a pane that is not a companion's, the app says no and the agent reads the code.
    let error = error_of(call_tool(json!({ "name": "companion_recall", "arguments": { "query": "tea" } })));
    assert_eq!(error["code"], "COMPANION_ONLY");
}

#[test]
fn companion_recall_is_listed_only_for_an_agent_started_in_a_companion_pane() {
    let _run = start(vec![]);
    let names = |pane: Option<&str>| -> Vec<String> {
        stdio::mcp_tools_for_pane(pane.map(str::to_string)).iter().map(|t| t["name"].as_str().unwrap().to_string()).collect()
    };
    assert!(names(Some("companion-mochi")).contains(&"companion_recall".to_string()));
    assert!(!names(Some("5f3c-ordinary-pane")).contains(&"companion_recall".to_string()));
    assert!(!names(None).contains(&"companion_recall".to_string()));
    // Added once, never doubled, next to the usual agent tools.
    let listed = names(Some("companion-mochi"));
    assert_eq!(listed.iter().filter(|n| *n == "companion_recall").count(), 1);
    assert!(listed.contains(&"run_agent".to_string()));
}
