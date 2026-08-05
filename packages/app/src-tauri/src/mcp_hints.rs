//! Machine-usable next-step hints for MCP mutating tool responses.

use crate::opencode::OpenCodeModelRef;
use serde_json::{json, Value};

pub fn suggested_wait_panes(
    pane_ids: &[String],
    until: &[&str],
    model: Option<&OpenCodeModelRef>,
    task_id: Option<&str>,
    output_regex: Option<&str>,
) -> Value {
    let mut args = json!({
        "pane_ids": pane_ids,
        "until": until,
    });
    if let Some(model) = model {
        args["match"] = json!({
            "provider_id": model.provider_id,
            "model_id": model.model_id,
        });
    }
    if let Some(task_id) = task_id {
        args["task_id"] = json!(task_id);
    }
    if let Some(output_regex) = output_regex {
        args["output_regex"] = json!(output_regex);
    }
    json!({
        "tool": "wait_for_panes",
        "args": args,
    })
}

pub fn suggested_wait_worker(pane_id: &str) -> Value {
    json!({
        "tool": "wait_for_worker",
        "args": {
            "pane_id": pane_id,
            "timeout_ms": 120_000,
        },
    })
}

pub fn suggested_wait_after_model_switch(pane_id: &str, model: &OpenCodeModelRef) -> Value {
    suggested_wait_panes(
        &[pane_id.to_string()],
        &["model_ready", "tui_ready", "permission", "error"],
        Some(model),
        None,
        None,
    )
}

pub fn suggested_wait_after_write(pane_id: &str) -> Value {
    suggested_wait_worker(pane_id)
}

pub fn suggested_wait_after_spawn(pane_id: &str) -> Value {
    suggested_wait_panes(
        &[pane_id.to_string()],
        &["tui_ready", "idle", "waiting_input", "error"],
        None,
        None,
        None,
    )
}

pub fn suggested_wait_after_delegate(pane_id: &str) -> Value {
    suggested_wait_worker(pane_id)
}

pub fn mutate_ok(snapshot: Value, suggested_wait: Value) -> Value {
    json!({
        "ok": true,
        "snapshot": snapshot,
        "suggested_wait": suggested_wait,
    })
}
