//! The optional `shell` argument that shapes printed `watch_command`s.

use super::*;

/// Removes and parses the optional `shell` argument of the tools that print a `watch_command`.
pub(crate) fn take_shell_hint(
    tool: &str,
    args: &mut Value,
) -> Result<Option<puppet_master_app_lib::watch_command::ShellHint>, String> {
    if !matches!(
        tool,
        "run_agent" | "wait_agents" | "followup_task" | "send_agent"
    ) {
        return Ok(None);
    }
    let Some(raw) = args
        .as_object_mut()
        .and_then(|object| object.remove("shell"))
    else {
        return Ok(None);
    };
    let name = raw.as_str().unwrap_or_default();
    puppet_master_app_lib::watch_command::ShellHint::parse(name)
        .map(Some)
        .ok_or_else(|| format!("invalid shell {raw}: expected bash, powershell, cmd, or default"))
}

/// Rewrites every `watch_command` (next to an `operation_id`) in a JSON tool result for `hint`.
pub(crate) fn apply_shell_hint(
    text: &str,
    hint: puppet_master_app_lib::watch_command::ShellHint,
    project_fallback: Option<&str>,
) -> String {
    fn walk(
        value: &mut Value,
        hint: puppet_master_app_lib::watch_command::ShellHint,
        project_fallback: Option<&str>,
    ) {
        match value {
            Value::Object(object) => {
                let operation_id = object
                    .get("operation_id")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                if let (Some(operation_id), Some(Value::String(command))) =
                    (operation_id, object.get_mut("watch_command"))
                {
                    *command = puppet_master_app_lib::watch_command::reformat_for_shell(
                        command,
                        &operation_id,
                        project_fallback,
                        hint,
                    );
                }
                for child in object.values_mut() {
                    walk(child, hint, project_fallback);
                }
            }
            Value::Array(items) => {
                for child in items {
                    walk(child, hint, project_fallback);
                }
            }
            _ => {}
        }
    }
    match serde_json::from_str::<Value>(text) {
        Ok(mut value) => {
            walk(&mut value, hint, project_fallback);
            value.to_string()
        }
        Err(_) => text.to_string(),
    }
}
