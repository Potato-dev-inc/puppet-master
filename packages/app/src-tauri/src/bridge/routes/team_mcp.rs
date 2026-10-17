//! `/team/agent/*`: the Team tools for agents (`team_roster`, `team_events`, `team_wait`,
//! `team_notify`; logic in `crate::team::mcp`).
//!
//! | Route | Tool |
//! |---|---|
//! | `POST /team/agent/roster` | `team_roster` |
//! | `POST /team/agent/events` | `team_events` |
//! | `POST /team/agent/wait` | `team_wait` (one bounded block; the MCP shim repeats it) |
//! | `POST /team/agent/notify` | `team_notify` (a draft; the person sends it) |
//!
//! Who may call: only an MCP session of an agent on **this computer**. The router has already
//! checked the bearer token and the device caps; on top of that these routes refuse every paired
//! phone (whatever its caps, and even if it sends a session header), every request that carries a
//! forwarder header, a foreign `Host` or a foreign `Origin` (a tunnel, a web page: the bridge-auth
//! invariant), and every caller without an MCP session. Team traffic itself never uses the bridge:
//! these routes only read the local app's own team state.

use super::{Ctx, RouteResult};
use crate::mobile_pairing::Caller;
use crate::team::mcp::McpError;
use serde_json::{json, Value};
use std::time::Duration;

/// One `team_wait` block on the bridge; the shim repeats it, like `ask_user`.
pub(crate) const MAX_WAIT_SLICE_MS: u64 = 25_000;

/// The local app's team state, as the tools see it. The live one is [`LiveTeam`]; tests use a
/// recording fake to prove which callers get as far as the team at all.
pub(crate) trait TeamBackend {
    fn roster(&self) -> Result<Value, McpError>;
    fn events(&self, args: &Value) -> Result<Value, McpError>;
    fn wait(&self, filter: &Value, since: u64, wait: Duration) -> Result<Value, McpError>;
    fn notify(&self, args: &Value, session: &str) -> Result<Value, McpError>;
}

/// This process's Team service.
pub(crate) struct LiveTeam;

impl TeamBackend for LiveTeam {
    fn roster(&self) -> Result<Value, McpError> {
        crate::team::service::mcp_roster()
    }
    fn events(&self, args: &Value) -> Result<Value, McpError> {
        crate::team::service::mcp_events(args)
    }
    fn wait(&self, filter: &Value, since: u64, wait: Duration) -> Result<Value, McpError> {
        crate::team::service::mcp_wait(filter, since, wait)
    }
    fn notify(&self, args: &Value, session: &str) -> Result<Value, McpError> {
        crate::team::service::mcp_notify(args, session)
    }
}

pub(super) fn handle(cx: &Ctx) -> Option<RouteResult> {
    handle_request(cx.method, cx.segments, &cx.request_body, cx.headers, cx.peer_loopback, cx.session, &LiveTeam)
}

fn refuse(status: u16, code: &str, message: &str) -> RouteResult {
    Err((status, json!({ "code": code, "error": message, "message": message, "recoverable": false, "context": {} })))
}

fn answer(result: Result<Value, McpError>) -> RouteResult {
    result.map(|value| (200, value)).map_err(|error| error.to_http())
}

