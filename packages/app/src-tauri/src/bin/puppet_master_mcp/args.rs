//! Argument extraction and URL-encoding helpers shared by the tool dispatchers.

use super::*;

pub(crate) fn required_string(args: &Value, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("missing required string argument: {key}"))
}

pub(crate) fn optional_number(args: &Value, key: &str) -> Option<u64> {
    args.get(key).and_then(Value::as_u64)
}

pub(crate) fn assert_worker_pane(pane_id: &str) -> Result<String, String> {
    if pane_id.starts_with("puppet-master-orchestrator-") {
        return Err(format!("refusing to target orchestrator pane: {pane_id}"));
    }
    Ok(pane_id.to_string())
}

pub(crate) fn encode_path_segment(input: &str) -> String {
    input
        .bytes()
        .flat_map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                vec![byte as char]
            }
            _ => format!("%{byte:02X}").chars().collect(),
        })
        .collect()
}
