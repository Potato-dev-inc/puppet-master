//! Team tools (`team_roster`, `team_events`, `team_wait`, `team_notify`): maps each tool call to its
//! bridge request. The app does the work (`bridge/routes/team_mcp.rs`); the shim only shapes the
//! long-poll of `team_wait`, in short bridge slices like `ask_user`, so a cancelled call stops
//! quickly and a bridge hiccup between slices loses nothing.

use super::super::*;
use std::sync::mpsc::Receiver;

/// One bridge slice of `team_wait`.
const SLICE_MS: u64 = 4_000;
const DEFAULT_WAIT_MS: u64 = 30_000;
const MAX_WAIT_MS: u64 = 120_000;
const REQUEST_CANCELLED: &str = r#"{"code":"REQUEST_CANCELLED","message":"team_wait was cancelled","recoverable":true,"context":{}}"#;

/// Runs `name` when this family owns it; `None` lets the next family try.
pub(super) fn dispatch(
    name: &str,
    args: &Value,
    cancel_rx: Option<&Receiver<()>>,
) -> Option<Result<String, String>> {
    match name {
        "team_roster" => Some(bridge_request("POST", "/team/agent/roster", Some(args.clone()))),
        "team_events" => Some(bridge_request("POST", "/team/agent/events", Some(args.clone()))),
        "team_notify" => Some(bridge_request("POST", "/team/agent/notify", Some(args.clone()))),
        "team_wait" => Some(wait(args, cancel_rx)),
        _ => None,
    }
}

fn timed_out(filter: &Value, since: Option<u64>, timeout_ms: u64) -> String {
    let mut next = filter.clone();
    if let (Some(since), Some(object)) = (since, next.as_object_mut()) {
        object.insert("since".into(), json!(since));
    }
    json!({
        "code": "WAIT_TIMEOUT",
        "message": format!("no team event matched within {timeout_ms} ms; nothing is wrong, wait again from `since`"),
        "recoverable": true,
        "since": since,
        "next_action": { "tool": "team_wait", "filter": next },
        "context": { "tool": "team_wait" },
    })
    .to_string()
}

/// How long `team_wait` waits: `timeout_ms`, default 30 s, at most 120 s (like `wait_agents`).
pub(crate) fn team_wait_timeout_ms(args: &Value) -> u64 {
    optional_number(args, "timeout_ms").unwrap_or(DEFAULT_WAIT_MS).clamp(1, MAX_WAIT_MS)
}

fn wait(args: &Value, cancel_rx: Option<&Receiver<()>>) -> Result<String, String> {
    let timeout_ms = team_wait_timeout_ms(args);
    let filter = args.get("filter").cloned().unwrap_or_else(|| json!({}));
    if !filter.is_object() {
        return Err("filter must be an object".into());
    }
    let mut since = filter.get("since").and_then(Value::as_u64);
    let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        if cancel_rx.is_some_and(|rx| rx.try_recv().is_ok()) {
            return Err(REQUEST_CANCELLED.into());
        }
        let remaining = deadline.saturating_duration_since(std::time::Instant::now()).as_millis() as u64;
        if remaining == 0 {
            return Err(timed_out(&filter, since, timeout_ms));
        }
        let mut body = json!({ "filter": filter, "wait_ms": remaining.clamp(1, SLICE_MS) });
        if let Some(since) = since {
            body["since"] = json!(since);
        }
        let response = match bridge_request("POST", "/team/agent/wait", Some(body)) {
            Ok(text) => text,
            // The app is reconnecting. Once the starting point is fixed, nothing is lost by trying again.
            Err(error) if since.is_some() && error.starts_with("bridge_down") => {
                std::thread::sleep(Duration::from_millis(500));
                continue;
            }
            Err(error) => return Err(error),
        };
        let value: Value = serde_json::from_str(&response).map_err(|e| format!("invalid team_wait response: {e}"))?;
        if value.get("matched").and_then(Value::as_bool) == Some(true) {
            return Ok(response);
        }
        // The app picked the starting point of this wait (the moment it began): keep it, so an event
        // that arrives between two slices is still seen.
        since = value.get("since").and_then(Value::as_u64).or(since);
    }
}
