//! Stopping runs whose OpenCode server is unreachable.

use crate::operations::{self, OperationStatus};
use std::fs;
use uuid::Uuid;

use super::fixtures::*;

const REFUSED: &str = "opencode abort session: http://127.0.0.1:4100/session/ses-live/abort: Connection Failed: Connect error: No connection could be made because the target machine actively refused it. (os error 10061)";

#[test]
fn only_an_unreachable_server_counts_as_unreachable() {
    use crate::opencode::client::is_unreachable_error;
    assert!(is_unreachable_error(REFUSED));
    assert!(is_unreachable_error(
        "opencode list messages: Connection refused"
    ));
    assert!(!is_unreachable_error(
        "opencode abort session: HTTP 404 session not found"
    ));
    assert!(!is_unreachable_error(
        "opencode abort session: HTTP 500 boom"
    ));
}

#[test]
fn cancel_and_close_finalise_a_run_whose_opencode_server_is_gone() {
    let project = std::env::temp_dir().join(format!("pm-oc-gone-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let running = running_turn(
        &project_path,
        "oc-gone",
        "opencode_native",
        Some("pane-oc"),
        true,
    );
    let view = crate::agent_runs::runtime::interrupt_active_turns(
        &project_path,
        "oc-gone",
        Some("persist-session"),
        |snapshot| {
            crate::agent_runs::runtime::settle_native_abort(
                &snapshot.project_path,
                &snapshot.operation_id,
                Err(REFUSED.to_string()),
            )
        },
        |_| Ok(()),
    )
    .expect("an unreachable server must not block the stop");
    assert_eq!(view.status, "interrupted");
    let stopped = operations::get_operation(&project_path, &running.operation_id).unwrap();
    assert_eq!(stopped.status, OperationStatus::Cancelled);
    assert!(stopped
        .warnings
        .iter()
        .any(|w| w.code == "OPENCODE_SERVER_UNREACHABLE"));
    let inspected = crate::agent_runs::runtime::inspect_agent(
        &project_path,
        "oc-gone",
        Some("persist-session"),
    )
    .unwrap();
    assert!(
        inspected["warnings"]
            .to_string()
            .contains("OPENCODE_SERVER_UNREACHABLE"),
        "{inspected}"
    );
    let _ = fs::remove_dir_all(project);
}

#[test]
fn a_server_that_answers_with_an_error_still_fails_the_stop() {
    let project = std::env::temp_dir().join(format!("pm-oc-err-{}", Uuid::new_v4()));
    let project_path = project.to_string_lossy().into_owned();
    let running = running_turn(
        &project_path,
        "oc-err",
        "opencode_native",
        Some("pane-oc"),
        true,
    );
    let error = crate::agent_runs::runtime::settle_native_abort(
        &running.project_path,
        &running.operation_id,
        Err("opencode abort session: HTTP 500 boom".into()),
    )
    .unwrap_err();
    assert_eq!(error.code, "INTERRUPT_FAILED");
    let _ = fs::remove_dir_all(project);
}

#[test]
fn a_run_stuck_on_a_dead_opencode_server_fails_after_the_grace_period() {
    use crate::agent_runs::native::{note_server_health, SERVER_LOSS_GRACE_MS};
    let id = format!("op-{}", Uuid::new_v4());
    assert_eq!(note_server_health(&id, false, 1_000), None);
    assert_eq!(
        note_server_health(&id, false, 1_000 + SERVER_LOSS_GRACE_MS - 1),
        None
    );
    // A blip that recovers resets the clock.
    assert_eq!(note_server_health(&id, true, 2_000), None);
    assert_eq!(note_server_health(&id, false, 70_000), None);
    assert_eq!(
        note_server_health(&id, false, 70_000 + SERVER_LOSS_GRACE_MS),
        Some(SERVER_LOSS_GRACE_MS)
    );
}
