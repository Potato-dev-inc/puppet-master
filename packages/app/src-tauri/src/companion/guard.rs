//! The hard rule: a companion never answers a card. Not its own, and above all not its helpers'.
//!
//! A companion can start helpers (`run_agent`) and so, by the usual ownership rules, it "owns" their
//! panes. That ownership must never turn into the right to say yes to a helper's trust, permission or
//! question card: those go to the person, always. The companion may SEE that a helper is waiting (the
//! waiting status of `wait_agents` / `list_agents`); it has no tool and no route to answer.
//!
//! The bridge router calls `check` for every MCP request, with whether the caller is a companion
//! (`caller::Verdict::suspected`: where it runs, or where it says it runs). The routes are the ones that
//! answer a dialog or press a key into a pane. The person's own answers (the desktop app, a paired
//! phone) are not MCP requests and never come through here.

use crate::mcp_sessions::route_matches;
use crate::operations::OperationError;

/// `(method, route)` of every catalog route that answers a prompt, a permission or a question, or types
/// into a pane (which could do the same). `answer_prompt`, `reply_opencode_permission`,
/// `reply_opencode_question`, `write_terminal_input`, `press_key`.
pub const ANSWERING_ROUTES: &[(&str, &str)] = &[
    ("POST", "/agents/answer"),
    ("POST", "/panes/{pane_id}/opencode/permissions/{request_id}/reply"),
    ("POST", "/panes/{pane_id}/opencode/question/reply"),
    ("POST", "/panes/{pane_id}/input"),
    ("POST", "/panes/{pane_id}/key"),
];

/// Whether this request is one that answers a card or types into a pane.
pub fn is_answering_route(method: &str, path: &str) -> bool {
    let path = path.split('?').next().unwrap_or(path);
    ANSWERING_ROUTES.iter().any(|(m, template)| *m == method && route_matches(template, path))
}

/// Routes that are not MCP tools but answer or dismiss a card, or put text into a pane, for a caller on this
/// computer who needs no token: `POST /questions/{id}/answer`, `.../dismiss`, and `POST /panes/{id}/messages`.
/// They are reached by a plain loopback request (so a companion's shell could `curl` them), and `peer.rs` checks
/// who is on the other end of the connection. The desktop window does not come through here (Tauri commands).
pub const LOOPBACK_CARD_ROUTES: &[(&str, &str)] = &[
    ("POST", "/questions/{id}/answer"),
    ("POST", "/questions/{id}/dismiss"),
    ("POST", "/panes/{pane_id}/messages"),
];

/// Whether this request answers or dismisses a card, or types into a pane, by an MCP tool route or a loopback one.
/// The loopback handlers also take extra path segments (`/panes/{id}/key/...`), so they are matched by their first
/// segments, not by the exact route.
pub fn is_card_route(method: &str, path: &str) -> bool {
    if is_answering_route(method, path) {
        return true;
    }
    if method != "POST" {
        return false;
    }
    let route = path.split('?').next().unwrap_or(path);
    let parts: Vec<&str> = route.trim_matches('/').split('/').collect();
    match parts.as_slice() {
        ["questions", _, "answer" | "dismiss", ..] => true,
        ["panes", _, "input" | "key" | "messages", ..] => true,
        ["panes", _, "opencode", ..] => true,
        ["agents", "answer", ..] => true,
        _ => false,
    }
}

/// The refusal a companion gets.
pub fn refusal() -> OperationError {
    OperationError::new(
        "COMPANION_CANNOT_ANSWER",
        "Only your person can answer a helper's question, trust or permission card. You can see that a helper is waiting: tell your person and let them answer it. You have no way to answer it for them.",
        false,
    )
}

/// Refuses a companion's request to answer a card or type into a pane. Everything else passes.
pub fn check(method: &str, path: &str, caller_is_companion: bool) -> Result<(), OperationError> {
    if caller_is_companion && is_answering_route(method, path) {
        return Err(refusal());
    }
    Ok(())
}

