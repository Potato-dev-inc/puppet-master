//! Helpers shared by more than one test topic.

use crate::opencode::messages::{MessageView, SessionMessagesView};
use crate::operations::{self, DelegateWorkRequest, OperationStatus, ResultCapture, WorkerPersist};
use serde_json::json;
use uuid::Uuid;

pub(super) fn view(messages: Vec<MessageView>) -> SessionMessagesView {
    SessionMessagesView {
        pane_id: "pane-native".into(),
        session_id: "session-native".into(),
        messages,
        pending_question: None,
        last_assistant_text: None,
    }
}

pub(super) fn parked_snapshot(
    status: OperationStatus,
    kind: &str,
) -> operations::OperationSnapshot {
    let project = std::env::temp_dir().join(format!("pm-agent-trust-{}", Uuid::new_v4()));
    let request = DelegateWorkRequest {
        project_path: project.to_string_lossy().into_owned(),
        task: "inspect".into(),
        agent_type: "cursor_agent".into(),
        pane_id: None,
        idempotency_key: Uuid::new_v4().to_string(),
        acceptance_criteria: Some(vec!["report findings".into()]),
        task_id: None,
        exclusive: false,
        locks: vec![],
        timeout_ms: 900_000,
        read_only: true,
        keep_pane: false,
        placement: None,
        owner_session_id: Some("test-session".into()),
        worker_has_mcp_tools: false,
        agent_run_id: None,
        turn_index: 0,
        worker: Default::default(),
        context_policy: None,
        checks: Vec::new(),
    };
    let mut snapshot = operations::create_operation(request).unwrap().0;
    snapshot.status = status;
    snapshot.required_action = Some(json!({"kind": kind}));
    snapshot
}

pub(super) fn persist_req(
    project: &str,
    handle: &str,
    turn: u32,
    task: &str,
) -> DelegateWorkRequest {
    DelegateWorkRequest {
        project_path: project.to_string(),
        task: task.into(),
        agent_type: "codex".into(),
        pane_id: None,
        idempotency_key: Uuid::new_v4().to_string(),
        acceptance_criteria: Some(vec!["return a result".into()]),
        task_id: None,
        exclusive: false,
        locks: vec![],
        timeout_ms: 900_000,
        read_only: false,
        keep_pane: false,
        placement: None,
        owner_session_id: Some("persist-session".into()),
        worker_has_mcp_tools: false,
        agent_run_id: Some(handle.into()),
        turn_index: turn,
        worker: WorkerPersist::default(),
        context_policy: None,
        checks: Vec::new(),
    }
}

pub(super) fn finish_turn(project: &str, operation_id: &str, result: &str) {
    operations::run_operation_dispatch(project, operation_id, |_| Ok(())).unwrap();
    operations::mark_operation_finished(
        project,
        operation_id,
        Some(result.into()),
        ResultCapture::Authoritative,
        true,
    )
    .unwrap();
}

pub(super) fn running_turn(
    project: &str,
    handle: &str,
    agent_type: &str,
    pane_id: Option<&str>,
    pane_created: bool,
) -> operations::OperationSnapshot {
    let mut request = persist_req(project, handle, 0, "current turn");
    request.agent_type = agent_type.into();
    request.pane_id = pane_id.map(str::to_string);
    if agent_type == "opencode_native" {
        request.worker.provider_session_id = Some("ses-live".into());
    }
    let queued = operations::create_operation(request).unwrap().0;
    operations::run_operation_dispatch(project, &queued.operation_id, |snapshot| {
        snapshot.pane_id = pane_id.map(str::to_string);
        snapshot.pane_created = pane_created;
        Ok(())
    })
    .unwrap()
}
