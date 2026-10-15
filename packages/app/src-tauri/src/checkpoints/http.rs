//! HTTP shape of the checkpoint routes, independent of the bridge types so it can be tested
//! against a fake environment:
//! `GET /panes/{id}/checkpoints`, `GET /panes/{id}/checkpoints/{cid}/diff`,
//! `POST /panes/{id}/checkpoints/{cid}/restore`.

use serde_json::{json, Value};

use super::engine::CheckpointError;
use super::runtime::Runtime;
use super::{list_view_with, ListView};

fn fail(error: CheckpointError) -> (u16, Value) {
    let (status, code) = match &error {
        CheckpointError::NotFound => (404, "CHECKPOINT_NOT_FOUND"),
        CheckpointError::Running => (409, "PANE_RUNNING"),
        CheckpointError::RunOpen => (409, "RUN_OPEN"),
        CheckpointError::Disabled => (409, "CHECKPOINTS_DISABLED"),
        CheckpointError::GitMissing => (501, "GIT_MISSING"),
        CheckpointError::TooLarge(_) => (413, "CHECKPOINT_TOO_LARGE"),
        CheckpointError::Other(_) => (500, "CHECKPOINT_FAILED"),
    };
    (status, json!({"code": code, "error": error.to_string()}))
}

/// `None` when the request is not a checkpoint route; otherwise `Ok((status, body))` or an error response.
pub fn respond(rt: &Runtime, method: &str, segments: &[&str]) -> Option<Result<(u16, Value), (u16, Value)>> {
    match (method, segments) {
        ("GET", ["panes", pane, "checkpoints"]) => Some(
            list_view_with(rt, pane)
                .map(|view: ListView| (200, serde_json::to_value(view).unwrap_or_default()))
                .map_err(|e| (500, json!({"code": "CHECKPOINT_FAILED", "error": e}))),
        ),
        ("GET", ["panes", pane, "checkpoints", id, "diff"]) => {
            Some(rt.diff(pane, id).map(|changes| (200, json!({"changes": changes}))).map_err(fail))
        }
        ("POST", ["panes", pane, "checkpoints", id, "restore"]) => {
            Some(rt.restore(pane, id).map(|r| (200, serde_json::to_value(r).unwrap_or_default())).map_err(fail))
        }
        _ => None,
    }
}
