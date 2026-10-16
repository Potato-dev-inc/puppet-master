//! `/agents/wizard/{start,probe,capture,verify,livetest,visibletest,finish,cancel}` (docs/specs/agent-wizard/SPEC.md).
//!
//! The Agent Wizard runs programs and writes agent definitions, so, like `/agents/definitions`, it
//! belongs to the local desktop app only: an MCP session (any request carrying the session header)
//! is refused here as well as by `mcp_sessions::authorize`, and so is anything that is not the local
//! app (a paired phone, the tunnel, a proxied or bearer request). This group runs before the
//! agent-run router, whose `/agents/{handle}` would otherwise swallow the paths.

use serde_json::{json, Value};

use super::definitions::is_local_app;
use super::{Ctx, RouteResult};
use crate::tui::declarative::wizard::api;

pub(super) fn handle(cx: &Ctx) -> Option<RouteResult> {
    let local_app = is_local_app(cx.headers, cx.peer_loopback);
    respond(cx.method, cx.segments, &cx.request_body, cx.session, local_app, &|action, body| {
        api::handle_global(action, body).map(|v| (200, v)).map_err(|e| (e.status, e.to_json()))
    })
}

fn denied(message: &str) -> (u16, Value) {
    (403, json!({"code": "AUTHORIZATION_DENIED", "error": message, "message": message, "recoverable": false}))
}

/// The route table without the Tauri context, so tests can drive it with their own wizard.
pub(in crate::bridge) fn respond(
    method: &str,
    segments: &[&str],
    body: &Value,
    session: Option<&str>,
    local_app: bool,
    run: &dyn Fn(&str, &Value) -> RouteResult,
) -> Option<RouteResult> {
    let action = match (method, segments) {
        ("POST", ["agents", "wizard", action]) if api::ACTIONS.contains(action) => *action,
        _ => return None,
    };
    if session.is_some_and(|s| !s.trim().is_empty()) {
        return Some(Err(denied("the agent wizard belongs to the app session, not MCP sessions")));
    }
    if !local_app {
        return Some(Err(denied("the agent wizard can only be used from the local desktop app, not a remote device")));
    }
    Some(run(action, body))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::bridge::routes::definitions::is_local_app;
    use crate::mcp_sessions;
    use crate::tui::declarative::wizard::tests::support::Tree;

    const LOCAL: &str = "POST /agents/wizard/start HTTP/1.1\r\nHost: 127.0.0.1:17321\r\n";
    const TUNNEL: &str = "POST /agents/wizard/start HTTP/1.1\r\nX-PM-Proxied: 1\r\nAuthorization: Bearer tok\r\n";
    const PHONE: &str = "POST /agents/wizard/start HTTP/1.1\r\nAuthorization: Bearer tok\r\n";
    const FORWARDED: &str = "POST /agents/wizard/start HTTP/1.1\r\nCF-Connecting-IP: 1.2.3.4\r\n";

    fn refused(local_app: bool, session: Option<&str>, action: &str) -> (u16, Value) {
        respond("POST", &["agents", "wizard", action], &json!({}), session, local_app, &|_, _| {
            panic!("the wizard must not run for a refused caller")
        })
        .expect("the route is owned by this group")
        .unwrap_err()
    }

    #[test]
    fn a_remote_device_or_a_phone_is_refused_for_every_action() {
        for headers in [TUNNEL, PHONE, FORWARDED] {
            let local_app = is_local_app(headers, true);
            assert!(!local_app, "{headers}");
            for action in api::ACTIONS {
                let (status, body) = refused(local_app, None, action);
                assert_eq!((status, body["code"].as_str()), (403, Some("AUTHORIZATION_DENIED")), "{action}");
            }
        }
        assert!(!is_local_app(LOCAL, false), "a non-loopback peer is never the app");
        assert_eq!(refused(false, None, "start").0, 403);
    }

    #[test]
    fn an_mcp_session_is_refused_even_from_the_local_app() {
        for action in api::ACTIONS {
            let (status, body) = refused(true, Some("session-1"), action);
            assert_eq!(status, 403, "{action}");
            assert_eq!(body["code"], "AUTHORIZATION_DENIED");
        }
        // a blank header is not a session
        let tree = Tree::new("route-blank");
        let wizard = tree.wizard();
        let reply = respond("POST", &["agents", "wizard", "capture"], &json!({}), Some("  "), true, &|a, b| {
            api::handle(&wizard, a, b).map(|v| (200, v)).map_err(|e| (e.status, e.to_json()))
        });
        assert_eq!(reply.unwrap().unwrap_err().0, 400, "a blank header is not a session, and capture then needs a session id");
    }

    #[test]
    fn only_post_to_the_listed_actions_belongs_to_this_group() {
        let never = |_: &str, _: &Value| -> RouteResult { panic!("not routed") };
        assert!(respond("GET", &["agents", "wizard", "start"], &json!({}), None, true, &never).is_none());
        assert!(respond("POST", &["agents", "wizard", "nope"], &json!({}), None, true, &never).is_none());
        assert!(respond("POST", &["agents", "wizard"], &json!({}), None, true, &never).is_none());
        assert!(respond("POST", &["agents", "definitions", "validate"], &json!({}), None, true, &never).is_none());
        assert!(respond("POST", &["agents", "wizard", "start", "x"], &json!({}), None, true, &never).is_none());
    }

    #[test]
    fn the_local_app_reaches_the_wizard_and_errors_keep_the_route_shape() {
        let tree = Tree::new("route-local");
        let wizard = tree.wizard();
        let run = |a: &str, b: &Value| -> RouteResult { api::handle(&wizard, a, b).map(|v| (200, v)).map_err(|e| (e.status, e.to_json())) };
        let (status, started) =
            respond("POST", &["agents", "wizard", "start"], &json!({"command": "fake-agent"}), None, true, &run).unwrap().unwrap();
        assert_eq!(status, 200);
        assert_eq!(started["state"], "resolved");
        let (status, body) =
            respond("POST", &["agents", "wizard", "probe"], &json!({"session": "wz_none"}), None, true, &run).unwrap().unwrap_err();
        assert_eq!(status, 404);
        assert_eq!((body["code"].as_str(), body["recoverable"].as_bool()), (Some("WIZARD_SESSION_NOT_FOUND"), Some(false)));
        assert!(body["error"].is_string() && body["message"].is_string());
        let (status, body) = respond("POST", &["agents", "wizard", "capture"], &json!({}), None, true, &run).unwrap().unwrap_err();
        assert_eq!((status, body["code"].as_str()), (400, Some("INVALID_ARGUMENT")));
    }

    #[test]
    fn the_session_gate_lets_the_app_through_and_keeps_mcp_sessions_out() {
        for action in api::ACTIONS {
            let path = format!("/agents/wizard/{action}");
            assert!(mcp_sessions::authorize(None, "POST", &path, &json!({})).is_ok(), "{path}");
            let err = mcp_sessions::authorize(Some("mcp-session-1"), "POST", &path, &json!({})).unwrap_err();
            assert!(err.message.contains("app session"), "{path}: {}", err.message);
        }
        // the same prefix check must not open other agent paths for the app session
        assert!(mcp_sessions::authorize(None, "POST", "/agents/run", &json!({})).is_err());
    }
}
