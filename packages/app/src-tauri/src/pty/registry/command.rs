//! Builds the `CommandBuilder` for each agent type, including the Windows cmd/PowerShell shims.

use super::*;

/// Build a CommandBuilder for the given agent, using the given cwd.
/// On Windows, `.cmd`/`.bat` shims must run under `cmd.exe /C`.
pub(super) fn build_command(
    agent: AgentType,
    cwd: &str,
    extra_args: &[String],
    env: &crate::tui::LaunchEnv,
) -> CommandBuilder {
    if matches!(agent, AgentType::Cursor | AgentType::CursorAgent) {
        if let Some(cli) = crate::tui::cursor::binary::installed() {
            let (program, args) = crate::tui::cursor::binary::visible_launch(&cli, extra_args);
            let mut cmd_builder = CommandBuilder::new(program);
            cmd_builder.args(args);
            cmd_builder.cwd(cwd);
            apply_common_pty_env(&mut cmd_builder);
            apply_launch_env(&mut cmd_builder, &env.set, &env.remove);
            return cmd_builder;
        }
    }
    let (cmd, base_args) = resolve_command(agent);

    #[cfg(windows)]
    {
        if windows_powershell_host_agent(agent) {
            let invocation = if agent == AgentType::CursorAgent {
                powershell_cursor_agent_command(cmd, &base_args, extra_args)
            } else {
                powershell_agent_command(cmd, &base_args, extra_args)
            };
            return powershell_host_command(invocation, cwd, env);
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
            apply_launch_env(&mut cmd_builder, &env.set, &env.remove);
        cmd_builder
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
            apply_launch_env(&mut cmd_builder, &env.set, &env.remove);
        cmd_builder
    }
}

/// Build a CommandBuilder from a profile's `spawn_command`: the program runs directly with its
/// arguments (no shell, no AgentType), in the spec's cwd, with the common PTY environment.
pub(super) fn build_launch_spec_command(
    spec: &crate::tui::LaunchSpec,
    env: &crate::tui::LaunchEnv,
) -> CommandBuilder {
    let mut cmd_builder = CommandBuilder::new(&spec.program);
    for a in &spec.args {
        cmd_builder.arg(a);
    }
    cmd_builder.cwd(&spec.cwd);
    apply_common_pty_env(&mut cmd_builder);
    apply_launch_env(&mut cmd_builder, &spec.env, &spec.env_remove);
    apply_launch_env(&mut cmd_builder, &env.set, &env.remove);
    cmd_builder
}

/// Set then remove variables on the child (the TOML `[env]` table, or an account's env). The
/// PowerShell host wrapper inherits them, so the agent it starts sees them too.
pub(super) fn apply_launch_env(cmd_builder: &mut CommandBuilder, set: &[(String, String)], remove: &[String]) {
    for (key, value) in set {
        cmd_builder.env(key, value);
    }
    for key in remove {
        cmd_builder.env_remove(key);
    }
}

/// The host shell an agent pane runs its agent under: `powershell -NoExit -Command <invocation>`.
/// `-NoExit` stays (user panes keep their shell after the agent exits); whether the agent is still
/// running is read from the process tree (`pty::agent_proc`) and the screen, never assumed.
#[cfg(windows)]
pub(super) fn powershell_host_command(
    invocation: String,
    cwd: &str,
    env: &crate::tui::LaunchEnv,
) -> CommandBuilder {
    let mut cmd_builder = CommandBuilder::new("powershell.exe");
    cmd_builder.arg("-NoLogo");
    cmd_builder.arg("-NoExit");
    cmd_builder.arg("-Command");
    cmd_builder.arg(invocation);
    cmd_builder.cwd(cwd);
    apply_common_pty_env(&mut cmd_builder);
    apply_launch_env(&mut cmd_builder, &env.set, &env.remove);
    cmd_builder
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
    // The pane is a real terminal that renders ANSI colors. Variables inherited from whatever
    // launched the app (`npm run dev > log` makes npm export NO_COLOR=1 / COLOR=0) switch
    // Claude Code and other TUIs to monochrome output, so drop them.
    for name in COLOR_DISABLING_ENV {
        cmd_builder.env_remove(name);
    }
}

/// Environment variables that tell a program not to emit color; never forwarded to a pane PTY.
pub(super) const COLOR_DISABLING_ENV: &[&str] = &["NO_COLOR", "NODE_DISABLE_COLORS", "COLOR"];

#[cfg(windows)]
pub(super) fn powershell_agent_command(
    cmd: &str,
    base_args: &[&str],
    extra_args: &[String],
) -> String {
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
pub(super) fn powershell_cursor_agent_command(
    cmd: &str,
    base_args: &[&str],
    extra_args: &[String],
) -> String {
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
    /// Account (id) to run the agent under; see `crate::accounts`.
    #[serde(default)]
    pub account: Option<String>,
    /// Env guard for the pane (`pty::env_guard`, e.g. `no_push`). Never read from JSON: only Rust
    /// code inside the app sets it. An unknown name refuses the spawn.
    #[serde(skip)]
    pub guard_env: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_env_in_spawn_pane_json_is_ignored() {
        let args: SpawnPaneArgs =
            serde_json::from_value(serde_json::json!({"agent_type": "x", "guard_env": "no_push"})).unwrap();
        assert_eq!(args.guard_env, None);
    }
}
