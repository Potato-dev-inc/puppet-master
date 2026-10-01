//! Rebind a worker when its native process is replaced under the same pane id.

use super::messaging;
use super::persist;
use crate::operations::{self, OperationSnapshot};
use parking_lot::Mutex;
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Arc;
use tauri::AppHandle;

pub fn on_native_pane_replaced(
    pane_id: &str,
    previous_session: &str,
    new_session: &str,
    registry: &Arc<Mutex<crate::pty::PaneRegistry>>,
    app: &AppHandle,
) {
    let project = registry.lock().project_path.clone();
    let operations = operations::list_indexed_operations(&project).unwrap_or_default();
    let mut latest: BTreeMap<String, OperationSnapshot> = BTreeMap::new();
    for snapshot in operations {
        if snapshot.pane_id.as_deref() != Some(pane_id) || snapshot.worker.closed {
            continue;
        }
        let handle = if snapshot.agent_run_id.is_empty() {
            snapshot.operation_id.clone()
        } else {
            snapshot.agent_run_id.clone()
        };
        let replace = latest.get(&handle).map_or(true, |existing| {
            snapshot.turn_index >= existing.turn_index
        });
        if replace {
            latest.insert(handle, snapshot);
        }
    }
    for (handle, snapshot) in latest {
        let running = !snapshot.status.terminal();
        let updated = match operations::rebind_provider_session(
            &snapshot.project_path,
            &snapshot.operation_id,
            new_session,
            Some(previous_session),
            true,
            Some(json!({
                "kind": if previous_session == new_session {
                    "process_restarted"
                } else {
                    "session_replaced"
                },
                "previous_session": previous_session,
                "session_id": new_session,
                "pane_id": pane_id,
                "context": if previous_session == new_session {
                    "replayed"
                } else {
                    "replayed"
                },
                "detail": "OpenCode process was replaced under the same pane id; rebound to the current native session"
            })),
        ) {
            Ok(updated) => updated,
            Err(_) => continue,
        };
        messaging::expire_unsettled_steers(&snapshot.project_path, &handle);
        if running {
            let facts = persist::conversation_facts(&snapshot.project_path, &handle);
            let rendered = persist::render_turn_prompt(persist::TurnPromptArgs {
                policy: persist::followup_policy(None, snapshot.worker.context_policy),
                task: &snapshot.task,
                scope: snapshot.worker.scope.as_deref(),
                prior_result: snapshot.result.as_deref(),
                prior_user: facts.as_deref(),
                selected_history: &[],
                can_resume: false,
                is_followup: true,
            });
            if let Err(error) =
                crate::opencode::prompt_native_session(registry, app, pane_id, &rendered.prompt)
            {
                let failure = operations::OperationError::new(
                    "SESSION_REPLACED",
                    format!(
                        "worker process was replaced; replay on the new session failed: {error}"
                    ),
                    true,
                );
                if let Ok(failed) = operations::mark_operation_state(
                    &snapshot.project_path,
                    &snapshot.operation_id,
                    operations::OperationStatus::Failed,
                    operations::StateSource::Native,
                    Some("session_replaced".into()),
                    None,
                    Some(failure),
                ) {
                    let _ = crate::bridge::publish_operation(&failed, registry, app);
                    continue;
                }
            } else {
                let _ = operations::clear_session_reset(
                    &snapshot.project_path,
                    &snapshot.operation_id,
                );
            }
        }
        let published = operations::get_operation(&snapshot.project_path, &snapshot.operation_id)
            .unwrap_or(updated);
        let _ = crate::bridge::publish_operation(&published, registry, app);
    }
}
