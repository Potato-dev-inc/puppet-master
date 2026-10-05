//! Presenting a run with live context: model fields from the pane registry, pane open/closing
//! state, and the cursor-aware view a `wait_agents` wake returns.

use super::view::AgentRunView;
use crate::agent_runs::persist::{self};
use crate::operations::OperationSnapshot;
use parking_lot::Mutex;
use std::sync::Arc;

impl AgentRunView {
    pub fn present(
        snapshot: OperationSnapshot,
        registry: Option<&Arc<Mutex<crate::pty::PaneRegistry>>>,
    ) -> Self {
        let mut view = Self::from(snapshot.clone());
        let live = registry.and_then(|registry| persist::live_resolved_model(registry, &snapshot));
        let (requested, resolved) = persist::resolve_model_fields(
            snapshot.worker.requested_model.as_deref(),
            snapshot.worker.resolved_model.as_deref(),
            live,
            snapshot.agent_type == "opencode_native",
        );
        view.requested_model = requested;
        view.resolved_model = resolved;
        if view.resolved_model.is_some() {
            view.warnings
                .retain(|warning| warning.code != "MODEL_UNRESOLVED");
        }
        if let Some(receipt) = view.receipt.as_mut() {
            // Live registry knowledge may confirm the model after the snapshot was written.
            receipt.model = view
                .resolved_model
                .clone()
                .or_else(|| view.requested_model.clone());
            if view.requested_model.is_none() || view.requested_model == view.resolved_model {
                receipt
                    .not_verified
                    .retain(|entry| entry != "model not confirmed");
            }
        }
        if let (Some(registry), Some(pane_id)) = (registry, snapshot.pane_id.as_deref()) {
            let alive = registry.lock().panes.contains_key(pane_id);
            let busy = alive
                && snapshot.status.terminal()
                && crate::agent_runs::pane_close::should_dispose_after_terminal(&snapshot)
                && crate::operations::conflicting_pane_operation(
                    &snapshot.project_path,
                    pane_id,
                    &snapshot.operation_id,
                )
                .ok()
                .flatten()
                .is_some();
            let (open, closing, note) = pane_presentation(&snapshot, alive, busy);
            view.pane_open = Some(open);
            view.pane_closing = closing.then_some(true);
            if note.is_some() {
                view.pane_note = note;
            }
        }
        view
    }
}

/// `(pane_open, pane_closing, note)` at return time. A finished keep_pane=false run's pane is
/// closed right after the result is published, so it may still be in the registry when the view
/// is built; reporting it open would be false by the time the caller reads it.
pub(in crate::agent_runs) fn pane_presentation(
    snapshot: &OperationSnapshot,
    alive: bool,
    other_op_busy: bool,
) -> (bool, bool, Option<String>) {
    if alive
        && snapshot.status.terminal()
        && !other_op_busy
        && crate::agent_runs::pane_close::should_dispose_after_terminal(snapshot)
    {
        return (
            false,
            true,
            Some(
                "pane is being closed automatically now that the run finished (keep_pane=false)"
                    .into(),
            ),
        );
    }
    let note = (alive && snapshot.keep_pane && snapshot.status.terminal()).then(|| {
        "pane stays open (keep_pane=true); call close_agent when you are done with it".into()
    });
    (alive, false, note)
}

/// `caller_cursor` is the revision the caller explicitly said it had already seen, if any. The
/// result is omitted as unchanged only then; with no cursor there is nothing to be "unchanged"
/// since, so a run that finished before the wait started must still return its result.
pub fn agent_view_after_wait(
    snapshot: OperationSnapshot,
    caller_cursor: Option<u64>,
    wake_reason: Option<String>,
) -> AgentRunView {
    let mut view = AgentRunView::from(snapshot.clone());
    view.wake_reason = wake_reason;
    if caller_cursor.is_some_and(|seen| snapshot.revision <= seen) && view.result.is_some() {
        view.result = None;
        view.result_unchanged = Some(true);
    }
    view
}
