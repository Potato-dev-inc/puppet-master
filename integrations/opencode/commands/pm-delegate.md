---
description: Hand a task to another agent in an isolated worktree (Puppet Master)
---

Delegate this task to another agent with the Puppet Master MCP tools: $ARGUMENTS

1. Restate the task so a fresh agent can act on it alone. Choose an installed agent with `list_agents`.
2. Use the project's own test or build command as a `command_exits` check; ask the user once if you cannot tell.
3. Call `run_agent` with `agent_type`, `project_path`, `task`, `checks` and `isolation: "worktree"` when the task edits files.
4. Loop `wait_agents` while the status is `running`.
5. Report the result, `receipt.outcome`, `receipt.not_verified` and the change counts, show `review_worktree(handle)`, and merge or discard only if the user says so.
