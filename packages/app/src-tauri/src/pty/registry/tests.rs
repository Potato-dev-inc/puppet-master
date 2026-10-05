use super::*;

#[test]
fn write_enter_bytes_sends_cr() {
    let mut buf = Vec::new();
    write_enter_bytes(&mut buf, "powershell").expect("write enter");
    assert_eq!(buf, b"\r");
}

#[cfg(windows)]
#[test]
fn multi_line_prompts_go_through_the_api_but_keys_and_escapes_do_not() {
    // A task with acceptance criteria is several lines; it used to be classified as terminal
    // input, typed into the attached TUI and dropped (PROMPT_NOT_SUBMITTED after 45s).
    assert!(!is_pty_control_input(
        "Reply PONG.\n\nAcceptance criteria:\n- be brief"
    ));
    assert!(!is_pty_control_input("line one\r\nline two\ttabbed"));
    assert!(is_pty_control_input("\x1b[A"));
    assert!(is_pty_control_input("\x03"));
    assert!(is_pty_control_input("text with an escape \x1b inside"));
    assert!(is_pty_control_input("\x15"));
}

#[test]
fn cursor_agent_launches_through_interactive_powershell() {
    assert!(windows_powershell_host_agent(AgentType::CursorAgent));
    let (command, base_args) = resolve_command(AgentType::CursorAgent);
    assert_eq!(command, "cursor-agent.ps1");
    let invocation = powershell_cursor_agent_command(command, &base_args, &[]);
    assert!(invocation.contains("Get-Command -Name 'cursor-agent.ps1'"));
    assert!(invocation.starts_with("& (Get-Command"));
}
