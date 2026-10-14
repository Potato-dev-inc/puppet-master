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
    crate::worktrees::routes::handle(
        cx.method,
        cx.segments,
        project.as_deref(),
        path.as_deref(),
        &cx.request_body,
        cx.registry,
        cx.session,
    )
    .map(|result| result.map(|value| (200, value)).map_err(operation_http_error))
}
