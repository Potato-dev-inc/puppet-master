//! `wait_for_model` and `wait_for_task`: panes blocked on a model switch or a task-board status.

use super::*;

pub fn wait_for_model(
    registry: &Arc<Mutex<PaneRegistry>>,
    pane_id: &str,
    model_provider: Option<&str>,
    model_id: Option<&str>,
    timeout_ms: u64,
) -> Result<WaitForPanesResult, String> {
    wait_for_panes(
        registry,
        WaitForPanesRequest {
            pane_ids: vec![pane_id.to_string()],
            until: vec!["model_ready".into(), "tui_ready".into(), "error".into()],
            timeout_ms,
            task_id: None,
            output_regex: None,
            model_match: Some(WaitModelMatch {
                provider_id: model_provider.map(str::to_string),
                model_id: model_id.map(str::to_string),
            }),
        },
    )
}

pub fn wait_for_task(
    registry: &Arc<Mutex<PaneRegistry>>,
    pane_id: &str,
    task_id: &str,
    until: &[&str],
    timeout_ms: u64,
) -> Result<WaitForPanesResult, String> {
    wait_for_panes(
        registry,
        WaitForPanesRequest {
            pane_ids: vec![pane_id.to_string()],
            until: until.iter().map(|value| value.to_string()).collect(),
            timeout_ms,
            task_id: Some(task_id.to_string()),
            output_regex: None,
            model_match: None,
        },
    )
}
