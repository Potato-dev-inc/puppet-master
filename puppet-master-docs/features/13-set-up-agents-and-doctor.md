# Set up agents and Doctor

**Status:** Works today · The list and the checks work. The in-app **Install** button is tested only by automated tests (it was not clicked on a real PC).

## What it is
"Set up agents" is a screen that lists every agent PM knows. It is also called the Doctor. For each agent it shows if the program is installed, which version it is, and if you are signed in. If an agent is missing, the **Install** button shows the exact command first, then runs it in a window you can watch.

## Why it matters
A new user often opens an empty cockpit and does not know why an agent will not start. Many people also do not know commands like `npm i -g`. The Doctor tells you what is wrong in one sentence and gives you one next step.

## How to use it
1. Open the screen in any of these ways: **Set up agents** at the bottom of the sidebar; Ctrl+K and type "Set up agents"; or **Set up your agents** on the empty Inbox. On a first start, the welcome flow does the same job.
2. Read the three groups: **Needs you**, **Ready** and **Not installed**. Each row has a state: Ready, Needs login, Problem, Not installed or Not tested.
3. For a missing agent, press **Install**. A sheet opens and shows the command, where it comes from, and what to expect. Nothing runs yet.
4. Press **Run** to go on, or **Cancel**. **Copy command** copies it so you can run it yourself. **More > Copy install command** does the same from the row.
5. A PowerShell window opens. Watch it. Windows may ask if the installer can make changes. That is expected. Press **Show window** if the window is behind other windows.
6. When the command ends, PM checks again. If it worked you see "Installed. Sign in next."
7. Press **Sign in**. The agent opens in a visible pane. Sign in there. PM never sees your password.
8. Press **Test** to prove the agent can answer. It asks one tiny read-only question ("Reply with exactly OK"). It uses a few tokens, so it only runs when you press the button.
9. **Check again** (top of the screen) reads everything once more.

## How it works (simple)
- **Check.** PM asks each program for its version, with a time limit. One slow program cannot delay the others. This costs no tokens.
- **States.** "Not tested" means the program is installed and gives a version, but PM has not asked it a question. "Needs login" is shown when the agent's error text contains login words. Any other failure shows "Problem" with a short reason.
- **Install order.** For each agent PM builds a plan from a small data file. It tries winget first (the Windows package tool), then npm (the Node.js package tool), and a vendor script last. If the chosen way needs Node.js or Git and you do not have it, PM first offers to install that.
- **Safe by design.** The plan is built twice: once to show you, and again when you press Run. If the command text is different, the run is refused. A vendor script is marked high risk. You must tick "This downloads a script from the internet and runs it. I trust <site>." before **Run** works.
- **After the install.** PM adds new folders to its own PATH (the list of places Windows looks for programs). It never removes or reorders anything. Then it checks the agent again.

| Agent | Install routes, in order |
|---|---|
| Claude Code | winget `Anthropic.ClaudeCode`, npm `@anthropic-ai/claude-code`, vendor script |
| Codex | winget `OpenAI.Codex`, npm `@openai/codex`, vendor script |
| Cursor Agent | vendor script only (it is not on winget or npm) |
| OpenCode | winget `SST.opencode`, npm `opencode-ai` |
| Kilo Code | no install plan. The row shows `npm install -g @kilocode/cli` to copy. |

## Limits and what is not done yet
- Windows only. Other systems show only the older hint text.
- A signed-out agent shows "Not tested" until you press **Test**. The quick check cannot know without spending a token.
- The Test cannot tell "signed out" from "no network" unless the agent says so.
- If an agent fails with unfamiliar words, you see "Problem" and its first line.
- No checksum method exists for downloads, because vendors publish moving "latest" links. winget checks its own installer hash.
- If a program was installed but PM cannot find it, close PM and open it again.
- The login hint for Cursor Agent (`agent login`) was written from memory and not checked against the vendor docs.
- Real `.cmd` version output on Windows was tested only with fake programs.

## For developers
- `src-tauri/src/agent_health/mod.rs`: `collect_probes`, `check_all`, `test_agent`. Routes: `GET /agents/health`, `POST /agents/health/test {"agent": id}`.
- `src-tauri/src/agent_install/`: `plan.rs` (pure), `command.rs`, `path_refresh.rs`, `tools.toml` (Node.js, Git). `tui/<agent>/install.toml` holds each agent's recipe.
- Install is Tauri commands only (`commands/agent_install.rs`). No bridge route and no MCP tool exist, so an agent cannot start an install.
- Profile methods: `version_command`, `install_hint`, `login_hint`, `install_spec`. TOML agents use `[install.windows]`, `binary.install_hint`, `binary.login_hint`.
- UI: `components/cockpit/AgentDoctor.tsx`, `InstallSheet.tsx`, `lib/agent-health.ts`, `lib/agent-install.ts`. First-run flow: `src-tauri/src/first_run/`.

## Related
- [First-run welcome](08-first-run-welcome.md)
- [Supported agents](12-supported-agents.md)
- [Trust and start-up screens](15-trust-and-startup-dialogs.md)
- [Tested versions and canary](18-tested-versions-canary.md)
- [Hook Lane](19-hook-lane.md) (the Doctor row also shows a "Live updates" line)
