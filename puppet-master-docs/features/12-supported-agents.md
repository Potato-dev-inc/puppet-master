# Supported agents

**Status:** Works today · Claude Code and Codex are tested live. Cursor Agent, OpenCode and Kilo Code are less tested.

## What it is
Puppet Master (PM) does not have its own AI. It starts the real program of each agent in a real terminal window and watches its screen. You use your own logins. Five agents work today: Claude Code, Codex, Cursor Agent, OpenCode and Kilo Code. The first four are built in. Kilo Code is added with a small settings file.

## Why it matters
You keep the tools you already like. PM adds approval cards, receipts, undo and a phone view around them. You do not need to learn a new agent. If you add a new agent later, it fits into the same window.

## How to use it
1. Open **Set up agents** (bottom of the sidebar, or press Ctrl+K and type "Set up agents"). See [Set up agents and Doctor](13-set-up-agents-and-doctor.md). It shows which agents are on your PC.
2. Press **+** (New agent) at the top of the sidebar.
3. Pick an agent and a project folder.
4. The agent opens in its own pane. Work with it as usual, or let another agent start it through MCP (a standard way for agents to use tools).

To add Kilo Code, or your own agent, see "How to add an agent" below.

## How well each agent is tested

| Agent | How it is added | How well tested (0.2.1) |
|---|---|---|
| Claude Code | Built in | Live runs with real Claude Code: trust card, tool approval, follow-ups. Screens recorded with versions 2.1.299 and 2.1.300. |
| Codex | Built in | Live runs with real Codex: trust card, update menu, follow-ups. Screens recorded with version 0.162.1. |
| Cursor Agent | Built in (the `agent` command) | Tested mostly with recorded screens (CLI version 2026.10.01). Live checks were limited. In an earlier live test, its own trust screen stopped a run. I found no 0.2.1 live check. |
| OpenCode | Built in. Two kinds: a terminal pane, and "OpenCode (API)", which talks to OpenCode over its web interface | Less tested. Only one recorded screen (version 1.19.0). Its readiness check is a weaker kind (see [Typing permit](16-typing-permit.md)). |
| Kilo Code | A settings file (`kilo.toml`), not built in | Tested live early in the project with Kilo 7.8.8. I found no 0.2.1 live check. |

The 0.2.1 release notes say the same: Cursor, Kilo and OpenCode are less tested than Claude Code and Codex.

## How it works (simple)
- **Profile.** For each agent, PM has a "TUI profile". TUI means text user interface, the screen an agent draws in a terminal. A profile is a piece of code that knows one agent's screens: how to start it, when its input box is ready, what its approval and trust screens look like, and how to install it.
- **One rule.** The shared code never checks an agent's name. It asks the profile. So a fix for one agent cannot break another.
- **Built-in profiles.** Nine: `claude`, `codex`, `opencode`, `opencode_native` (OpenCode over its web interface), `cursor_agent`, `cursor` (the Cursor editor, which cannot be started as an agent), and three plain shells (Command Prompt, PowerShell, Bash).
- **Tests.** Each profile is tested against real screens that were recorded from the real program. A shared set of rules (the "conformance" tests) runs for every profile.
- **TOML agents.** TOML is a simple settings-file format. A TOML file can describe an agent that is started with a list of arguments. PM reads it and makes a profile from it. No Rust code and no new build is needed.
- **Approval.** A new or changed TOML file stays off until you review it and press Approve. The dialog shows the exact command. If you edit the file later, PM asks again before the next run. Runs that already started are not changed. A definition can never replace a built-in agent.

## How to add an agent
**Without code (a TOML file):**
1. Write the file, for example `foo.toml`. The format is in `docs/specs/agent-definitions/DEFINITION-FORMAT.md`. A worked example is `kilo.toml`.
2. Put it in `%APPDATA%\com.puppetmaster.app\agents\`, or use **Settings > Agents > Your own agents > Import**. Import takes a `.toml` file, checks it, and never overwrites an existing file.
3. In the same place, press **Check with fixtures** if you have recorded screens, then **Review and approve**, then **Approve**.
4. The agent now appears in the **+** menu and in **Set up agents**.

Kilo Code: the sample file is `packages/app/src-tauri/tests/agent-definitions/kilo.toml` in the source. The installer does not include it (no definitions are bundled). A second sample, `kiro.toml`, is next to it. You also need the Kilo program itself: `npm install -g @kilocode/cli`.

There is also **Add as generic agent** (type a command name). It is experimental. It runs the command in a visible pane and guesses from the screen.

**With code (developers):** copy the steps in `packages/app/src-tauri/src/tui/README.md`: write a profile, register it in `tui/registry.rs`, add real recorded screens, run `cargo test --lib tui::`.

## Limits and what is not done yet
- Only Claude Code and Codex were checked live in 0.2.1 (real runs). The others rest on recorded screens and tests.
- Cursor Agent: its headless runs cannot run shell commands. Use a visible pane when the task must run commands.
- A TOML agent's result in a visible pane is read from the screen, so it is a best guess and is flagged as such.
- Accounts (several logins) only work for Claude Code and Codex. See [Accounts and keys](20-accounts-and-keys.md).
- No agent definitions are bundled. Gemini CLI has an install recipe idea but no profile.
- Windows is the supported system.

## For developers
- `src-tauri/src/tui/` (one folder per agent), `tui/profile.rs` (the `TuiProfile` trait), `tui/registry.rs` (runtime registry).
- `tui/declarative/` (TOML agents: loader, approvals, fingerprints, hot reload), `tui/conformance.rs` (shared rules).
- Tauri commands and bridge routes for definitions: `commands/agents.rs`, `POST /agents/definitions/validate`.
- Agent folder: `<app data>/agents/`. Project folder (needs your consent per path): `<project>/.puppet-master/agents`.
- Frontend: `components/settings/blocks/AgentDefinitions.tsx`, `lib/agentRegistry.ts`.

## Related
- [Set up agents and Doctor](13-set-up-agents-and-doctor.md)
- [Tested versions and canary](18-tested-versions-canary.md)
- [Accounts and keys](20-accounts-and-keys.md)
- [Trust and start-up screens](15-trust-and-startup-dialogs.md)
