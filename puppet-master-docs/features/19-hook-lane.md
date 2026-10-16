# Hook Lane (live updates from agents)

**Status:** Partly built · "Watch only" mode. Claude Code is on by default. Codex is off until you turn it on. Nothing acts on the events yet.

## What it is
Claude Code and Codex can run a small command at fixed moments: a session starts, you send a prompt, a tool asks for permission, a file is edited, a turn ends. These small commands are called hooks. Hook Lane makes each agent tell PM about these moments directly. PM writes them down next to what its screen reader decided, so the two can be compared. In the app this is called **Live updates**.

## Why it matters
PM normally learns what an agent does by reading its screen. That is a guess and can be wrong after an agent update. Hooks are exact signals from the agent. The comparison shows where the screen reader and the agent disagree. That data will help decide if PM may use hooks as real evidence in the future. That is a later decision.

## How to use it
1. Open **Settings > General** and go to **Labs**. Find **Live updates**. (**Settings > Agents** also has a link that jumps there.)
2. Choose **Watch only** (default) or **Off**.
3. For Codex, turn on **Live updates from Codex**. This switch is off by default. It appears only when Codex is installed.
4. Open Codex in a project. Codex shows a "Hooks need review" screen. In that Codex pane, type `/hooks` and trust the Puppet Master entries. You do this once for each project folder.
5. Open **Set up agents** to check. Each agent row can show a line such as: "Live updates: Connected. Claude Code tells Puppet Master when it starts, asks and finishes. Watch only: Puppet Master still decides from the screen."

Other lines you may see: "Turns on the next time you open Claude Code." / "Waiting for you to trust this folder in Claude Code." / "Needs your OK in Codex." / "Untested Codex version. Live updates stay watch only." / "This project shares its Codex settings in git, so Puppet Master doesn't change them."

## How it works (simple)
- **One fixed command.** PM registers `puppet-master-mcp hook <agent>`. The agent runs it at each moment. The command sends the event to PM over the local connection with a secret token for that pane. It prints nothing and always exits with success, even if PM is not running. It stops after 300 ms.
- **Claude Code.** PM starts it with a `--settings` file kept in PM's own data folder. No file is written in your project or in your Claude settings. Claude's hooks start only after you accept the folder trust.
- **Codex.** PM adds managed entries to `.codex/hooks.json` in the project and keeps the file out of git. It never writes the file when git already tracks it.
- **Events recorded:**

| Moment | Claude Code | Codex |
|---|---|---|
| Session starts | yes | yes (with the first prompt, not at start-up) |
| You send a prompt | yes | yes |
| A tool asks for permission | yes | yes |
| A file is edited | yes | yes (patches) |
| A turn ends | yes | yes |
| A turn fails | yes | no |
| A helper agent finishes | yes | no |
| The session ends | yes | no |
| You interrupt (Esc) | no | yes |

- **Safe by design.** Hooks never type, answer, approve or unlock anything. The code of Hook Lane has no path to those parts, and a test checks it. The hook token gets "forbidden" on every other route, and a paired phone is refused on the hook route.
- **Codex review screen.** If Codex shows "Hooks need review", PM can press Esc ("continue without trusting") for that start. It never presses "Trust all". Trusting stays your step.

## Limits and what is not done yet
- Watch only. The "evidence" mode exists in the settings file but behaves exactly like Watch only. No use of hook events for decisions is built.
- You cannot see the recorded comparison in the app. It is data for developers.
- Hook Lane has narrower tested ranges than the agent check: Claude Code 2.1.299 to 2.1.300, and Codex 0.162.1 only. Outside them the Doctor line says the version is untested. See [Tested versions and canary](18-tested-versions-canary.md).
- Codex hooks are opt-in because they write a file into each project. If they were on by default, plain `codex` in your own terminal would show "Hooks need review" in every project, and each separate copy would ask again.
- Cursor Agent, OpenCode and Kilo have no hooks. PM reads their screen only, and the Doctor shows no "Live updates" line for them. The same is true when Live updates are off.
- Checked live: events were recorded in shadow mode. Real Codex review and trust behaviour was seen on 0.162.1 only.

## For developers
- `src-tauri/src/agent_hooks/`: `route.rs` (`POST /hooks/event`), `shadow.rs`, `ledger.rs`, `token.rs`, `ingest.rs`, `health.rs` (Doctor sentences), `install/` (`launch.rs`, `project.rs`, `stable_exe.rs`).
- Emitter: `mcp_shim/hook.rs` (the `hook` subcommand of `puppet-master-mcp`). Tests: `tests/hook_emitter.rs`, `agent_hooks/tests/golden/codex-hooks.json` (every byte is pinned).
- Per-agent facts live in `tui/claude/hooks.toml` and `tui/codex/hooks.toml` (`[hooks]`, `default_mode = "off"` for Codex).
- Settings keys: `agent_hooks_mode` (`off`, `shadow`, `evidence`; default `shadow`) and `agent_hooks_agents` (per-agent choice).
- UI: `components/settings/sections/AgentsSection.tsx` (`LiveUpdates`), `components/cockpit/AgentDoctor.tsx`.
- Developer doc: `docs/features/hook-lane.md`. Design: `D:\pm-night\DESIGN-HOOK-LANE.md`.

## Related
- [Tested versions and canary](18-tested-versions-canary.md)
- [Trust and start-up screens](15-trust-and-startup-dialogs.md)
- [Set up agents and Doctor](13-set-up-agents-and-doctor.md)
- [Supported agents](12-supported-agents.md)
