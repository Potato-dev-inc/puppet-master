//! Operations tests: a run handed over to another agent (Keep going) and the copy it joined.

use super::*;
use crate::worktrees::WorktreeRef;
use serde_json::json;

fn worktree_of(handle: &str) -> WorktreeRef {
    WorktreeRef {
        path: format!("C:\\Users\\you\\wt\\{handle}"),
        cwd: format!("C:\\Users\\you\\wt\\{handle}"),
        branch: format!("pm/{handle}"),
        base: "main".into(),
        base_commit: "0000000".into(),
        repo: "C:\\Users\\you\\repo".into(),
        handle: handle.into(),
        created_at_ms: 1,
        closed: None,
        changes: None,
        setup: None,
        pr: None,
    }
}

fn running(dir: &Path, key: &str) -> OperationSnapshot {
    let started = starting_in_dispatch(dir, key);
    mark_operation_state(
        &dir.to_string_lossy(),
        &started.operation_id,
        OperationStatus::Running,
        StateSource::Native,
        Some("running".into()),
        None,
        None,
    )
    .unwrap()
}

#[test]
fn a_running_run_is_handed_off_as_cancelled_with_its_pane_kept() {
    let dir = temp();
    let project = dir.to_string_lossy().into_owned();
    let op = running(&dir, "handed-running");
    let after = mark_operation_handed_off(&project, &op.operation_id, json!({ "keep_going": "kg-1" })).unwrap();
    assert_eq!(after.status, OperationStatus::Cancelled);
    assert_eq!(after.stage.as_deref(), Some("handed_off"));
    assert!(after.keep_pane);
    assert!(after.required_action.is_none());
    assert!(after.finished_at_ms.is_some());
    let error = after.error.expect("an error");
    assert_eq!(error.code, HANDED_OFF);
    assert_eq!(error.code, "HANDED_OFF");
    assert!(!error.recoverable);
    assert_eq!(error.context["keep_going"], "kg-1");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn a_run_parked_at_a_usage_limit_can_be_handed_off() {
    let dir = temp();
    let project = dir.to_string_lossy().into_owned();
    let op = running(&dir, "handed-limit");
    mark_operation_usage_limit(&project, &op.operation_id, json!({ "kind": USAGE_LIMIT_KIND })).unwrap();
    assert_eq!(get_operation(&project, &op.operation_id).unwrap().status, OperationStatus::WaitingInput);
    let after = mark_operation_handed_off(&project, &op.operation_id, json!({})).unwrap();
    assert_eq!(after.status, OperationStatus::Cancelled);
    assert_eq!(after.error.map(|e| e.code).as_deref(), Some(HANDED_OFF));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn a_finished_run_is_left_alone_by_a_hand_over() {
    let dir = temp();
    let project = dir.to_string_lossy().into_owned();
    let op = running(&dir, "handed-done");
    mark_operation_finished(&project, &op.operation_id, Some("done".into()), ResultCapture::Authoritative, false).unwrap();
    let before = get_operation(&project, &op.operation_id).unwrap();
    assert_eq!(before.status, OperationStatus::Completed);
    let after = mark_operation_handed_off(&project, &op.operation_id, json!({ "keep_going": "kg-2" })).unwrap();
    assert_eq!(after.status, OperationStatus::Completed);
    assert!(after.error.is_none());
    assert_eq!(after.revision, before.revision, "nothing was written");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn set_handed_off_to_adds_continued_by_and_keeps_the_other_context() {
    let dir = temp();
    let project = dir.to_string_lossy().into_owned();
    let op = running(&dir, "handed-merge");
    mark_operation_handed_off(&project, &op.operation_id, json!({ "keep_going": "kg-3" })).unwrap();
    let after = set_handed_off_to(&project, &op.operation_id, json!({ "handle": "h-codex", "agent_type": "codex" })).unwrap();
    let context = after.error.expect("an error").context;
    assert_eq!(context["keep_going"], "kg-3");
    assert_eq!(context["continued_by"]["handle"], "h-codex");
    assert_eq!(context["continued_by"]["agent_type"], "codex");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn set_handed_off_to_is_a_no_op_for_any_other_run() {
    let dir = temp();
    let project = dir.to_string_lossy().into_owned();
    // A run still going: no error at all.
    let live = running(&dir, "merge-live");
    let after = set_handed_off_to(&project, &live.operation_id, json!({ "handle": "h" })).unwrap();
    assert!(after.error.is_none());
    // A run cancelled for another reason: its error is not touched.
    let other = running(&dir, "merge-other");
    mark_operation_usage_limit(&project, &other.operation_id, json!({ "kind": USAGE_LIMIT_KIND })).unwrap();
    let stopped = mark_operation_stopped_at_limit(&project, &other.operation_id, json!({})).unwrap();
    let after = set_handed_off_to(&project, &other.operation_id, json!({ "handle": "h" })).unwrap();
    assert_eq!(after.error.as_ref().map(|e| e.code.as_str()), Some(USAGE_LIMIT_STOPPED));
    assert_eq!(after.revision, stopped.revision, "nothing was written");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn a_copy_joined_by_a_run_stays_active_until_that_run_ends() {
    let dir = temp();
    let project = dir.to_string_lossy().into_owned();
    // The copy's own run ended long ago.
    let mut owner_req = request(&dir, "copy-owner");
    owner_req.agent_run_id = Some("copy-owner".into());
    let (owner, _) = create_operation(owner_req).unwrap();
    mark_operation_state(&project, &owner.operation_id, OperationStatus::Cancelled, StateSource::Native, None, None, None).unwrap();
    assert!(!crate::operations::handle_is_active(&project, "copy-owner"));

    // A Keep going run joined that copy and works in it now.
    let mut joiner_req = request(&dir, "copy-joiner");
    joiner_req.agent_run_id = Some("copy-joiner".into());
    joiner_req.worker.worktree = Some(worktree_of("copy-owner"));
    let (joiner, _) = create_operation(joiner_req).unwrap();
    assert!(
        crate::operations::handle_is_active(&project, "copy-owner"),
        "a joined run keeps Merge and Discard away"
    );

    mark_operation_handed_off(&project, &joiner.operation_id, json!({})).unwrap();
    assert!(
        !crate::operations::handle_is_active(&project, "copy-owner"),
        "once that run has ended the copy is free again"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn an_empty_handle_is_never_active_through_a_joined_run() {
    let dir = temp();
    let project = dir.to_string_lossy().into_owned();
    let mut req = request(&dir, "empty-handle");
    req.worker.worktree = Some(worktree_of(""));
    create_operation(req).unwrap();
    assert!(!crate::operations::handle_is_active(&project, ""));
    let _ = fs::remove_dir_all(dir);
}
