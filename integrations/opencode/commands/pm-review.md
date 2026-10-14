---
description: Second opinion on the current changes from a different agent (Puppet Master)
---

Get a second opinion on the work in this repository from a different agent than yourself, using the Puppet Master MCP tools. If they are missing, say the PM app is not running and stop.

1. Run `git status --short` and `git diff`. If nothing changed, review the last commit.
2. Pick the reviewer: "$ARGUMENTS" names the agent or the focus if it is not empty; otherwise call `list_agents` and choose an installed agent that is not you.
3. Call `run_agent` with `read_only: true`, `project_path` set to the repository root, and a task asking for concrete problems only (bugs, edge cases, broken tests, risky changes), each with file:line, why it matters and a suggested fix. The reviewer must not edit files.
4. Loop `wait_agents` while the status is `running`.
5. Report the findings, which you verified yourself, and `receipt.not_verified`. Do not apply fixes unless asked.
