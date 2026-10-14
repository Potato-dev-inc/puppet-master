---
name: pm-delegate
description: Hand a task to another AI agent through Puppet Master, isolated in its own git worktree, and report what was verified. The text after the command is the task.
disable-model-invocation: true
---
<!-- puppet-master-pack: 1 -->

Delegate the user's task to another agent with the Puppet Master MCP tools. If they are missing, say the PM app is not running and stop.

1. Restate the task in one sentence a fresh agent can act on alone: the goal, the files or area, and what "done" means. If the user named an agent, use it; otherwise call `list_agents` and choose an installed agent that suits the task.
2. Decide how success is measured. Use the project's own test or build command as a `command_exits` check (for example `["npm","test"]`). If you cannot tell, call `ask_user` once.
3. Call `run_agent` with `agent_type`, `project_path` (the repository root), `task`, `checks`, and `isolation: "worktree"` when the task edits files.
4. Loop `wait_agents` until the status is not `running`. For follow-up work on the same agent, use `followup_task(handle, task)`.
5. Report: the result in your words, `receipt.outcome`, the checks line, `receipt.not_verified`, and for a worktree the change counts. Then show the diff with `review_worktree(handle)`. Merge or discard only if the user says so.
