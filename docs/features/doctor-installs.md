# The doctor installs (Set up agents > Install)

## What and why
Personas B and C (vibecoders, non-technical people) hit a wall at "npm i -g ...". Set up agents now has an
**Install** button on every missing agent. It shows the exact command and where it comes from, asks, runs it in a
**visible** PowerShell window, refreshes PATH for Puppet Master's own processes, and checks the agent again.
**Sign in** then opens the agent in a visible pane; Puppet Master never handles a password.

Agent-agnostic: the generic code never matches an agent name (a test greps for it). Each agent declares how it
installs as data.

## Design
- `tui/install.rs`: `InstallSpec` (`[install.windows]`): `source`, `order`, `winget`, `npm`, `script_url`,
  `requires`, `note`. Only closed, validated fields (winget id, npm name, https URL), never a command line.
- Code profiles ship `tui/<agent>/install.toml` (`include_str!`) and return it from `TuiProfile::install_spec`.
  TOML definitions add `[install.windows]` (validated in `declarative/validate.rs`, so shell text is rejected).
- `agent_install/plan.rs` (pure): spec + what the PC has (`Tools`: winget, Node, Git) -> ordered `Step`s.
  winget, then npm, in the spec's order; a missing Node.js or Git step is added only when the chosen method needs
  it (`agent_install/tools.toml`); the vendor `irm | iex` script is last and marked high risk.
- `agent_install/command.rs`: the `-Command` text for the pane. Every token is quoted as a PowerShell
  single-quoted string. A done marker (`PM-INSTALL-EXIT <code>`) is printed at the end.
- `agent_install/path_refresh.rs`: reads `HKLM` and `HKCU` PATH with `reg query`, expands `%VAR%`, and ADDS the
  missing folders to this process's PATH (never reorders or removes). Panes spawned afterwards inherit it.
- `agent_install/mod.rs`: `plan`, `start`, `status`, `finish`. `start` rebuilds the plan itself and refuses unless
  the command text equals what the person saw (`INSTALL_COMMAND_CHANGED`) and, for a script, unless a second
  confirmation was given (`INSTALL_NEEDS_CONFIRMATION`).
- Tauri commands only (`commands/agent_install.rs`): there is no bridge route and no MCP tool, so an agent cannot
  start an install.
- UI: `InstallSheet.tsx`, `AgentDoctor.tsx`, `lib/agent-install.ts`.

## Install options found (2026-10-14)
| Agent | winget | npm | Native or script | Sources |
|---|---|---|---|---|
| Claude Code | `Anthropic.ClaudeCode` (portable claude.exe, no Node, no Git, no admin) | `@anthropic-ai/claude-code` (Node 22+) | `irm https://claude.ai/install.ps1 \| iex` | https://code.claude.com/docs/en/setup |
| Codex | `OpenAI.Codex` (portable zip; not in OpenAI's docs, can lag) | `@openai/codex` | `irm https://chatgpt.com/codex/install.ps1 \| iex` | https://developers.openai.com/codex/quickstart, `winget show` |
| Cursor Agent | none (`winget search cursor` lists only the editor) | none | `irm 'https://cursor.com/install?win32=true' \| iex` | https://cursor.com/docs/cli/installation |
| OpenCode | `SST.opencode` (portable zip; not in the docs) | `opencode-ai` | none on Windows (Scoop, Chocolatey, Mise also listed) | https://opencode.ai/docs/, `winget show` |
| Gemini CLI | none (`winget search gemini`: no package) | `@google/gemini-cli` | none | https://github.com/google-gemini/gemini-cli |

Gemini CLI is not a registered agent, so it has no `install.toml` yet. A definition for it would use
`[install.windows] npm = "@google/gemini-cli"` and Puppet Master would offer Node.js through winget first.

## Evidence
See `D:\pm-night\p0\doctor-installs\REPORT.md`.

## Limits
- No checked download (URL + checksum) method: the vendors publish moving "latest" URLs, so there is no stable
  checksum to pin. winget verifies the installer hash itself (it is in the manifest).
- Windows only. Other systems show the old hint.
- The PATH refresh adds folders; a tool that moved would still need a restart (the doctor says so).
