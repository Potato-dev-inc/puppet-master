//! Chat sessions over ACP on the bridge (A2): for the paired phone and the app. Not in the MCP tool catalog, so an
//! agent session cannot reach them (`mcp_sessions::authorize`); the MCP view of a chat session comes in A4.
//!
//! - `POST /acp/sessions` `{agent, folder, lane?}` starts one: `lane: "acp"` asks for it whatever the agent's saved
//!   choice is (the Labs switch always applies).
//! - `GET /acp/sessions/{pane}` the session's state (phase, "stopping", what waits on the person).
//! - `POST /acp/sessions/{pane}/cancel` stops the turn. A phone needs `type` for it (`route_need`).
//!
//! Answering a permission question is the question route (`POST /questions/{id}/answer`, the phone's card).
//! The work is `crate::acp::chat::api`; this only matches the paths.

use super::{Ctx, RouteResult};
use crate::acp::chat::api;
use crate::commands::acp_chat::live_deps;

pub(super) fn handle(cx: &Ctx) -> Option<RouteResult> {
    match (cx.method, cx.segments) {
        ("POST", ["acp", "sessions"]) => Some(api::start(&cx.request_body, &live_deps(cx.app, cx.registry))),
        ("GET", ["acp", "sessions", pane_id]) => Some(api::state(pane_id)),
        ("POST", ["acp", "sessions", pane_id, "cancel"]) => Some(api::cancel(pane_id)),
        _ => None,
    }
}
