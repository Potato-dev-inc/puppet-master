//! `POST /companion/recall`: the `companion_recall` tool (logic in `crate::companion::recall`).
//!
//! Who may call: only the agent of a companion, from inside that companion's own pane. The router has
//! already checked the bearer token and the device caps; on top of that this route refuses
//!
//! - every paired phone, every request that carries a forwarder header, a foreign `Host` or a foreign
//!   `Origin` (the bridge-auth invariant: `Caller::Local` only),
//! - every caller without an MCP session, and
//! - every connection whose process is not inside a live companion pane. The check is the process that owns
//!   the TCP connection, walked up the process tree (`companion::peer::recall_caller_pane`), not a header the
//!   caller writes: neither `X-Puppet-Master-Pane` nor `X-Puppet-Master-Pid` gets you anything. Where the
//!   connection's owner or the process table cannot be read (another OS today) the answer is no.
//!
//! The pane that passes the check is the only thing handed to the search: a caller cannot name another
//! companion, another pane or a wider scope.

use super::{Ctx, RouteResult};
use crate::companion::model::CompanionError;
use crate::companion::peer::{self, Peer};
use crate::mobile_pairing::Caller;
use serde_json::{json, Value};

/// What the route needs from the app. The live one is [`LiveRecall`]; tests use a recording fake to prove
/// which callers get as far as the search at all.
pub(crate) trait RecallBackend {
    /// The companion pane the process on the other end of this connection verifiably runs in. Decided from the
    /// TCP connection (`peer`), never from a process id the caller sends.
    fn caller_pane(&self, peer: Option<Peer>) -> Option<String>;
    fn recall(&self, pane_id: &str, query: &str, limit: Option<usize>) -> Result<Value, CompanionError>;
}

pub(crate) struct LiveRecall<'a> {
    registry: &'a std::sync::Arc<parking_lot::Mutex<crate::pty::PaneRegistry>>,
}

impl RecallBackend for LiveRecall<'_> {
    fn caller_pane(&self, peer: Option<Peer>) -> Option<String> {
        peer::recall_caller_pane(self.registry, &peer::SystemTcp, &crate::pty::agent_proc::snapshot, peer)
    }

    fn recall(&self, pane_id: &str, query: &str, limit: Option<usize>) -> Result<Value, CompanionError> {
        let service = crate::companion::live::service()
            .ok_or_else(|| CompanionError::new("COMPANION_NOT_READY", "Companions are not ready yet. Try again in a moment."))?;
        service.recall(pane_id, query, limit)
    }
}

pub(super) fn handle(cx: &Ctx) -> Option<RouteResult> {
    handle_request(
        cx.method,
        cx.segments,
        &cx.request_body,
        cx.headers,
        cx.peer_loopback,
        cx.peer,
        cx.session,
        &LiveRecall { registry: cx.registry },
    )
}

fn refuse(status: u16, code: &str, message: &str) -> RouteResult {
    Err((status, json!({ "code": code, "error": message, "message": message, "recoverable": false, "context": {} })))
}

