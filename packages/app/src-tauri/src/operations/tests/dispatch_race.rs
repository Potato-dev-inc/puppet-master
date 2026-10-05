//! Operations tests: dispatch race.

use super::*;

/// Live regression (cursor_agent visible_close follow-up): the previous turn's TUI was still
/// repainting, a pane status event arrived while the follow-up was being dispatched, flipped the
/// operation Starting -> Running, and `dispatch_input_if_active` then refused to type the prompt.
/// The run was reported against a prompt that never reached the model.
#[test]
fn pane_activity_while_dispatching_does_not_cancel_the_dispatch() {
    let dir = temp();
    let starting = starting_in_dispatch(&dir, "dispatch-race");
    let project = dir.to_string_lossy().into_owned();
    let observed = mark_operation_observation(
        &project,
        &starting.operation_id,
        Some("running".into()),
        None,
        StateSource::Inferred,
    )
    .unwrap();
    assert_eq!(observed.status, OperationStatus::Starting);
    assert_eq!(observed.stage.as_deref(), Some("dispatching"));
    assert_eq!(observed.pane_state.as_deref(), Some("running"));
    let mut typed = false;
    dispatch_input_if_active(&project, &starting.operation_id, |_| {
        typed = true;
        Ok(())
    })
    .expect("the prompt must still be dispatched");
    assert!(typed);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn pane_activity_after_dispatch_still_marks_the_run_running() {
    let dir = temp();
    let project = dir.to_string_lossy().into_owned();
    let (queued, _) = create_operation(request(&dir, "after-dispatch")).unwrap();
    let done = run_operation_dispatch(&project, &queued.operation_id, |op| {
        op.stage = Some("dispatched_waiting_for_task_completion".into());
        Ok(())
    })
    .unwrap();
    assert_eq!(done.status, OperationStatus::Running);
    assert!(prompt_dispatched(&done));
    assert!(!dispatch_in_flight(&done.operation_id));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn dispatch_is_in_flight_only_while_the_closure_runs() {
    let dir = temp();
    let project = dir.to_string_lossy().into_owned();
    let (queued, _) = create_operation(request(&dir, "in-flight")).unwrap();
    let id = queued.operation_id.clone();
    run_operation_dispatch(&project, &id, |_| {
        assert!(dispatch_in_flight(&id));
        Ok(())
    })
    .unwrap();
    assert!(!dispatch_in_flight(&id));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn prompt_dispatched_reads_the_stage_log_of_the_current_turn() {
    // The failing live follow-up: previous turn delivered, this turn went straight to running.
    assert!(!prompt_dispatched(&marks(&[
        "dispatching",
        "dispatched_waiting_for_task_completion",
        "completed",
        "dispatching",
        "running",
    ])));
    assert!(prompt_dispatched(&marks(&[
        "dispatching",
        "dispatched_waiting_for_task_completion",
        "completed",
        "dispatching",
        "dispatched_waiting_for_task_completion",
    ])));
    // No record of a dispatch at all (an older snapshot): do not guess that it was skipped.
    assert!(prompt_dispatched(&marks(&["running"])));
    assert!(prompt_dispatched(&marks(&[])));
}
