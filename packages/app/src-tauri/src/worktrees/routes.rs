//! Transport-independent bodies of the `/worktrees/*` bridge routes (the bridge only extracts
//! the arguments and maps errors to HTTP):
//!
//! - `GET  /worktrees[?project_path=]`                       open worktrees
//! - `GET  /worktrees/{handle}/diff[?project_path=&path=]`   stat + capped patch
//! - `POST /worktrees/{handle}/merge`   `{project_path?}`    `--no-ff` merge, then closes it
//! - `POST /worktrees/{handle}/discard` `{project_path?}`    removes worktree and branch
//!
//! `POST /worktrees/review {handle, action, path?}` is the one MCP-visible route
//! (`review_worktree`); the session must own the handle. The rest is app session only.

use super::service;
use crate::operations::OperationError;
use crate::pty::PaneRegistry;
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::sync::Arc;

pub fn handle(
    method: &str,
    segments: &[&str],
    project: Option<&str>,
    path: Option<&str>,
    body: &Value,
    registry: &Arc<Mutex<PaneRegistry>>,
) -> Option<Result<Value, OperationError>> {
    Some(match (method, segments) {
        ("POST", ["worktrees", "review"]) => {
            let Some(handle) = body.get("handle").and_then(Value::as_str) else {
                return Some(Err(OperationError::new("INVALID_ARGUMENT", "handle is required", false)));
            };
            let body_path = body.get("path").and_then(Value::as_str).or(path);
            match body.get("action").and_then(Value::as_str).unwrap_or("diff") {
                "diff" => service::diff(project, handle, body_path, registry).map(|view| json!(view)),
                "merge" => service::merge(project, handle, registry).map(|outcome| json!(outcome)),
                "discard" => service::discard(project, handle, registry).map(|()| json!({ "discarded": true })),
                other => Err(OperationError::new(
                    "INVALID_ARGUMENT",
                    format!("action must be diff, merge or discard, got {other:?}"),
                    false,
                )),
            }
        }
        ("GET", ["worktrees"]) => Ok(json!({ "worktrees": service::list(project, registry) })),
        ("GET", ["worktrees", handle, "diff"]) => {
            service::diff(project, handle, path, registry).map(|view| json!(view))
        }
        ("POST", ["worktrees", handle, "merge"]) => {
            service::merge(project, handle, registry).map(|outcome| json!(outcome))
        }
        ("POST", ["worktrees", handle, "discard"]) => {
            service::discard(project, handle, registry).map(|()| json!({ "discarded": true }))
        }
        _ => return None,
    })
}
