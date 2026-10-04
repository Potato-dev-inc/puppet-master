//! Session tests: pane ids.

use super::*;

#[test]
fn ambiguous_short_and_numeric_pane_ids_fail_with_guidance() {
    let ids = panes();
    assert_eq!(
        resolve_pane_id("57c0", &ids).unwrap_err().code,
        "AMBIGUOUS_PANE_ID"
    );
    let numeric = resolve_pane_id("362", &ids).unwrap_err();
    assert_eq!(numeric.code, "PANE_NOT_FOUND");
    assert!(numeric.message.contains("UI display number"));
    assert!(numeric.message.contains("list_panes"));
    let short = resolve_pane_id("57c", &ids).unwrap_err();
    assert!(short.message.contains("at least 4"));
}

#[test]
fn pane_id_prefix_passed_as_agent_handle_is_named_as_a_pane_id() {
    let ids = panes();
    let error = enrich_handle_error(agent_not_found("57c03490"), &ids);
    assert_eq!(error.code, "AGENT_NOT_FOUND");
    assert!(error
        .message
        .contains("pane id prefix, not an agent run handle"));
    assert!(error.message.contains(&ids[0]));
    let unrelated = enrich_handle_error(agent_not_found("c6399aed"), &ids);
    assert!(unrelated
        .message
        .contains("not registered to this MCP connection"));
    assert!(unrelated.message.contains("list_workers"));
}

#[test]
fn pane_id_as_handle_message_names_kinds_sources_and_run_handle() {
    let ids = panes();
    let error = enrich_handle_error_with(agent_not_found(&ids[0]), &ids, |_| {
        Some("run-xyz".to_string())
    });
    assert!(error.message.contains("a pane id, not an agent run handle"));
    assert!(error.message.contains("run_agent result field `handle`"));
    assert!(error.message.contains("list_agents `handle`"));
    assert!(error.message.contains("`run-xyz`"));
    assert_eq!(error.context["run_handle"], "run-xyz");
}
