# Changelog

## Unreleased

- Agent Wizard ("Finish without me"): a permission question that sits under the list of tool calls an agent
  printed (`Edited 1 file`, `Explored 1 file`) is read from its real options again, so the wizard answers it
  with the one-time yes instead of stopping. When a question really is yours to answer, the agent's pane now
  stays open: "Show the pane" opens it and Retry goes on from there (before, the stop closed the pane and
  the message told you to answer in a pane that was gone). If the agent quit by itself the message says so.
- Agent updates are now visible. When Claude Code, Codex, Cursor or OpenCode update, Settings > Agents and the Inbox say how it went
  ("Claude Code 2.3.0 to 2.4.0: checked, all good." or "...: screens changed. Puppet Master will ask you before acting."). A check that
  passes on this computer clears the "Untested version" label for that exact version only; the next update starts again. A run that
  cannot find an agent's input box, a screen Puppet Master cannot read, or a turn that goes quiet without saying it finished now starts
  one check (at most one per agent and version, never faster than every 10 minutes, and only while "Check agents after they update" is
  on). Before a run starts, a changed program file makes Puppet Master read the version right away instead of waiting up to 15 minutes.
- Windows: no more console windows flashing on screen while an agent runs. Starting the Cursor agent
  (whose launcher is PowerShell) or stopping a run used to open and close a PowerShell/`taskkill`
  window each time. Every helper process now starts hidden through one place (`proc_util`), a test
  fails if new code starts one any other way, and checking whether an MCP connection is still alive
  no longer starts `tasklist` at all.
- The "+" menu starts an agent in the folder you selected. **Separate copy** now starts off (it used to start
  on for a Git project); turn it on per agent, and your own choice for a project is still remembered.
  "+" starts in the last folder you used, Recent projects never list a separate copy, and the pane header
  says "separate copy of <folder>" when it is one.

## 0.1.5 - security patch

- Fixes a bridge authorization bypass: requests that came through a tunnel, a reverse proxy or a
  web page were trusted as the desktop app and needed no paired-device token. All earlier
  releases (0.1.0 to 0.1.4) are affected; update. Details and what to do:
  `docs/security/2026-10-14-tunnel-auth-bypass.md`.

## 0.1.2 — PWA fixes and MCP publishing

- Mobile PWA bug fixes and polish
- MCP server publishing workflow (`@puppet-master/mcp`)

## 0.1.0 — initial scaffold


- Monorepo: `@puppet-master/{shared,cli,bridge,mcp,app}` workspaces
- Tauri 2 + React + xterm.js desktop app
- Rust PTY manager with `portable-pty` (Windows ConPTY)
- 5 MCP tools: `list_panes`, `spawn_agent`, `read_terminal_buffer`, `write_terminal_input`, `kill_pane_process`
- Local HTTP bridge (Node stdlib) with SSE event stream
- Built-in Puppet Master LLM chat (Anthropic + OpenAI) using the same MCP tools
- Settings panel for API keys + model picker (Anthropic / OpenAI)
- Status heuristics: `running` / `waiting_input` / `idle` / `error`
- Unit tests: 6 scrollback buffer tests passing
- Bridge smoke test: `scripts/test-bridge.ps1`
- MCP smoke test: `scripts/test-mcp.ps1`
