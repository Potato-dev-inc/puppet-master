//! Session tests: reconnect.

use super::*;

#[test]
fn reconnect_attaches_with_token_without_the_old_connection() {
    let original = id();
    register_owned_agent(&original, "worker-a").unwrap();
    register_owned_pane(&original, "pane-a").unwrap();
    let identity = session_identity(&original);
    let replacement = id();
    let attached = attach_coordinator(&replacement, Some(&identity.attach_token), None).unwrap();
    assert_eq!(attached["coordinator_id"], identity.coordinator_id);
    assert!(check_run_access(&replacement, "worker-a").is_ok());
    assert!(check_run_access(&original, "worker-a").is_err());
    assert!(session_controls_pane(&replacement, "pane-a"));
    assert!(!session_controls_pane(&original, "pane-a"));
    let lease = inspect_lease("worker-a").unwrap();
    assert_eq!(lease.owner, replacement);
    assert_eq!(lease.coordinator_id, identity.coordinator_id);
    assert!(!lease.released);
}

/// Simulate a bridge restart for one session: persist, drop its in-memory entries, reload.
fn restart_for(session: &str, handles: &[&str]) {
    let snapshot = {
        let state = lock_state();
        let mut snap = snapshot_state(&state);
        // Round-trip through text exactly as the file would.
        snap = serde_json::from_str(&snap.to_string()).unwrap();
        snap
    };
    let mut state = lock_state();
    let coordinator = state.sessions.remove(session).map(|s| s.coordinator_id);
    if let Some(coordinator) = coordinator {
        state.coordinators.remove(&coordinator);
    }
    for handle in handles {
        state.agents.remove(*handle);
        state.panes.remove(*handle);
    }
    restore_snapshot(&mut state, &snapshot);
}

#[test]
fn restart_keeps_owner_access_and_token_reclaim() {
    let owner = id();
    let handle = format!("run-{}", id());
    register_owned_agent(&owner, &handle).unwrap();
    let identity = session_identity(&owner);
    restart_for(&owner, &[&handle]);
    // Same transport id still controls its run after restart.
    assert!(check_run_access(&owner, &handle).is_ok());
    assert_eq!(session_identity(&owner).attach_token, identity.attach_token);
    // A new connection with the valid token reclaims it.
    let replacement = id();
    let attached = attach_coordinator(&replacement, Some(&identity.attach_token), None).unwrap();
    assert!(attached["handles"].as_array().unwrap().iter().any(|h| h == handle.as_str()));
    assert!(check_run_access(&replacement, &handle).is_ok());
}

#[test]
fn restart_reclaim_rejects_wrong_token_and_other_sessions() {
    let owner = id();
    let other = id();
    let handle = format!("run-{}", id());
    register_owned_agent(&owner, &handle).unwrap();
    let identity = session_identity(&owner);
    restart_for(&owner, &[&handle]);
    let attacker = id();
    assert_eq!(
        attach_coordinator(&attacker, Some("not-the-token"), None).unwrap_err().code,
        "AUTHORIZATION_DENIED"
    );
    assert_eq!(
        attach_coordinator(&attacker, None, Some(&identity.coordinator_id)).unwrap_err().code,
        "AUTHORIZATION_DENIED"
    );
    assert!(check_run_access(&attacker, &handle).is_err());
    assert!(check_run_access(&other, &handle).is_err());
    assert!(take_over_agent(&other, &handle, false).is_err());
}

#[test]
fn reconnect_recovers_only_the_owners_panes_never_another_live_connections() {
    let owner = id();
    let neighbour = id();
    let own_pane = format!("pane-{}", id());
    let their_pane = format!("pane-{}", id());
    let own_run = format!("run-{}", id());
    let their_run = format!("run-{}", id());
    register_owned_pane(&owner, &own_pane).unwrap();
    register_owned_agent(&owner, &own_run).unwrap();
    register_owned_pane(&neighbour, &their_pane).unwrap();
    register_owned_agent(&neighbour, &their_run).unwrap();
    note_session_pid(&neighbour, std::process::id());
    let owner_identity = session_identity(&owner);

    // The owner's connection drops; a new one reattaches with the owner's token.
    let replacement = id();
    let attached =
        attach_coordinator(&replacement, Some(&owner_identity.attach_token), None).unwrap();
    let panes = attached["panes"].as_array().unwrap();
    assert!(panes.iter().any(|p| p == own_pane.as_str()));
    assert!(!panes.iter().any(|p| p == their_pane.as_str()));
    assert!(attached["handles"].as_array().unwrap().iter().all(|h| h != their_run.as_str()));
    assert!(session_controls_pane(&replacement, &own_pane));
    assert!(check_run_access(&replacement, &own_run).is_ok());

    // The live neighbour keeps its own and the reconnected owner cannot touch them.
    assert!(session_controls_pane(&neighbour, &their_pane));
    assert!(!session_controls_pane(&replacement, &their_pane));
    assert!(check_run_access(&neighbour, &their_run).is_ok());
    assert!(check_run_access(&replacement, &their_run).is_err());
    assert!(check_run_read_access(&replacement, &their_run).is_err());
    assert!(check_run_wait_access(&replacement, &their_run).is_err());
    // Presenting the neighbour's id or the owner's token again does not move their lease.
    assert!(attach_coordinator(&replacement, Some("guess"), Some("whoever")).is_err());
    assert!(inspect_lease(&their_run).unwrap().owner == neighbour);
}

#[test]
fn caller_supplied_coordinator_name_does_not_grant_authority() {
    let owner = id();
    let other = id();
    register_owned_agent(&owner, "named-worker").unwrap();
    let identity = session_identity(&owner);
    assert!(attach_coordinator(&other, None, Some(&identity.coordinator_id)).is_err());
    assert!(attach_coordinator(&other, Some("not-the-token"), Some("alice")).is_err());
    assert!(transfer_agent(&other, "named-worker", None, Some("alice")).is_err());
    assert!(transfer_agent(&owner, "named-worker", Some("alice"), Some("alice")).is_err());
    assert!(check_run_access(&owner, "named-worker").is_ok());
    assert!(check_run_access(&other, "named-worker").is_err());
}