/// `None` when the request is not a `/team/agent/*` route.
pub(crate) fn handle_request(
    method: &str,
    raw_segments: &[&str],
    body: &Value,
    headers: &str,
    peer_loopback: bool,
    session: Option<&str>,
    backend: &dyn TeamBackend,
) -> Option<RouteResult> {
    let segments = crate::mobile_pairing::normalize_bridge_segments(raw_segments);
    let ["team", "agent", op] = segments else { return None };
    // Who is calling, by the bridge's own rule: `Local` is only a loopback peer with no sign of a
    // forwarder, a foreign Host or a foreign Origin. A bearer token turns the caller into a
    // `Device` (a phone), which these routes never serve.
    match crate::mobile_pairing::authorize_caller(headers, peer_loopback, raw_segments, method) {
        Ok(Caller::Local) => {}
        Ok(_) | Err(_) => {
            return Some(refuse(
                403,
                "TEAM_LOCAL_ONLY",
                "Team tools are for the agents on this computer. A phone or any remote caller cannot use them.",
            ))
        }
    }
    let Some(session) = session.filter(|s| !s.trim().is_empty()) else {
        return Some(refuse(400, "SESSION_REQUIRED", "Team tools need an MCP session id."));
    };
    if method != "POST" {
        return Some(refuse(405, "METHOD_NOT_ALLOWED", "Use POST."));
    }
    Some(match *op {
        "roster" => answer(backend.roster()),
        "events" => answer(backend.events(body)),
        "wait" => {
            let filter = body.get("filter").cloned().unwrap_or_else(|| json!({}));
            // The caller's starting point: `since` of the call, else the filter's own, else the moment this wait began.
            let since = body
                .get("since")
                .and_then(Value::as_u64)
                .or_else(|| filter.get("since").and_then(Value::as_u64))
                .unwrap_or_else(|| crate::team::service::now_ms().saturating_sub(1));
            let wait_ms = body.get("wait_ms").and_then(Value::as_u64).unwrap_or(MAX_WAIT_SLICE_MS).min(MAX_WAIT_SLICE_MS);
            answer(backend.wait(&filter, since, Duration::from_millis(wait_ms)))
        }
        "notify" => answer(backend.notify(body, session)),
        _ => refuse(404, "NOT_FOUND", "Unknown team tool route."),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// Records which calls reached the team.
    #[derive(Default)]
    struct Spy {
        calls: RefCell<Vec<String>>,
    }

    impl TeamBackend for Spy {
        fn roster(&self) -> Result<Value, McpError> {
            self.calls.borrow_mut().push("roster".into());
            Ok(json!({ "members": [] }))
        }
        fn events(&self, args: &Value) -> Result<Value, McpError> {
            self.calls.borrow_mut().push(format!("events {args}"));
            Ok(json!({ "events": [] }))
        }
        fn wait(&self, filter: &Value, since: u64, wait: Duration) -> Result<Value, McpError> {
            self.calls.borrow_mut().push(format!("wait {filter} {since} {}", wait.as_millis()));
            Ok(json!({ "matched": false, "since": since }))
        }
        fn notify(&self, args: &Value, session: &str) -> Result<Value, McpError> {
            self.calls.borrow_mut().push(format!("notify {session} {args}"));
            Ok(json!({ "drafted": true, "sent": false }))
        }
    }

    const LOCAL: &str = "Host: 127.0.0.1:17321\r\nContent-Type: application/json\r\n";

    fn call(spy: &Spy, op: &str, headers: &str, loopback: bool, session: Option<&str>) -> RouteResult {
        handle_request("POST", &["team", "agent", op], &json!({ "project": "billing" }), headers, loopback, session, spy).expect("a team/agent route")
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
    fn a_local_mcp_session_reaches_every_tool() {
        let spy = Spy::default();
        for op in ["roster", "events", "wait", "notify"] {
            let (status, _) = call(&spy, op, LOCAL, true, Some("session-1")).unwrap();
            assert_eq!(status, 200, "{op}");
        }
        assert_eq!(spy.calls.borrow().len(), 4);
        assert!(spy.calls.borrow()[3].starts_with("notify session-1 "), "the draft is attributed to the session");
    }

    #[test]
    fn the_wait_slice_is_bounded_and_defaults_to_now() {
        let spy = Spy::default();
        let body = json!({ "filter": { "kinds": ["pushed"] }, "wait_ms": 10_000_000u64 });
        handle_request("POST", &["team", "agent", "wait"], &body, LOCAL, true, Some("s"), &spy).unwrap().unwrap();
        let call = spy.calls.borrow()[0].clone();
        assert!(call.ends_with(&format!(" {MAX_WAIT_SLICE_MS}")), "{call}");
        // `since` was filled in with about now.
        let since: u64 = call.split(' ').nth(2).unwrap().parse().unwrap();
        assert!(crate::team::service::now_ms().saturating_sub(since) < 5_000, "{call}");
    }

    #[test]
    fn a_request_that_is_not_a_team_agent_route_is_left_alone() {
        let spy = Spy::default();
        for segments in [&["team", "cards"][..], &["team", "agent"], &["team", "agent", "roster", "x"], &["panes"]] {
            assert!(handle_request("POST", segments, &json!({}), LOCAL, true, Some("s"), &spy).is_none(), "{segments:?}");
        }
        assert!(spy.calls.borrow().is_empty());
    }

    #[test]
    fn no_mcp_session_no_team() {
        let spy = Spy::default();
        refused_with(call(&spy, "roster", LOCAL, true, None), 400, "SESSION_REQUIRED");
        refused_with(call(&spy, "notify", LOCAL, true, Some("  ")), 400, "SESSION_REQUIRED");
        assert!(spy.calls.borrow().is_empty());
    }

    #[test]
    fn a_remote_caller_never_reaches_the_team() {
        let spy = Spy::default();
        // A tunnel / reverse proxy on this machine (forwarder headers), a web page (foreign Origin),
        // DNS rebinding (foreign Host), a non-loopback peer: each is refused even WITH a session
        // header, and even when a bearer token is sent (the bearer is checked by the gate first;
        // here it is simply not valid, and a valid one is a phone, tested below).
        let cases = [
            ("forwarded", format!("{LOCAL}X-Forwarded-For: 203.0.113.9\r\n"), true),
            ("cloudflare", format!("{LOCAL}Cf-Connecting-Ip: 203.0.113.9\r\n"), true),
            ("pwa proxy", format!("{LOCAL}X-PM-Proxied: 1\r\n"), true),
            ("web page", format!("{LOCAL}Origin: https://evil.example\r\n"), true),
            ("null origin", format!("{LOCAL}Origin: null\r\n"), true),
            ("foreign host", "Host: team.example.com\r\n".to_string(), true),
            ("remote peer", LOCAL.to_string(), false),
            ("bearer from nowhere", format!("{LOCAL}Authorization: Bearer not-a-token\r\nX-Forwarded-For: 1.2.3.4\r\n"), true),
        ];
        for (why, headers, loopback) in cases {
            for op in ["roster", "events", "wait", "notify"] {
                refused_with(call(&spy, op, &headers, loopback, Some("session-1")), 403, "TEAM_LOCAL_ONLY");
            }
            let _ = why;
        }
        assert!(spy.calls.borrow().is_empty(), "{:?}", spy.calls.borrow());
    }

    /// A phone that is paired and holds every capability: the strongest bearer there is. The bridge
    /// lets it through (it may do everything on a phone's menu); these routes do not.
    #[test]
    fn a_paired_phone_with_every_cap_is_refused_even_with_a_session_header() {
        use crate::mobile_pairing::{init_pairing_store, pairing_store, Caps, PairRequestBody};
        let dir = std::env::temp_dir().join(format!("pm-team-mcp-phone-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let _ = init_pairing_store(dir.join("pairing.json"));
        let token = {
            let store = pairing_store().expect("pairing store");
            let mut guard = store.lock();
            let session = guard.create_pairing_session_with_caps("https://example.invalid".into(), Caps::all());
            guard
                .pair_device(PairRequestBody {
                    pairing_code: session.pairing_code,
                    device_name: "test phone".into(),
                    device_public_key: session.server_public_key,
                })
                .expect("pair a test phone")
                .device_token
        };
        let spy = Spy::default();
        // Through a tunnel: a remote peer with the bearer, or a loopback forwarder (cloudflared, the
        // PWA proxy) that adds its headers. Both are a Device to the gate.
        for headers in [
            format!("Host: tunnel.example\r\nAuthorization: Bearer {token}\r\n"),
            format!("{LOCAL}Authorization: Bearer {token}\r\nX-Forwarded-For: 203.0.113.9\r\n"),
            format!("{LOCAL}Authorization: Bearer {token}\r\nCf-Connecting-Ip: 203.0.113.9\r\n"),
            format!("{LOCAL}Authorization: Bearer {token}\r\nX-PM-Proxied: 1\r\n"),
            format!("{LOCAL}Authorization: Bearer {token}\r\nOrigin: https://evil.example\r\n"),
        ] {
            // The gate itself knows this caller as a device that may do everything (`loopback` is
            // the TCP peer: a tunnel's own process on this computer, or a remote address)...
            let gate = crate::mobile_pairing::authorize_caller(&headers, true, &["team", "agent", "notify"], "POST");
            assert!(matches!(gate, Ok(crate::mobile_pairing::Caller::Device { .. })), "{gate:?}");
            // ...and the team routes still say no.
            for op in ["roster", "events", "wait", "notify"] {
                refused_with(call(&spy, op, &headers, true, Some("spoofed-session")), 403, "TEAM_LOCAL_ONLY");
            }
        }
        assert!(spy.calls.borrow().is_empty(), "{:?}", spy.calls.borrow());
    }

    #[test]
    fn only_post_is_served() {
        let spy = Spy::default();
        let reply = handle_request("GET", &["team", "agent", "roster"], &json!({}), LOCAL, true, Some("s"), &spy).unwrap();
        refused_with(reply, 405, "METHOD_NOT_ALLOWED");
        assert!(spy.calls.borrow().is_empty());
    }

    #[test]
    fn errors_use_the_codes_of_the_other_mcp_routes() {
        struct Off;
        impl TeamBackend for Off {
            fn roster(&self) -> Result<Value, McpError> {
                Err(McpError::team_off("Team is off."))
            }
            fn events(&self, _: &Value) -> Result<Value, McpError> {
                Err(McpError::no_team("no team"))
            }
            fn wait(&self, _: &Value, _: u64, _: Duration) -> Result<Value, McpError> {
                Err(McpError::not_a_member())
            }
            fn notify(&self, _: &Value, _: &str) -> Result<Value, McpError> {
                Err(McpError::invalid("bad"))
            }
        }
        let expect = [("roster", "TEAM_OFF"), ("events", "NO_TEAM"), ("wait", "NOT_A_MEMBER"), ("notify", "INVALID_ARGUMENT")];
        for (op, code) in expect {
            let reply = handle_request("POST", &["team", "agent", op], &json!({}), LOCAL, true, Some("s"), &Off).unwrap();
            let Err((_, body)) = reply else { panic!("expected an error") };
            assert_eq!(body["code"], code);
            // The shape the MCP shim passes through untouched: code + recoverable + message.
            assert!(body["recoverable"].is_boolean() && body["message"].is_string(), "{body}");
        }
    }
}
