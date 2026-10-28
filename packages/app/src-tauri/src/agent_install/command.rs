//! The PowerShell text a visible install pane runs, and how Puppet Master notices it is done.
//!
//! The pane is `powershell.exe -NoLogo -NoExit -Command <script>`. The script is assembled here
//! from a `Step` whose tokens already passed the spec validator, and every token is quoted again
//! as a PowerShell single-quoted string (`'` doubled), so no token can end the string or start a
//! second command. The window stays open (`-NoExit`) so the person can read the result.

use super::plan::{Action, Step};

/// Printed on its own line when the command has finished: `PM-INSTALL-EXIT <code>`.
pub const DONE_MARKER: &str = "PM-INSTALL-EXIT";

/// `'text'` with embedded quotes doubled: the only escaping a PowerShell single-quoted string has.
pub fn ps_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// The `-Command` argument for one step.
pub fn powershell_script(step: &Step) -> String {
    let call = match &step.action {
        Action::Argv { exe, args } => {
            let mut parts = vec![format!("& {}", ps_quote(exe))];
            parts.extend(args.iter().map(|a| ps_quote(a)));
            parts.join(" ")
        }
        Action::Script { url } => format!("irm {} | iex", ps_quote(url)),
    };
    [
        "$ErrorActionPreference = 'Stop'".to_string(),
        "$global:LASTEXITCODE = $null".to_string(),
        format!("Write-Host {}", ps_quote("Puppet Master is running this for you:")),
        format!("Write-Host {}", ps_quote(&format!("  {}", step.command))),
        "Write-Host ''".to_string(),
        format!(
            "try {{ {call}; $pmCode = if ($null -ne $LASTEXITCODE) {{ $LASTEXITCODE }} else {{ 0 }} }} catch {{ Write-Host $_; $pmCode = 1 }}"
        ),
        "Write-Host ''".to_string(),
        // Split so the marker text never appears in the command line itself.
        "Write-Host (('PM-INSTALL-' + 'EXIT ') + $pmCode)".to_string(),
    ]
    .join("; ")
}

/// The exit code printed by a finished install pane, from its terminal text. `None` while it runs.
pub fn parse_done(terminal_text: &str) -> Option<i32> {
    terminal_text
        .lines()
        .rev()
        .map(str::trim)
        .find_map(|line| line.strip_prefix(DONE_MARKER))
        .and_then(|rest| rest.trim().parse::<i32>().ok())
}

/// Plain words for an exit code, when one is known. Success is decided by the health check, not here.
pub fn explain_exit(code: i32) -> Option<&'static str> {
    match code {
        0 => None,
        // ERROR_CANCELLED: the Windows "make changes?" prompt was declined.
        1223 => Some("The install was cancelled in the Windows prompt."),
        _ => Some("The install did not finish."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_install::plan::Method;

    fn argv_step(exe: &str, args: &[&str]) -> Step {
        Step {
            tool: None,
            label: "X".into(),
            method: Method::Winget,
            action: Action::Argv { exe: exe.into(), args: args.iter().map(|s| s.to_string()).collect() },
            command: format!("{exe} {}", args.join(" ")),
            source: "https://example.com/x".into(),
            high_risk: false,
            note: String::new(),
        }
    }

    #[test]
    fn every_token_is_a_quoted_string() {
        let s = powershell_script(&argv_step("winget", &["install", "--id", "A.B", "-e"]));
        assert!(s.contains("& 'winget' 'install' '--id' 'A.B' '-e'"), "{s}");
    }

    #[test]
    fn a_hostile_token_cannot_end_its_string() {
        // The validator would reject this value; the quoting must hold even if it were let through.
        let s = powershell_script(&argv_step("winget", &["install", "x'; calc; '"]));
        assert!(s.contains("'x''; calc; '''"), "{s}");
        // After removing the doubled quotes no bare quote can close the token early.
        let call = s.split("try { ").nth(1).unwrap();
        let token = call.split("'install' ").nth(1).unwrap().split("; $pmCode").next().unwrap();
        assert_eq!(token, "'x''; calc; '''");
    }

    #[test]
    fn a_uv_install_is_quoted_token_by_token_and_shown_as_typed() {
        let step = Step {
            method: Method::Uv,
            ..argv_step("uv", &["tool", "install", "--python", "3.12", "aider-chat"])
        };
        let s = powershell_script(&step);
        assert!(s.contains("& 'uv' 'tool' 'install' '--python' '3.12' 'aider-chat'"), "{s}");
        assert!(s.contains("Write-Host '  uv tool install --python 3.12 aider-chat'"), "{s}");
    }

    #[test]
    fn script_method_downloads_with_irm_and_pipes_to_iex() {
        let step = Step {
            action: Action::Script { url: "https://claude.ai/install.ps1".into() },
            command: "irm 'https://claude.ai/install.ps1' | iex".into(),
            method: Method::Script,
            ..argv_step("x", &[])
        };
        let s = powershell_script(&step);
        assert!(s.contains("try { irm 'https://claude.ai/install.ps1' | iex;"), "{s}");
    }

    #[test]
    fn the_shown_command_is_printed_in_the_window() {
        let s = powershell_script(&argv_step("winget", &["install", "--id", "A.B"]));
        assert!(s.contains("Write-Host '  winget install --id A.B'"), "{s}");
    }

    #[test]
    fn done_marker_is_read_from_the_terminal_text() {
        assert_eq!(parse_done("Installing...\r\nPM-INSTALL-EXIT 0\r\nPS C:\\>"), Some(0));
        assert_eq!(parse_done("a\nPM-INSTALL-EXIT 1223\n"), Some(1223));
        assert_eq!(parse_done("a\nPM-INSTALL-EXIT -1978335189\n"), Some(-1978335189));
        assert_eq!(parse_done("still installing"), None);
        // The marker text inside the command line must not count as done.
        assert_eq!(parse_done("PS> Write-Host 'PM-INSTALL-' + 'EXIT '"), None);
        assert_eq!(parse_done("PM-INSTALL-EXIT soon"), None);
    }

    #[test]
    fn the_command_line_never_contains_the_marker_text() {
        let s = powershell_script(&argv_step("winget", &["install"]));
        assert!(!s.contains(DONE_MARKER), "{s}");
    }

    #[test]
    fn cancelled_prompt_has_its_own_sentence() {
        assert_eq!(explain_exit(0), None);
        assert!(explain_exit(1223).unwrap().contains("cancelled"));
        assert!(explain_exit(5).unwrap().contains("did not finish"));
    }
}
