//! `tools/call` entry points: argument shaping, mode gating and result wrapping.

use super::*;

#[cfg(test)]
pub(crate) fn call_tool(params: Value) -> Result<Value, String> {
    call_tool_cancellable(params, None)
}

pub(crate) fn call_tool_cancellable(
    params: Value,
    cancel_rx: Option<std::sync::mpsc::Receiver<()>>,
) -> Result<Value, String> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| "tools/call missing params.name".to_string())?;
    let mut args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    // Optional `shell` hint (bash|powershell|cmd) shapes the printed watch_command only; the
    // bridge never sees it.
    let shell_hint = take_shell_hint(name, &mut args)?;
    let project_hint: Option<String> = args
        .get("project_path")
        .and_then(Value::as_str)
        .map(str::to_string);
    ensure_tool_mode(name)?;
    let call = dispatch::dispatch(name, &args, cancel_rx.as_ref());

    let project_hint = project_hint.as_deref();
    let mut text = match call {
        Ok(text) => text,
        Err(error) => {
            // WAIT_TIMEOUT recovery payloads carry a watch_command too.
            let error = error.to_string();
            return Err(match shell_hint {
                Some(hint) => apply_shell_hint(&error, hint, project_hint),
                None => error,
            });
        }
    };
    if let Some(hint) = shell_hint {
        text = apply_shell_hint(&text, hint, project_hint);
    }

    let structured = serde_json::from_str::<Value>(&text).ok();
    let mut result = json!({ "content": [{ "type": "text", "text": text }] });
    if let Some(value) = structured {
        if let Some(object) = result.as_object_mut() {
            object.insert(
                "structuredContent".into(),
                if value.is_object() {
                    value
                } else {
                    json!({"data": value})
                },
            );
        }
    }
    Ok(result)
}
