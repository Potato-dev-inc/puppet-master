//! `/panes/{id}/flight`: the pane's flight recording (see `crate::flight`).
//!
//! - `GET`: the recording as a redacted asciicast v2 file, inside JSON (`cast`), for an export.
//! - `POST`: freeze it to `<project>/.puppet-master/operations/<operation_id>/flight.cast`
//!   (`operation_id` in the body, or a generated `manual-...` name).
//!
//! Neither is part of the MCP tool catalog, so an agent session cannot call them; the app and paired
//! devices can, like every other bridge route.

use super::{Ctx, RouteResult};
use crate::bridge::http::parse_json;
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Default, Deserialize)]
struct FreezeBody {
    #[serde(default)]
    operation_id: Option<String>,
}

pub(super) fn handle(cx: &Ctx) -> Option<RouteResult> {
    match (cx.method, cx.segments) {
        ("GET", ["panes", pane_id, "flight"]) => Some(export(pane_id)),
        ("POST", ["panes", pane_id, "flight"]) => Some(freeze(cx, pane_id)),
        _ => None,
    }
}

fn export(pane_id: &str) -> RouteResult {
    let (cast, events) = crate::flight::freeze::export(pane_id, "exported on request")
        .map_err(|error| (404, json!({ "error": error })))?;
    Ok((200, json!({ "pane_id": pane_id, "format": "asciicast-v2", "events": events, "bytes": cast.len(), "cast": cast })))
}

fn freeze(cx: &Ctx, pane_id: &str) -> RouteResult {
    let body: FreezeBody = if cx.body.is_empty() { FreezeBody::default() } else { parse_json(cx.body)? };
    let project = crate::pty::registry_get_project_path(cx.registry);
    let operation_id = body.operation_id.filter(|id| !id.trim().is_empty()).unwrap_or_else(|| {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        format!("manual-{secs}")
    });
    let (cast, events) = crate::flight::freeze::export(pane_id, "frozen on request")
        .map_err(|error| (404, json!({ "error": error })))?;
    let frozen = crate::flight::freeze::write(&project, &operation_id, &cast, events)
        .map_err(|error| (400, json!({ "error": error })))?;
    Ok((200, json!({ "pane_id": pane_id, "operation_id": operation_id, "saved": frozen })))
}
