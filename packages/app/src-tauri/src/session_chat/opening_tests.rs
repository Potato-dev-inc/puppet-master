use super::*;

#[test]
fn an_agent_without_a_session_file_is_read_off_its_screen_and_a_shell_is_not_read() {
    assert!(matches!(Opening::for_agent("no-such-agent"), Some(Opening::Screen(_))));
    assert!(Opening::for_agent("powershell").is_none());
    assert_eq!(Opening::for_agent("no-such-agent").unwrap().source_name(), "screen");
}

#[test]
fn an_agent_that_serves_its_session_over_an_api_is_read_from_there_with_the_screen_behind_it() {
    for agent in ["opencode", "opencode_native"] {
        let opening = Opening::for_agent(agent).unwrap_or_else(|| panic!("{agent} has a chat"));
        assert!(matches!(opening, Opening::Api(..)), "{agent}");
        assert_eq!(opening.source_name(), "api");
    }
    // The agents with a session file keep reading it.
    assert!(matches!(Opening::for_agent("claude"), Some(Opening::File(_))));
}
