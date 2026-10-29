# Linux Inside (run agents in Linux)

Status: **experimental**. Phase 1 only, Windows desktop only. The code has unit tests. The live check (a real WSL 2
distro) has not been run. The end-to-end test with a fake `wsl.exe` (WSL-17) is **in progress** on branch
`feat/wsl-e2e`; it is not on this branch. The file map is in `REPO_MAP.md` section 3.14.

## What it does

- Settings > Agents > **Run agents in Linux (WSL)**: one `Runs on` select per agent. The choice is `Windows` (the
  default) or `Linux · <distro>`.
- Agents that can run in Linux: **Claude Code**, **Codex** and **OpenCode** (the TUI). Other agents do not show a choice.
- A Linux pane runs the agent inside the distro, in the same folder on a Windows drive. `D:\work\shop` is
  `/mnt/d/work/shop` inside Linux. The pane shows a chip `Linux · <distro>`.
- The Chat tab reads the agent's session files from the Linux home. Question cards come from the screen, as today.
- Undo and checkpoints run on Windows, on the Windows folder. The agent sees the restored files through `/mnt/<d>`.
- Close and Keep going stop the agent's processes inside the distro. Only processes that this pane started are stopped.

## Decisions

1. Windows stays the default. Nothing moves to Linux unless the person picks it in Settings.
2. WSL 2 distros only. `docker-desktop*` and `rancher-desktop*` distros are hidden.
3. Only folders on a Windows drive. Folders inside the distro are refused for now.
4. Nothing from PM's own environment crosses into Linux, except the pane id and the login PATH. Saved logins
   (accounts), env guards and separate copies are refused for a Linux pane.
5. PM never installs WSL or a distro, never runs `wsl --shutdown` or `--terminate`, and never edits `.wslconfig`
   or `wsl.conf`. It shows the official command with a Copy button.
6. Opening Settings only lists the distros; it never starts one. A distro starts only when the person clicks
   **Check**, or when a Linux pane starts.
7. The place is changed only in the desktop window. The bridge, MCP tools and the phone cannot change it.
8. Linux is not a sandbox. Linux programs can read the Windows drives. The Settings footer says so.

## What works in this build

| Feature | Where |
|---|---|
| Distro list (`wsl --list --verbose`, cached 30 s), status and the install or add-distro command as text | `wsl/probe.rs`, `wsl/parse.rs`, `commands/wsl.rs` |
| Check one distro (boots it, up to 25 s): git, interop, networking, and whether each agent is found | `wsl/probe.rs` (`check`), `wsl/parse.rs` (`PROBE_SCRIPT`) |
| Per-agent place setting in `<app data>/wsl.json` | `wsl/settings.rs` |
| Pane spawn in Linux (`wsl.exe --exec`, fixed launcher, argv kept apart) | `wsl/launch.rs`, `pty/registry/linux.rs`, `pty/registry/spawn.rs` |
| Pane chip `Linux · <distro>` | `components/cockpit/PaneView.tsx` |
| Chat facts for a Linux pane (Linux cwd and home) | `wsl/chat.rs`, `commands/session_chat.rs` |
| Stop on close or Keep going (only the marked processes) | `wsl/stop.rs`, `wsl/live.rs` |
| Run check: a one-task run in Linux runs visible instead, with a `linux` fallback record | `agent_runs/headless/entry.rs` |
| Startup: 20 s more for the first ready screen of a Linux pane | `wsl/model.rs`, `bridge/dispatch.rs` |
| Agents in Linux are never warmed in standby | `standby/host.rs` |

## What does not work yet

- Folders inside the distro (`\\wsl.localhost\...`): refused.
- Separate copies (`isolation: worktree`), one-task runs as headless processes, and saved logins for a Linux pane.
- Env guards (for example Assign an issue).
- Hooks in a Linux pane: none. Hook Lane is off there.
- Cursor Agent, `opencode_native`, custom agents (TOML or the wizard) in Linux.
- PM's MCP tools inside a Linux agent, and `run_agent` with a `place` input.
- A per-pane switch, a phone chip, a doctor row, and old chats from a distro's home.
- WSL 1 distros.
- Companion (helper) panes always run on Windows.
- The `spawn_pane_in_linux` command exists, but no screen calls it yet.

## Performance (expected, not measured yet)

| Thing | Expected |
|---|---|
| `wsl --list --verbose` | 50-300 ms; starts no distro |
| Check (probe) when the distro is warm / cold | 0.3-1 s / 2-10 s; cached 10 minutes |
| First screen of a Linux pane | 0.2-0.5 s slower than Windows when warm; 2-10 s more when cold |
| Agent work in `/mnt/<d>` | Many small files (npm install, `git status` in a big repo) can be 3-10x slower than inside Linux's own disk |
| Stop | One `wsl.exe` call, under 3 s |

## WSL commands

PM never installs WSL, adds a distro or changes a distro's version. Settings shows the first two commands as text
with a Copy button, and you run them yourself in PowerShell. PM runs only the read-only `wsl.exe` calls below and its
own fixed scripts inside the distro (probe, stop).

- `wsl --install`: installs WSL and a default Linux. Run as administrator. (Shown with Copy.)
- `wsl --install -d Ubuntu`: adds Ubuntu to a PC that has WSL but no distro. (Shown with Copy.)
- `wsl --set-version <distro> 2`: changes a WSL 1 distro to WSL 2. You run it; PM only tells you that the distro uses WSL 1.
- `wsl --list --verbose`: lists the distros, their state and their WSL version. PM runs it; it starts nothing.
- `wsl --status`: PM runs it only when the list fails, to tell "not installed" from "no distro".

## Privacy

- Linux agents get only the pane id and the login PATH from PM's environment. Names that hold Windows paths are refused.
- The Linux home and the Windows folder names are shown as `~` in chat, search and exports (`session_chat/home_path.rs`).
- PM never copies, reads or writes sign-in files on either side. The agent shows its own sign-in screen.
- PM never answers a trust, permission or sign-in question in a Linux pane.

## Tests

- Rust: `cargo test --lib wsl`, `cargo test --lib pty::registry::linux`, `cargo test --lib commands::wsl`.
- The real stop check is ignored by default. It needs a WSL 2 distro: `PM_WSL_LIVE_DISTRO=<distro> cargo test --lib wsl::stop::live_tests -- --ignored`.
- UI: `npx vitest run src/lib/wsl src/components/settings/sections/LinuxRows src/components/cockpit/PaneView.place`.

## Known gaps

- No live run on a real distro yet. Speeds in the table above are targets, not measurements.
- The end-to-end test with the fake `wsl.exe` (WSL-17) is in progress on `feat/wsl-e2e`.
- Broken-status text: PM shows one fixed sentence (`WSL did not answer in time.`) and never the raw output of `wsl.exe`.
  A failure of both `--list` and `--status` (not a timeout) is shown as "not installed".
- The launch-command refusal for an agent with no Linux launch can say `WSL_START_FAILED`; in practice the spawn
  refuses first with `WSL_AGENT_NOT_SUPPORTED`.