/// The check for the router: asks who the caller is only when the route could answer a card.
pub fn check_session(
    registry: &std::sync::Arc<parking_lot::Mutex<crate::pty::PaneRegistry>>,
    session: &str,
    method: &str,
    path: &str,
) -> Result<(), OperationError> {
    if !is_answering_route(method, path) {
        return Ok(());
    }
    check(method, path, super::caller::of_session(registry, session).suspected.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_registry::{tools, ToolSafety};

    #[test]
    fn a_companion_cannot_answer_a_helpers_card_by_any_route() {
        for (method, path) in [
            ("POST", "/agents/answer"),
            ("POST", "/panes/abc123/opencode/permissions/req-1/reply"),
            ("POST", "/panes/abc123/opencode/question/reply"),
            ("POST", "/panes/abc123/input"),
            ("POST", "/panes/abc123/key"),
        ] {
            let error = check(method, path, true).expect_err(path);
            assert_eq!(error.code, "COMPANION_CANNOT_ANSWER", "{path}");
            assert!(!error.recoverable, "{path}: not a retry-later error");
        }
    }

    #[test]
    fn a_companion_can_still_do_its_other_work() {
        for (method, path) in [
            ("POST", "/agents/run"),
            ("POST", "/agents/wait"),
            ("GET", "/agents"),
            ("POST", "/agents/followup"),
            ("POST", "/questions/ask"),
            ("GET", "/panes"),
            ("POST", "/companion/recall"),
        ] {
            assert!(check(method, path, true).is_ok(), "{method} {path}");
        }
    }

    #[test]
    fn everyone_else_is_not_touched_by_the_guard() {
        for path in ["/agents/answer", "/panes/p/key"] {
            assert!(check("POST", path, false).is_ok(), "{path}");
        }
        // The same path with another method is another route.
        assert!(check("GET", "/agents/answer", true).is_ok());
    }

    /// A new MCP tool that answers a card or types into a pane must be added to `ANSWERING_ROUTES`, or this fails.
    #[test]
    fn every_tool_that_could_answer_a_card_is_guarded() {
        let known = ["answer_prompt", "reply_opencode_permission", "reply_opencode_question", "write_terminal_input", "press_key"];
        for tool in tools() {
            let looks_like_answering = tool.path.contains("answer")
                || tool.path.ends_with("/reply")
                || tool.path.ends_with("/key")
                || tool.path.ends_with("/input")
                || tool.name.contains("answer")
                || tool.name.starts_with("reply_");
            if looks_like_answering && tool.safety != ToolSafety::ReadOnly {
                assert!(is_answering_route(tool.method, tool.path), "{} ({} {}) can answer a card but is not guarded", tool.name, tool.method, tool.path);
            }
        }
        for name in known {
            let tool = tools().into_iter().find(|t| t.name == name).unwrap_or_else(|| panic!("{name} left the catalog"));
            assert!(is_answering_route(tool.method, tool.path), "{name}");
        }
        assert_eq!(ANSWERING_ROUTES.len(), known.len());
    }

    #[test]
    fn the_loopback_card_routes_are_known_next_to_the_mcp_ones() {
        for (method, path) in [
            ("POST", "/questions/q1/answer"),
            ("POST", "/questions/q1/dismiss"),
            ("POST", "/panes/p1/messages"),
            ("POST", "/agents/answer"),
            ("POST", "/panes/p1/key"),
            // Extra path segments reach the same handlers, so they count too.
            ("POST", "/panes/p1/key/enter"),
            ("POST", "/panes/p1/input/x"),
            ("POST", "/panes/p1/opencode/question/reply"),
            ("POST", "/questions/q1/answer/x"),
        ] {
            assert!(is_card_route(method, path), "{method} {path}");
        }
        for (method, path) in [("GET", "/questions"), ("POST", "/questions/ask"), ("GET", "/panes/p1/messages"), ("POST", "/agents/run"), ("DELETE", "/panes/p1/messages/m1")] {
            assert!(!is_card_route(method, path), "{method} {path}");
        }
    }
}
