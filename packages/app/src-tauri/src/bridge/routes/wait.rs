//! `POST /panes/wait*` routes: block until panes, a model, a task or a worker reach a state.

use serde::Deserialize;
use serde_json::json;

use super::{Ctx, RouteResult};
use crate::bridge::http::parse_json;

fn default_wait_timeout() -> u64 {
    120_000
}

pub(super) fn handle(cx: &Ctx) -> Option<RouteResult> {
    if cx.method != "POST" {
        return None;
    }
    Some(match cx.segments {
        ["panes", "wait"] => panes(cx),
        ["panes", "wait", "model"] => model(cx),
        ["panes", "wait", "task"] => task(cx),
        ["panes", "wait", "worker"] => worker(cx),
        _ => return None,
    })
}

fn panes(cx: &Ctx) -> RouteResult {
    let body = cx.body;
    let registry = cx.registry;
    let req: crate::pane_wait::WaitForPanesRequest = parse_json(body)?;
    let result = crate::pane_wait::wait_for_panes(registry, req)
        .map_err(|err| (400, json!({ "error": err })))?;
    Ok((200, serde_json::to_value(result).unwrap()))
}

#[derive(Deserialize)]
struct WaitModelBody {
    pane_id: String,
    #[serde(default)]
    provider_id: Option<String>,
    #[serde(default)]
    model_id: Option<String>,
    #[serde(default = "default_wait_timeout")]
    timeout_ms: u64,
}

fn model(cx: &Ctx) -> RouteResult {
    let body = cx.body;
    let registry = cx.registry;
    let req: WaitModelBody = parse_json(body)?;
    let result = crate::pane_wait::wait_for_model(
        registry,
        &req.pane_id,
        req.provider_id.as_deref(),
        req.model_id.as_deref(),
        req.timeout_ms,
    )
    .map_err(|err| (400, json!({ "error": err })))?;
    Ok((200, serde_json::to_value(result).unwrap()))
}

#[derive(Deserialize)]
struct WaitTaskBody {
    pane_id: String,
    task_id: String,
    #[serde(default)]
    until: Vec<String>,
    #[serde(default = "default_wait_timeout")]
    timeout_ms: u64,
}

fn task(cx: &Ctx) -> RouteResult {
    let body = cx.body;
    let registry = cx.registry;
    let req: WaitTaskBody = parse_json(body)?;
    let until = if req.until.is_empty() {
        vec!["task_completed", "task_blocked", "error"]
    } else {
        req.until.iter().map(String::as_str).collect::<Vec<_>>()
    };
    let result = crate::pane_wait::wait_for_task(
        registry,
        &req.pane_id,
        &req.task_id,
        &until,
        req.timeout_ms,
    )
    .map_err(|err| (400, json!({ "error": err })))?;
    Ok((200, serde_json::to_value(result).unwrap()))
}

#[derive(Deserialize)]
struct WaitWorkerBody {
    pane_id: String,
    #[serde(default = "default_wait_timeout")]
    timeout_ms: u64,
}

fn worker(cx: &Ctx) -> RouteResult {
    let body = cx.body;
    let registry = cx.registry;
    let req: WaitWorkerBody = parse_json(body)?;
    let result = crate::pane_wait::wait_for_worker(registry, &req.pane_id, req.timeout_ms)
        .map_err(|err| (400, json!({ "error": err })))?;
    Ok((200, serde_json::to_value(result).unwrap()))
}
