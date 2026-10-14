//! Run info for the desktop cockpit: what the run bound to a pane did (`pane_run_info`) and what it
//! said (`pane_transcript`). Read-only, local app only (no MCP session), and `None` for a pane with
//! no run (a pane the user opened by hand has none).

use crate::pty::PaneRegistry;
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::sync::Arc;

use super::runtime::{self, AgentRunView};

/// The (project, handle) of the latest run bound to `pane_id`, searching the default project and
/// the pane's own working directory (the same candidates `/agents/answer` uses).
pub(crate) fn run_of_pane(registry: &Arc<Mutex<PaneRegistry>>, pane_id: &str) -> Option<(String, String)> {
    let (default_project, cwd) = {
        let guard = registry.lock();
        (guard.project_path.clone(), guard.panes.get(pane_id).map(|pane| pane.info.cwd.clone()))
    };
    // A pane in an isolated run's worktree belongs to the run keyed on the origin project.
    let origin = cwd.as_deref().and_then(crate::worktrees::origin_of_cwd);
    [Some(default_project), origin, cwd]
        .into_iter()
        .flatten()
        .find_map(|project| super::latest_handle_for_pane(&project, pane_id).ok().map(|handle| (project, handle)))
}

/// `{ handle, status, receipt, error }` or `None` when the pane has no run. `receipt` is the run's
/// `RunReceipt` as JSON (null while the view carries none).
pub fn pane_run_info(registry: &Arc<Mutex<PaneRegistry>>, pane_id: &str) -> Result<Option<Value>, String> {
    let Some((project, handle)) = run_of_pane(registry, pane_id) else { return Ok(None) };
    let snapshot = crate::operations::latest_agent_run(&project, &handle).map_err(|e| e.message)?;
    let view = serde_json::to_value(AgentRunView::from(snapshot)).map_err(|e| e.to_string())?;
    Ok(Some(run_info_value(&view)))
}

/// The cockpit's view of a run: `error` carries the typed code (`WORKSPACE_TRUST_DECLINED`,
/// `AGENT_EXITED`, ...) so the Inbox can say in plain words why a run stopped.
fn run_info_value(view: &Value) -> Value {
    json!({
        "handle": view.get("handle").cloned().unwrap_or(Value::Null),
        "status": view.get("status").cloned().unwrap_or(Value::Null),
        "receipt": view.get("receipt").cloned().unwrap_or(Value::Null),
        "error": view.get("error").cloned().unwrap_or(Value::Null),
    })
}

/// `{ chunks: [{ stream, text }] }` (the transcript store caps it like `GET /agents/<h>/transcript`),
/// or `None` when the pane has no run.
pub fn pane_transcript(registry: &Arc<Mutex<PaneRegistry>>, pane_id: &str) -> Result<Option<Value>, String> {
    let Some((project, handle)) = run_of_pane(registry, pane_id) else { return Ok(None) };
    let page = runtime::transcript_for_handle(&project, &handle, 0).map_err(|e| e.message)?;
    let chunks: Vec<Value> = page.chunks.into_iter().map(|c| json!({ "stream": c.stream, "text": c.text })).collect();
    Ok(Some(json!({ "chunks": chunks })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_info_carries_the_typed_error_of_a_failed_run() {
        let view = json!({"handle": "h", "status": "failed", "receipt": null,
            "error": {"code": "WORKSPACE_TRUST_DECLINED", "message": "m", "recoverable": false}});
        let info = run_info_value(&view);
        assert_eq!(info["error"]["code"], "WORKSPACE_TRUST_DECLINED");
        assert_eq!(info["status"], "failed");
        assert!(run_info_value(&json!({"handle": "h", "status": "completed"}))["error"].is_null());
    }

    #[test]
    fn a_pane_without_a_run_has_no_run_info_or_transcript() {
        let registry = Arc::new(Mutex::new(PaneRegistry::new()));
        assert_eq!(pane_run_info(&registry, "no-such-pane").unwrap(), None);
        assert_eq!(pane_transcript(&registry, "no-such-pane").unwrap(), None);
    }
}
