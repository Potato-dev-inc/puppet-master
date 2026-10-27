use super::*;

#[test]
fn an_agent_without_a_session_file_is_read_off_its_screen_and_a_shell_is_not_read() {
    assert!(matches!(Opening::for_agent("no-such-agent"), Some(Opening::Screen(_))));
    assert!(Opening::for_agent("powershell").is_none());
    assert_eq!(Opening::for_agent("no-such-agent").unwrap().source_name(), "screen");
}
