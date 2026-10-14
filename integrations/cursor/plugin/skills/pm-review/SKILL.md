---
name: pm-review
description: Get a second opinion on the current changes from a different AI agent through Puppet Master. Optional text after the command names the agent or the focus.
disable-model-invocation: true
---
<!-- puppet-master-pack: 1 -->

Get a second opinion on the work in this repository from a **different** agent than yourself, using the Puppet Master MCP tools. If they are missing, say the PM app is not running and stop.

1. Find what changed: `git status --short` and `git diff` (staged and unstaged). If nothing changed, review the last commit. Note the repository root.
2. Choose the reviewer. If the user named an agent after the command, use it. Otherwise call `list_agents`, and pick an installed agent that is not you (`claude`, `codex`, `cursor_agent`, `opencode_native`).
3. Call `run_agent` with `agent_type`, `project_path` set to the repository root, `read_only: true`, and a task like: "Review the uncommitted changes in this repository (run git diff). Report concrete problems only: bugs, missing edge cases, broken tests, risky changes. For each, give file:line, why it matters and a suggested fix. If you find nothing serious, say so. Do not edit files." Add any focus the user gave.
4. If the status is still `running`, loop `wait_agents`.
5. Report back: the findings, which ones you checked yourself and which you did not, and `receipt.not_verified`. Do not apply fixes unless the user asks.
