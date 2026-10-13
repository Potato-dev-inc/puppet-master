//! `POST /shell/exec`: run a command in a shell pane (an existing one or a fresh pane).

use super::{Ctx, RouteResult};
use crate::bridge::errors::operation_http_error;
use crate::bridge::pane_util::register_spawned_pane;
use crate::bridge::projects::normalize_shell_cwd;
use crate::pty::{registry_kill_pane, registry_spawn_pane, SpawnPaneArgs};

pub(super) fn handle(cx: &Ctx) -> Option<RouteResult> {
    match (cx.method, cx.segments) {
        ("POST", ["shell", "exec"]) => Some(exec(cx)),
        _ => None,
    }
}

fn exec(cx: &Ctx) -> RouteResult {
    let registry = cx.registry;
    let app = cx.app;
    let session = cx.session;
    let request_body = &cx.request_body;
    let command = request_body
        .get("command")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let timeout = request_body
        .get("timeout_ms")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(30_000);
    let requested_pane = request_body
        .get("pane_id")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty());
    let requested_cwd = request_body
        .get("cwd")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty());

    let (pane_id, cwd, created_pane) = if let Some(pane_id) = requested_pane {
        let cwd = requested_cwd
            .map(|path| normalize_shell_cwd(Some(path), registry))
            .transpose()
            .map_err(operation_http_error)?;
        (pane_id.to_string(), cwd, false)
    } else {
        let cwd = normalize_shell_cwd(requested_cwd, registry).map_err(operation_http_error)?;
        let agent_type = if cfg!(windows) { "powershell" } else { "bash" };
        let pane_id = registry_spawn_pane(
            registry,
            app,
            SpawnPaneArgs {
                agent_type: agent_type.to_string(),
                cwd: Some(cwd.clone()),
                cols: None,
                rows: None,
                extra_args: None,
                pane_id: None,
                read_only: false,
                account: None,
            },
        )
        .map_err(|error| {
            operation_http_error(crate::operations::OperationError::new(
                "PANE_SPAWN_FAILED",
                error,
                true,
            ))
        })?;
        if let Err(error) = register_spawned_pane(session, agent_type, &pane_id) {
            let _ = registry_kill_pane(registry, &pane_id);
            return Err(operation_http_error(error));
        }
        (pane_id, Some(cwd), true)
    };

    let mut result =
        crate::shell_exec::execute_in(registry, &pane_id, command, timeout, cwd.as_deref())
            .map_err(operation_http_error)?;
    if let Some(result) = result.as_object_mut() {
        result.insert("created_pane".into(), serde_json::Value::Bool(created_pane));
    }
    Ok((200, result))
}
