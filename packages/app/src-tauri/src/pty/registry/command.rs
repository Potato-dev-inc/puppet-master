//! Builds the `CommandBuilder` for each agent type, including the Windows cmd/PowerShell shims.

use super::*;

/// Build a CommandBuilder for the given agent, using the given cwd.
/// On Windows, `.cmd`/`.bat` shims must run under `cmd.exe /C`.
pub(super) fn build_command(agent: AgentType, cwd: &str, extra_args: &[String]) -> CommandBuilder {
    let (cmd, base_args) = resolve_command(agent);

    #[cfg(windows)]
    {
        if windows_powershell_host_agent(agent) {
            let mut cmd_builder = CommandBuilder::new("powershell.exe");
            cmd_builder.arg("-NoLogo");
            cmd_builder.arg("-NoExit");
            cmd_builder.arg("-Command");
            let invocation = if agent == AgentType::CursorAgent {
                powershell_cursor_agent_command(cmd, &base_args, extra_args)
            } else {
                powershell_agent_command(cmd, &base_args, extra_args)
            };
            cmd_builder.arg(invocation);
            cmd_builder.cwd(cwd);
            apply_common_pty_env(&mut cmd_builder);
            return cmd_builder;
        }

        let is_cmd_shim = cmd.ends_with(".cmd") || cmd.ends_with(".bat");
        let mut cmd_builder = if is_cmd_shim {
            let mut b = CommandBuilder::new("cmd.exe");
            b.arg("/C");
            b.arg(cmd);
            b
        } else {
            CommandBuilder::new(cmd)
        };
        for a in base_args {
            cmd_builder.arg(a);
        }
        for a in extra_args {
            cmd_builder.arg(a);
        }
        cmd_builder.cwd(cwd);
        apply_common_pty_env(&mut cmd_builder);
        return cmd_builder;
    }

    #[cfg(not(windows))]
    {
        let mut cmd_builder = CommandBuilder::new(cmd);
        for a in base_args {
            cmd_builder.arg(a);
        }
        for a in extra_args {
            cmd_builder.arg(a);
        }
        cmd_builder.cwd(cwd);
        apply_common_pty_env(&mut cmd_builder);
        cmd_builder
    }
}

#[cfg(windows)]
pub(super) fn windows_powershell_host_agent(agent: AgentType) -> bool {
    matches!(
        agent,
        AgentType::Claude | AgentType::Codex | AgentType::Opencode | AgentType::CursorAgent
    )
}

pub(super) fn apply_common_pty_env(cmd_builder: &mut CommandBuilder) {
    cmd_builder.env("PATH", crate::shell_env::path_for_spawn());
    #[cfg(windows)]
    cmd_builder.env("Path", crate::shell_env::path_for_spawn());
    cmd_builder.env("TERM", "xterm-256color");
    cmd_builder.env("COLORTERM", "truecolor");
}

#[cfg(windows)]
pub(super) fn powershell_agent_command(cmd: &str, base_args: &[&str], extra_args: &[String]) -> String {
    let args = base_args
        .iter()
        .map(|arg| powershell_single_quote(arg))
        .chain(extra_args.iter().map(|arg| powershell_single_quote(arg)))
        .collect::<Vec<_>>();
    if args.is_empty() {
        format!("& {}", powershell_single_quote(cmd))
    } else {
        format!("& {} {}", powershell_single_quote(cmd), args.join(" "))
    }
}

#[cfg(windows)]
pub(super) fn powershell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(windows)]
pub(super) fn powershell_cursor_agent_command(cmd: &str, base_args: &[&str], extra_args: &[String]) -> String {
    let args = base_args
        .iter()
        .map(|arg| powershell_single_quote(arg))
        .chain(extra_args.iter().map(|arg| powershell_single_quote(arg)))
        .collect::<Vec<_>>();
    format!(
        "& (Get-Command -Name {} -CommandType ExternalScript -ErrorAction Stop).Source {}",
        powershell_single_quote(cmd),
        args.join(" ")
    )
}

#[derive(Debug, Clone, Deserialize)]
pub struct SpawnPaneArgs {
    pub agent_type: String,
    pub cwd: Option<String>,
    pub cols: Option<u16>,
    pub rows: Option<u16>,
    pub extra_args: Option<Vec<String>>,
    pub pane_id: Option<String>,
    /// Start the pane with enforced read-only permissions (opencode_native only).
    #[serde(default)]
    pub read_only: bool,
}
