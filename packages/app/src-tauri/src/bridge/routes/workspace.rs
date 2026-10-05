//! Read-model and settings routes: workspace state, audit, event replay, inboxes, context
//! packs, project IR, the librarian prompt, settings and the active project path.

use serde::Deserialize;
use serde_json::json;

use super::{Ctx, RouteResult};
use crate::bridge::http::{parse_json, query_param};
use crate::bridge::projects::{librarian_indexer_path, resolve_project_root};
use crate::bridge::store::rebuild_read_models;
use crate::pty::registry_set_project_path;
use crate::settings_store;

#[derive(Debug, Deserialize)]
struct ProjectPathBody {
    path: String,
}

pub(super) fn handle(cx: &Ctx) -> Option<RouteResult> {
    Some(match (cx.method, cx.segments) {
        ("GET", ["events", "replay", "panes"]) => replay_panes(),
        ("GET", ["workspace", "state"]) => workspace_state(),
        ("GET", ["agents", id, "inbox"]) => inbox(id),
        ("GET", ["audit"]) => audit(),
        ("POST", ["context-packs"]) => context_packs(cx),
        ("GET", ["project-ir", "status"]) => project_ir_status(cx),
        ("GET", ["librarian", "prompt"]) => librarian_prompt(cx),
        ("GET", ["events", "recent"]) => events_recent(cx),
        ("GET", ["agent-contexts"]) => agent_contexts(),
        ("GET", ["settings"]) => settings_get(cx),
        ("PATCH" | "POST", ["settings"]) => settings_patch(cx),
        ("POST", ["project-path"]) => project_path(cx),
        _ => return None,
    })
}

fn replay_panes() -> RouteResult {
    let timeline = crate::event_log::replay_global_pane_timeline()
        .map_err(|err| (500, json!({ "error": err })))?;
    Ok((200, serde_json::to_value(timeline).unwrap()))
}

fn workspace_state() -> RouteResult {
    let read_models = rebuild_read_models().map_err(|err| (500, json!({ "error": err })))?;
    Ok((200, serde_json::to_value(read_models.workspace).unwrap()))
}

fn inbox(id: &str) -> RouteResult {
    let inbox = crate::projections::agent_inbox(id.to_string());
    Ok((200, serde_json::to_value(inbox).unwrap()))
}

fn audit() -> RouteResult {
    let read_models = rebuild_read_models().map_err(|err| (500, json!({ "error": err })))?;
    Ok((200, serde_json::to_value(read_models.audit).unwrap()))
}

fn context_packs(cx: &Ctx) -> RouteResult {
    let body = cx.body;
    let registry = cx.registry;
    let app = cx.app;
    let req: crate::context_pack::ContextPackRequest = parse_json(body)?;
    let read_models = rebuild_read_models().map_err(|err| (500, json!({ "error": err })))?;
    let project_root = resolve_project_root(registry);
    let indexer_path = librarian_indexer_path(app);
    let pack = crate::context_pack::build_context_pack(
        req,
        &read_models,
        project_root.as_deref(),
        indexer_path.as_deref(),
    );
    Ok((200, serde_json::to_value(pack).unwrap()))
}

fn project_ir_status(cx: &Ctx) -> RouteResult {
    let registry = cx.registry;
    let app = cx.app;
    let project_root = resolve_project_root(registry).ok_or_else(|| {
        (
            400,
            json!({ "error": "no active project path — set project folder first" }),
        )
    })?;
    let indexer_path = librarian_indexer_path(app);
    let status = crate::project_ir::status(&project_root, indexer_path.as_deref());
    Ok((200, serde_json::to_value(status).unwrap()))
}

fn librarian_prompt(cx: &Ctx) -> RouteResult {
    let registry = cx.registry;
    let project_root = resolve_project_root(registry).ok_or_else(|| {
        (
            400,
            json!({ "error": "no active project path — set project folder first" }),
        )
    })?;
    let prompt = crate::project_ir::render_librarian_prompt(&project_root)
        .map_err(|err| (500, json!({ "error": err })))?;
    Ok((
        200,
        json!({
            "prompt": prompt,
            "delegate_to": "opencode_native",
            "completion_marker": "LIBRARIAN_INDEX_COMPLETE",
        }),
    ))
}

fn events_recent(cx: &Ctx) -> RouteResult {
    let query = cx.query;
    let limit = query_param(query, "limit")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(50);
    let pane_id = query_param(query, "pane_id");
    let since_id = query_param(query, "since_id");
    let types = query_param(query, "types").map(|value| {
        value
            .split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>()
    });
    let events = crate::event_log::read_recent_events(
        limit,
        pane_id.as_deref(),
        types.as_deref(),
        since_id.as_deref(),
    )
    .map_err(|err| (500, json!({ "error": err })))?;
    Ok((200, serde_json::to_value(events).unwrap()))
}

fn agent_contexts() -> RouteResult {
    Ok((
        200,
        serde_json::to_value(crate::agent_contexts::list_agent_context_profiles()).unwrap(),
    ))
}

fn settings_get(cx: &Ctx) -> RouteResult {
    let app = cx.app;
    Ok((200, settings_store::read_public_settings(app)))
}

fn settings_patch(cx: &Ctx) -> RouteResult {
    let body = cx.body;
    let app = cx.app;
    let patch: serde_json::Value = parse_json(body)?;
    let updated = settings_store::patch_public_settings(app, patch);
    Ok((200, updated))
}

fn project_path(cx: &Ctx) -> RouteResult {
    let body = cx.body;
    let registry = cx.registry;
    let req: ProjectPathBody = parse_json(body)?;
    let normalized = crate::project_path::normalize_project_path(std::path::Path::new(&req.path))
        .map_err(|err| (400, json!({ "error": err })))?;
    registry_set_project_path(registry, normalized.to_string_lossy().into_owned());
    crate::event_log::set_active_project_path(Some(normalized));
    Ok((200, json!({ "ok": true })))
}
