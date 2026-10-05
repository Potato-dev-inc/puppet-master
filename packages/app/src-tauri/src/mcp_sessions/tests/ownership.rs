//! Session tests: ownership.

use super::*;

#[test]
fn release_returns_control_to_original_owner_without_removing_managed_pane_record() {
    let owner = id();
    register_owned_pane(&owner, "agent-pane-2").unwrap();
    release_owned_pane(&owner, "agent-pane-2").unwrap();
    assert!(session_controls_pane(&owner, "agent-pane-2"));
    take_over_pane(&owner, "agent-pane-2", false).unwrap();
}

#[test]
fn registration_cannot_steal_a_managed_pane_or_agent() {
    let owner = id();
    let other = id();
    register_owned_pane(&owner, "protected-pane").unwrap();
    register_owned_agent(&owner, "protected-agent").unwrap();
    assert!(register_owned_pane(&other, "protected-pane").is_err());
    assert!(register_owned_agent(&other, "protected-agent").is_err());
    take_over_agent(&other, "protected-agent", true).unwrap();
    assert!(check_run_access(&other, "protected-agent").is_ok());
}

#[test]
fn agent_handles_are_private_until_the_owner_releases_them() {
    let owner = id();
    let other = id();
    set_mode(&owner, "both").unwrap();
    set_mode(&other, "both").unwrap();
    register_owned_agent(&owner, "owned-run").unwrap();
    assert!(authorize(
        Some(&owner),
        "GET",
        "/agents/owned-run/transcript",
        &Value::Null
    )
    .is_ok());
    assert!(authorize(
        Some(&other),
        "GET",
        "/agents/owned-run/transcript",
        &Value::Null
    )
    .is_err());
    take_over_agent(&other, "owned-run", true).unwrap();
    assert!(check_run_access(&other, "owned-run").is_ok());
    release_owned_agent(&other, "owned-run").unwrap();
    take_over_agent(&owner, "owned-run", true).unwrap();
    assert!(check_run_access(&owner, "owned-run").is_ok());
}

#[test]
fn check_run_access_failure_carries_the_handle_for_enrichment() {
    let error = check_run_access(&id(), "no-such-run").unwrap_err();
    assert_eq!(
        error.context.get("handle").and_then(Value::as_str),
        Some("no-such-run")
    );
}

#[test]
fn taken_over_user_pane_can_be_registered_for_agent_dispatch() {
    let session = id();
    let other = id();
    take_over_pane(&session, "cursor-tui", true).unwrap();
    assert!(session_controls_pane(&session, "cursor-tui"));
    register_owned_pane(&session, "cursor-tui").unwrap();
    register_owned_pane(&session, "cursor-tui").unwrap();
    assert!(session_controls_pane(&session, "cursor-tui"));
    set_mode(&session, "both").unwrap();
    assert!(authorize(
        Some(&session),
        "POST",
        "/panes/cursor-tui/input",
        &serde_json::json!({"text": "x"}),
    )
    .is_ok());
    let denied = register_owned_pane(&other, "cursor-tui").unwrap_err();
    assert_eq!(denied.code, "AUTHORIZATION_DENIED");
}

#[test]
fn runs_of_a_dead_mcp_process_become_readable_but_a_live_one_stays_private() {
    let owner = id();
    let other = id();
    register_owned_agent(&owner, "dead-handle").unwrap();
    // A pid that cannot exist: the process behind `owner` is gone.
    note_session_pid(&owner, u32::MAX - 1);
    assert!(check_run_read_access(&other, "dead-handle").is_ok());
    assert!(check_run_wait_access(&other, "dead-handle").is_ok());
    // Mutation still needs an explicit take_over.
    assert!(check_run_access(&other, "dead-handle").is_err());

    let live_owner = id();
    register_owned_agent(&live_owner, "live-handle").unwrap();
    note_session_pid(&live_owner, std::process::id());
    assert!(check_run_read_access(&other, "live-handle").is_err());
}

#[test]
fn unauthorized_takeover_stays_blocked_even_with_a_coordinator_name() {
    let owner = id();
    let other = id();
    register_owned_agent(&owner, "locked-worker").unwrap();
    assert!(take_over_agent(&other, "locked-worker", false).is_err());
    assert_eq!(
        attach_coordinator(&other, None, Some("alice")).unwrap_err().code,
        "AUTHORIZATION_DENIED"
    );
    take_over_agent(&other, "locked-worker", true).unwrap();
    assert!(check_run_access(&other, "locked-worker").is_ok());
}

#[test]
fn return_pane_control_does_not_drop_the_worker_lease() {
    let owner = id();
    register_owned_agent(&owner, "leased-worker").unwrap();
    register_owned_pane(&owner, "manual-pane").unwrap();
    take_over_pane(&owner, "manual-pane", true).unwrap();
    return_pane_control(&owner, "manual-pane").unwrap();
    assert!(check_run_access(&owner, "leased-worker").is_ok());
    let lease = inspect_lease("leased-worker").unwrap();
    assert!(!lease.released);
    assert_eq!(lease.controller.as_deref(), Some(owner.as_str()));
    assert!(session_controls_pane(&owner, "manual-pane"));
}

#[test]
fn take_over_agent_grant_reclaims_without_prior_control() {
    let owner = id();
    let other = id();
    register_owned_agent(&owner, "todo-builder").unwrap();
    take_over_agent(&other, "todo-builder", true).unwrap();
    assert!(check_run_access(&other, "todo-builder").is_ok());
    assert!(check_run_read_access(&owner, "todo-builder").is_ok());
}

#[test]
fn terminal_run_lease_release_leaves_it_reclaimable_by_owner_only_via_token_or_grant() {
    let owner = id();
    let other = id();
    let handle = format!("run-{}", id());
    register_owned_agent(&owner, &handle).unwrap();
    note_session_pid(&owner, std::process::id());
    let identity = session_identity(&owner);
    release_agent_lease(&owner, &handle).unwrap();
    // Released: nobody controls it, but a stranger still needs an explicit grant.
    assert!(check_run_access(&other, &handle).is_err());
    let replacement = id();
    attach_coordinator(&replacement, Some(&identity.attach_token), None).unwrap();
    // The owner's new connection may take its own released lease back.
    take_over_agent(&replacement, &handle, true).unwrap();
    assert!(check_run_access(&replacement, &handle).is_ok());
}
