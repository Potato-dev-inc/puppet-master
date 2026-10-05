//! `wait_for_worker` and the spawn-readiness checks used before dispatching a prompt.

use super::*;

pub fn wait_for_worker(
    registry: &Arc<Mutex<PaneRegistry>>,
    pane_id: &str,
    timeout_ms: u64,
) -> Result<WaitForPanesResult, String> {
    wait_for_panes(
        registry,
        WaitForPanesRequest {
            pane_ids: vec![pane_id.to_string()],
            until: vec!["settled".into(), "error".into()],
            timeout_ms,
            task_id: None,
            output_regex: None,
            model_match: None,
        },
    )
}

/// Spawn readiness for `delegate_work`. Claude/Ink TUIs stay `running` and never
/// match `settled` (idle / waiting_input). A live pane with output is enough to
/// accept the task prompt. `wait_for_worker` after a prompt is unchanged.
pub fn wait_for_dispatch_ready(
    registry: &Arc<Mutex<PaneRegistry>>,
    pane_id: &str,
    timeout_ms: u64,
) -> Result<WaitForPanesResult, String> {
    let result = wait_for_panes(
        registry,
        WaitForPanesRequest {
            pane_ids: vec![pane_id.to_string()],
            until: vec!["settled".into(), "running".into(), "error".into()],
            timeout_ms,
            task_id: None,
            output_regex: None,
            model_match: None,
        },
    )?;
    if result.reason != "timeout" {
        return Ok(result);
    }
    // ponytail: Ink TUIs may never leave `running`; only proceed if the pane
    // is still live and has painted something. Dead/empty panes stay PANE_NOT_READY.
    if let Some(info) = status::pane_info(registry, pane_id) {
        if live_enough_for_dispatch(Some(&info.status))
            && pane_has_visible_output(registry, pane_id)
        {
            return Ok(wait_result(
                &info.status,
                pane_id,
                info.status.clone(),
                status::wait_snapshot(registry, pane_id).ok().flatten(),
                None,
                None,
                None,
                None,
                None,
            ));
        }
    }
    Ok(result)
}

/// Fast path for `wait_for_dispatch_ready` on opencode_native: one status read and no
/// wait. Returns the ready result plus the status so callers do not fetch it again.
pub fn native_fast_dispatch_ready(
    registry: &Arc<Mutex<PaneRegistry>>,
    pane_id: &str,
) -> Option<(
    WaitForPanesResult,
    crate::opencode::status::OpenCodeWorkerStatus,
)> {
    let status = crate::opencode::status::worker_status(registry, pane_id).ok()?;
    if !native_status_ready_for_dispatch(&status) {
        return None;
    }
    let result = wait_result(
        "running",
        pane_id,
        status.pane_status.clone(),
        None,
        None,
        None,
        None,
        None,
        None,
    );
    Some((result, status))
}

pub(super) fn pane_has_visible_output(registry: &Arc<Mutex<PaneRegistry>>, pane_id: &str) -> bool {
    registry_read_buffer(registry, pane_id, 40)
        .map(|buf| !buf.trim().is_empty())
        .unwrap_or(false)
}

pub(super) fn live_enough_for_dispatch(status: Option<&str>) -> bool {
    matches!(status, Some("running" | "idle" | "waiting_input"))
}
