//! Pane, terminal and agent-model tools: maps each tool call to its bridge request.

use super::super::*;
use std::sync::mpsc::Receiver;

/// Runs `name` when this family owns it; `None` lets the next family try.
pub(super) fn dispatch(
    name: &str,
    args: &Value,
    _cancel_rx: Option<&Receiver<()>>,
) -> Option<Result<String, String>> {
    let mut known = true;
    let result = run(name, args, &mut known);
    known.then_some(result)
}

fn run(name: &str, args: &Value, known: &mut bool) -> Result<String, String> {
    match name {
        "list_panes" => bridge_request("GET", "/panes", None),
        "bridge_health" => bridge_request("GET", "/health", None).map(|body| {
            let Ok(mut health) = serde_json::from_str::<Value>(&body) else {
                return body;
            };
            let drift = schema_drift_from_health(&health);
            if let Some(object) = health.as_object_mut() {
                object.insert(
                    "shim_catalog_version".into(),
                    json!(tool_registry::catalog_version()),
                );
                // Additive: which dev tools exist on PATH here (cached, short timeouts).
                object.insert(
                    "env".into(),
                    puppet_master_app_lib::env_probe::env_snapshot(),
                );
                object.insert("schema_drift".into(), json!(drift.is_some()));
                if let Some(drift) = drift {
                    object.insert("schema_drift_hint".into(), drift["hint"].clone());
                }
            }
            health.to_string()
        }),
        "list_agent_contexts" => bridge_request("GET", "/agent-contexts", None),
        "read_agent_context" => read_agent_context(args),
        "inspect_agent_model" => {
            let pane_id = required_string(args, "pane_id")?;
            let lines = optional_number(args, "lines").unwrap_or(200);
            Ok(bridge_request(
                "GET",
                &format!(
                    "/panes/{}/model?lines={lines}",
                    encode_path_segment(&pane_id)
                ),
                None,
            )?)
        }
        "switch_agent_model" => {
            let model_id = required_string(args, "model_id")?;
            let mut body = json!({ "model_id": model_id });
            if let Some(provider) = args.get("model_provider").and_then(Value::as_str) {
                body["model_provider"] = json!(provider);
            }
            let pane_id = if let Some(pane) = args.get("pane_id").and_then(Value::as_str) {
                assert_worker_pane(pane)?
            } else if let Some(handle) = args.get("handle").and_then(Value::as_str) {
                let project = args
                    .get("project_path")
                    .and_then(Value::as_str)
                    .map(|p| format!("?project_path={}", encode_path_segment(p)))
                    .unwrap_or_default();
                let inspect = bridge_request(
                    "GET",
                    &format!("/agents/{}{}", encode_path_segment(handle), project),
                    None,
                )?;
                let value: Value = serde_json::from_str(&inspect)
                    .map_err(|e| format!("invalid inspect_agent response: {e}"))?;
                let pane = value
                        .get("pane_id")
                        .and_then(Value::as_str)
                        .ok_or_else(|| {
                            format!(
                                "handle {handle} has no live pane; set keep_pane:true on run_agent to retain the pane after completion"
                            )
                        })?;
                body["handle"] = json!(handle);
                assert_worker_pane(pane)?
            } else {
                return Err("switch_agent_model requires handle or pane_id".into());
            };
            Ok(bridge_request(
                "POST",
                &format!("/panes/{}/model", encode_path_segment(&pane_id)),
                Some(body),
            )?)
        }
        "spawn_agent" => bridge_request("POST", "/panes", Some(args.clone())),
        "read_terminal_buffer" => {
            let pane_id = required_string(args, "pane_id")?;
            let lines = optional_number(args, "lines").unwrap_or(200);
            let view = args
                .get("view")
                .and_then(Value::as_str)
                .map(|view| format!("&view={}", encode_path_segment(view)))
                .unwrap_or_default();
            let response = bridge_request(
                "GET",
                &format!(
                    "/panes/{}/buffer?lines={lines}{view}",
                    encode_path_segment(&pane_id)
                ),
                None,
            )?;
            Ok(serde_json::from_str::<Value>(&response)
                .ok()
                .and_then(|value| {
                    value
                        .get("content")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
                .unwrap_or(response))
        }
        "write_terminal_input" => {
            let pane_id = assert_worker_pane(&required_string(args, "pane_id")?)?;
            let mut body = json!({
                "text": required_string(args, "text")?,
                "append_newline": args.get("append_newline").and_then(Value::as_bool).unwrap_or(true),
                "via_opencode_api": true,
            });
            if let Some(provider) = args.get("model_provider").and_then(Value::as_str) {
                body["model_provider"] = json!(provider);
            }
            if let Some(model_id) = args.get("model_id").and_then(Value::as_str) {
                body["model_id"] = json!(model_id);
            }
            Ok(bridge_request(
                "POST",
                &format!("/panes/{}/input", encode_path_segment(&pane_id)),
                Some(body),
            )?)
        }
        "press_key" => {
            let pane_id = assert_worker_pane(&required_string(args, "pane_id")?)?;
            let key = required_string(args, "key")?;
            let response = bridge_request(
                "POST",
                &format!("/panes/{}/key", encode_path_segment(&pane_id)),
                Some(json!({ "key": key })),
            )?;
            Ok(serde_json::from_str::<Value>(&response)
                .ok()
                .map(|value| {
                    let pressed = value.get("key").and_then(Value::as_str).unwrap_or(&key);
                    let bytes = value.get("bytes").and_then(Value::as_u64).unwrap_or(0);
                    format!(
                        "pressed {pressed} ({bytes} byte{})",
                        if bytes == 1 { "" } else { "s" }
                    )
                })
                .unwrap_or_else(|| format!("pressed {key}")))
        }
        "kill_pane_process" => {
            let pane_id = assert_worker_pane(&required_string(args, "pane_id")?)?;
            bridge_request(
                "DELETE",
                &format!("/panes/{}", encode_path_segment(&pane_id)),
                None,
            )?;
            Ok("killed".to_string())
        }
        _ => {
            *known = false;
            Ok(String::new())
        }
    }
}

fn read_agent_context(args: &Value) -> Result<String, String> {
    if let Some(pane_id) = args.get("pane_id").and_then(Value::as_str) {
        return bridge_request(
            "GET",
            &format!("/panes/{}/agent-context", encode_path_segment(pane_id)),
            None,
        );
    }
    let agent_type = required_string(args, "agent_type")?;
    let contexts = bridge_request("GET", "/agent-contexts", None)?;
    let value: Value = serde_json::from_str(&contexts)
        .map_err(|err| format!("bridge returned invalid agent contexts JSON: {err}"))?;
    let context = value
        .as_array()
        .and_then(|contexts| {
            contexts.iter().find(|context| {
                context.get("agent_type").and_then(Value::as_str) == Some(&agent_type)
            })
        })
        .ok_or_else(|| format!("unknown agent_type: {agent_type}"))?;
    serde_json::to_string_pretty(context).map_err(|err| format!("serialize agent context: {err}"))
}
