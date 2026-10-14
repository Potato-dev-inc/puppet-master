//! Session tests: identity.

use super::*;

#[test]
fn sessions_start_in_agent_mode_and_catalog_is_filtered() {
    let first = id();
    let second = id();
    let agent = tools_for_session(&first);
    let names: HashSet<_> = agent.iter().map(|tool| tool.name).collect();
    for name in [
        "set_mode",
        "run_agent",
        "quick_ask",
        "wait_agents",
        "send_message",
        "followup_task",
        "interrupt_agent",
        "inspect_agent",
        "agent_transcript",
        "close_agent",
    ] {
        assert!(names.contains(name), "agent catalog missing {name}");
    }
    // Aliases and OpenCode tools are callable in agent mode but no longer listed.
    for hidden in ["send_agent", "cancel_agent", "list_workers", "reply_opencode_question"] {
        assert!(!names.contains(hidden), "{hidden} must be unlisted");
    }
    assert!(names.contains("list_panes"));
    assert!(names.contains("list_agents"));
    assert!(!names.contains("write_terminal_input"));
    assert!(!names.contains("spawn_agent"));
    let shell = set_mode(&second, "shell").unwrap();
    assert!(shell.iter().any(|tool| tool.name == "shell_exec"));
    assert!(!shell.iter().any(|tool| tool.name == "run_agent"));
    assert!(!shell.iter().any(|tool| tool.name == "quick_ask"));
    assert_eq!(mode_for(&first), McpMode::Agent);
}

#[test]
fn invalid_mode_is_rejected_and_does_not_change_session() {
    let session = id();
    assert!(set_mode(&session, "root").is_err());
    assert_eq!(mode_for(&session), McpMode::Agent);
}
