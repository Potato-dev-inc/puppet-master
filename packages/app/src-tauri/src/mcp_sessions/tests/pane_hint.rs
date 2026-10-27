//! The pane an MCP connection says it runs in is a display hint. It must never decide a right.

use super::*;

#[test]
fn the_pane_a_connection_names_is_remembered_cleaned() {
    let session = id();
    assert_eq!(claimed_session_pane(&session), None);
    note_session_pane(&session, "  pane-1\r\nX-Evil: 1 ");
    assert_eq!(claimed_session_pane(&session).as_deref(), Some("pane-1X-Evil1"));
    note_session_pane(&session, "   ");
    assert_eq!(claimed_session_pane(&session).as_deref(), Some("pane-1X-Evil1"), "an empty claim changes nothing");
}

#[test]
fn naming_a_pane_grants_no_control_over_it() {
    let (owner, claimant, pane) = (id(), id(), id());
    register_shell_pane(&owner, &pane).unwrap();
    set_mode(&claimant, "shell").unwrap();
    // The claimant says it runs inside the owner's pane.
    note_session_pane(&claimant, &pane);
    let write = || {
        authorize(Some(&claimant), "POST", &format!("/panes/{pane}/input"), &serde_json::json!({"text": "x"}))
    };
    assert!(write().is_err(), "the claim opens no door");
    assert_eq!(relation_to_pane(&claimant, &pane), Relation::OtherAgent);
    assert_eq!(lineage_of_pane(&claimant, &pane), Relation::OtherAgent);
    // And it does not join the claimant to the owner's lineage or grant identity.
    assert_ne!(grant_identity(&claimant), grant_identity(&owner));
}

#[test]
fn a_connection_reports_its_process_id_once_it_sends_one() {
    let session = id();
    assert_eq!(session_pid(&session), None);
    note_session_pid(&session, 4242);
    assert_eq!(session_pid(&session), Some(4242));
}