/// `None` when the request is not `/companion/recall`.
pub(crate) fn handle_request(
    method: &str,
    raw_segments: &[&str],
    body: &Value,
    headers: &str,
    peer_loopback: bool,
    peer: Option<Peer>,
    session: Option<&str>,
    backend: &dyn RecallBackend,
) -> Option<RouteResult> {
    let segments = crate::mobile_pairing::normalize_bridge_segments(raw_segments);
    let ["companion", "recall"] = segments else { return None };
    match crate::mobile_pairing::authorize_caller(headers, peer_loopback, raw_segments, method) {
        Ok(Caller::Local) => {}
        Ok(_) | Err(_) => {
            return Some(refuse(
                403,
                "COMPANION_LOCAL_ONLY",
                "companion_recall is for a companion's own agent on this computer. A phone or any remote caller cannot use it.",
            ))
        }
    }
    if session.filter(|s| !s.trim().is_empty()).is_none() {
        return Some(refuse(400, "SESSION_REQUIRED", "companion_recall needs an MCP session id."));
    }
    if method != "POST" {
        return Some(refuse(405, "METHOD_NOT_ALLOWED", "Use POST."));
    }
    let Some(pane) = backend.caller_pane(peer) else {
        return Some(refuse(
            403,
            "COMPANION_ONLY",
            "companion_recall can only be used by a companion, from its own pane. It searches that companion's own chats and nothing else.",
        ));
    };
    let Some(query) = body.get("query").and_then(Value::as_str) else {
        return Some(refuse(400, "INVALID_ARGUMENT", "query is required: a few words to look for."));
    };
    let limit = body.get("limit").and_then(Value::as_u64).map(|n| n as usize);
    Some(backend.recall(&pane, query, limit).map(|value| (200, value)).map_err(|error| {
        let status = match error.code {
            "INVALID_ARGUMENT" => 400,
            "COMPANION_NOT_FOUND" => 404,
            "COMPANION_NOT_READY" => 503,
            _ => 500,
        };
        (status, json!({ "code": error.code, "error": error.message, "message": error.message, "recoverable": false, "context": {} }))
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// Records which calls reached the search, and answers with a fixed caller pane.
    struct Spy {
        pane: Option<&'static str>,
        calls: RefCell<Vec<String>>,
    }

    impl Spy {
        fn new(pane: Option<&'static str>) -> Self {
            Spy { pane, calls: RefCell::new(Vec::new()) }
        }
    }

    impl RecallBackend for Spy {
        fn caller_pane(&self, _peer: Option<Peer>) -> Option<String> {
            self.pane.map(str::to_string)
        }
        fn recall(&self, pane_id: &str, query: &str, limit: Option<usize>) -> Result<Value, CompanionError> {
            self.calls.borrow_mut().push(format!("{pane_id}|{query}|{limit:?}"));
            Ok(json!({ "enabled": true, "results": [] }))
        }
    }

    const LOCAL: &str = "Host: 127.0.0.1:17321\r\nContent-Type: application/json\r\n";

    fn call(spy: &Spy, headers: &str, loopback: bool, session: Option<&str>, body: Value) -> RouteResult {
        handle_request("POST", &["companion", "recall"], &body, headers, loopback, None, session, spy).expect("a companion/recall route")
    }

    fn refused_with(result: RouteResult, status: u16, code: &str) {
        match result {
            Err((got, body)) => {
                assert_eq!(got, status, "{body}");
                assert_eq!(body["code"], code, "{body}");
            }
            Ok(ok) => panic!("expected {code}, got {ok:?}"),
        }
    }

    #[test]
    fn a_companions_own_agent_reaches_the_search_with_its_own_pane_and_nothing_else() {
        let spy = Spy::new(Some("companion-mochi"));
        // A caller cannot widen the scope: a `pane_id` or `companion` in the body is ignored.
        let body = json!({ "query": "tea", "limit": 3, "pane_id": "companion-bit", "companion": "bit", "panes": ["x"] });
        let (status, _) = call(&spy, LOCAL, true, Some("s1"), body).unwrap();
        assert_eq!(status, 200);
        assert_eq!(*spy.calls.borrow(), vec!["companion-mochi|tea|Some(3)".to_string()]);
    }

    #[test]
    fn recall_is_refused_from_any_pane_that_is_not_a_companions() {
        let spy = Spy::new(None);
        refused_with(call(&spy, LOCAL, true, Some("s1"), json!({ "query": "tea" })), 403, "COMPANION_ONLY");
        assert!(spy.calls.borrow().is_empty());
    }

    #[test]
    fn recall_is_refused_without_a_session_and_from_phones_and_forwarders() {
        let spy = Spy::new(Some("companion-mochi"));
        refused_with(call(&spy, LOCAL, true, None, json!({ "query": "tea" })), 400, "SESSION_REQUIRED");
        refused_with(call(&spy, LOCAL, true, Some("  "), json!({ "query": "tea" })), 400, "SESSION_REQUIRED");
        // Not a loopback peer.
        refused_with(call(&spy, LOCAL, false, Some("s1"), json!({ "query": "tea" })), 403, "COMPANION_LOCAL_ONLY");
        // A forwarder header, a foreign Host, a foreign Origin.
        for headers in [
            "Host: 127.0.0.1:17321\r\nX-Forwarded-For: 203.0.113.9\r\n",
            "Host: evil.example.com\r\n",
            "Host: 127.0.0.1:17321\r\nOrigin: https://evil.example.com\r\n",
        ] {
            refused_with(call(&spy, headers, true, Some("s1"), json!({ "query": "tea" })), 403, "COMPANION_LOCAL_ONLY");
        }
        assert!(spy.calls.borrow().is_empty());
    }

    #[test]
    fn a_missing_query_is_a_plain_error_and_other_routes_are_left_alone() {
        let spy = Spy::new(Some("companion-mochi"));
        refused_with(call(&spy, LOCAL, true, Some("s1"), json!({})), 400, "INVALID_ARGUMENT");
        for segments in [&["companion"][..], &["companions", "recall"], &["companion", "recall", "x"], &["panes"]] {
            assert!(handle_request("POST", segments, &json!({}), LOCAL, true, None, Some("s1"), &spy).is_none(), "{segments:?}");
        }
        assert!(spy.calls.borrow().is_empty());
    }
}
