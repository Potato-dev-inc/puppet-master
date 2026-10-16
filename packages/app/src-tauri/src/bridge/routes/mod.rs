//! Route groups of the HTTP bridge. Each group exposes a `handle` function that returns
//! `None` for requests it does not own, and `dispatch` tries the groups in a fixed order
//! (the order matters where a literal path such as `/panes/wait` overlaps a parameterised one).

mod accounts;
mod attention;
pub(super) mod definitions;
pub(super) mod health;
mod checkpoints;
mod digest;
mod flight;
mod locks;
mod missions;
pub(super) mod mcp;
mod opencode;
mod operations;
mod pairing;
pub(super) mod panes;
pub(super) mod previews;
mod push;
mod session;
mod shell;
mod standby;
mod tamper;
mod tasks;
mod team;
pub(super) mod toolbelt;
mod wait;
mod workspace;
mod wizard;
mod worktrees;

use parking_lot::Mutex;
use std::sync::Arc;
use tauri::AppHandle;

use super::errors::{enrich_handle_error, operation_http_error, RouteResult};
use crate::pty::PaneRegistry;

/// One authorised, body-parsed request as seen by the route groups.
pub(super) struct Ctx<'a> {
    pub method: &'a str,
    pub segments: &'a [&'a str],
    pub query: &'a str,
    pub body: &'a [u8],
    /// `body` parsed as JSON (`{}` when empty).
    pub request_body: serde_json::Value,
    pub headers: &'a str,
    /// TCP peer of the request is a loopback address.
    pub peer_loopback: bool,
    pub session: Option<&'a str>,
    pub registry: &'a Arc<Mutex<PaneRegistry>>,
    pub app: &'a AppHandle,
}

/// Runs the first group that owns the request; `None` means no route matched.
pub(super) fn dispatch(cx: &Ctx) -> Option<RouteResult> {
    mcp::handle_session(cx)
        .or_else(|| definitions::handle(cx))
        .or_else(|| wizard::handle(cx))
        .or_else(|| health::handle(cx))
        .or_else(|| previews::handle(cx))
        .or_else(|| toolbelt::handle(cx))
        .or_else(|| agent_runs(cx))
        .or_else(|| shell::handle(cx))
        .or_else(|| mcp::handle_agents(cx))
        .or_else(|| pairing::handle(cx))
        .or_else(|| push::handle(cx))
        .or_else(|| workspace::handle(cx))
        .or_else(|| operations::handle(cx))
        .or_else(|| tasks::handle(cx))
        .or_else(|| locks::handle(cx))
        .or_else(|| session::handle(cx))
        .or_else(|| wait::handle(cx))
        .or_else(|| standby::handle(cx))
        .or_else(|| accounts::handle(cx))
        .or_else(|| opencode::handle(cx))
        .or_else(|| worktrees::handle(cx))
        .or_else(|| checkpoints::handle(cx))
        .or_else(|| tamper::handle(cx))
        .or_else(|| missions::handle(cx))
        .or_else(|| digest::handle(cx))
        .or_else(|| flight::handle(cx))
        .or_else(|| team::handle(cx))
        .or_else(|| attention::handle(cx))
        .or_else(|| panes::handle(cx))
}

/// The agent-run API (`run_agent`, `wait_agents`, ...) owns its own routes.
fn agent_runs(cx: &Ctx) -> Option<RouteResult> {
    crate::agent_runs::handle_request_as(
        cx.method,
        cx.segments,
        cx.query,
        cx.body,
        cx.registry,
        cx.app,
        cx.session,
        crate::mobile_pairing::requires_pairing_token(cx.headers, cx.peer_loopback),
    )
    .map(|response| {
        response
            .map_err(|error| enrich_handle_error(error, cx.registry))
            .map(|value| (200, value))
            .map_err(operation_http_error)
    })
}
