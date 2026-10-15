//! `/worktrees/*`: review, merge and discard the git worktrees of isolated runs
//! (`crate::worktrees::routes`). App session only; not exposed to MCP sessions.

use super::{Ctx, RouteResult};
use crate::bridge::errors::operation_http_error;
use crate::bridge::http::query_param;

pub(super) fn handle(cx: &Ctx) -> Option<RouteResult> {
    let project = query_param(cx.query, "project_path").or_else(|| {
        cx.request_body
            .get("project_path")
            .and_then(|value| value.as_str())
            .map(str::to_string)
    });
    let path = query_param(cx.query, "path");
    let result = crate::worktrees::routes::handle(
        cx.method,
        cx.segments,
        project.as_deref(),
        path.as_deref(),
        &cx.request_body,
        cx.registry,
        cx.session,
    )?;
    // The direct merge route is written down too (the MCP tool writes its own line).
    if let (("POST", ["worktrees", handle, "merge"]), Ok(outcome)) = ((cx.method, cx.segments), &result) {
        if outcome.get("merged").and_then(|v| v.as_bool()) == Some(true) {
            let by_phone = cx.session.is_none() && crate::mobile_pairing::requires_pairing_token(cx.headers, cx.peer_loopback);
            let text = |key: &str| outcome.get(key).and_then(|v| v.as_str()).unwrap_or_default().to_string();
            crate::agent_authority::merge::note_direct_merge(by_phone, handle, &text("branch"), &text("into"));
        }
    }
    Some(result.map(|value| (200, value)).map_err(operation_http_error))
}
