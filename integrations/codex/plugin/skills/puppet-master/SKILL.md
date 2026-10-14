---
name: puppet-master
description: Delegate work to other AI coding agents (Claude Code, Codex, Cursor, OpenCode) through the Puppet Master MCP tools. Use it for a second opinion or review from a different agent, a quick question to another model, a parallel or risky change in an isolated git worktree, or to check on agents that are already running. Needs the Puppet Master desktop app to be running.
---
<!-- puppet-master-pack: 1 -->

# Delegating with Puppet Master

Puppet Master (PM) lets you hand work to other coding agents and get back a result plus a receipt that says what was and was not verified. Your host may show its tools with a prefix (for example `mcp__puppet-master__run_agent`). If they are missing, the PM app is not running: tell the user, do not guess.

## When to use it

- A **second opinion** from a different agent before you call work done.
- A **tiny question** you want answered by another model.
- A **parallel** or **risky** change that should not touch the user's files yet.
- **Checking on** agents that are already running.

Do not delegate what you can do directly in a few steps, and never put secrets in a task.

## The five tools

1. **`quick_ask(task)`** for a tiny question. Read `answer`. `receipt.not_verified` lists what was not checked. If it returns `status: "running"`, call `wait_agents` with the `handle`.
2. **`run_agent(agent_type, project_path, task)`** for real work. `agent_type` is `claude`, `codex`, `cursor_agent` or `opencode_native`. It opens a visible pane the user can watch (`headless: true` hides it), runs the task and returns `status`, `result`, `receipt` and a `handle`. Pick a different agent than yourself for a review.
3. **`wait_agents(handles)`** collects results. Each call waits at most 120 s, so loop while `status` is `running`. A turn that still runs carries a `watch_command`: run it in your shell tool's background mode.
4. **`followup_task(handle, task)`** is the next turn with the same agent and the same conversation. `send_message(handle, message)` steers the turn that is running now.
5. **`list_agents()`** shows agents you can reuse: pass the `worker_id` to `run_agent` to adopt one.

## Do not trust "done"

Read `receipt.outcome`. Only `passed_checks` means the result was proven. `completed_unchecked` means the agent finished and nothing verified it.

Pass `checks` to `run_agent` so success is measured, for example:

```json
{ "checks": [
  { "type": "command_exits", "command": ["npm", "test"] },
  { "type": "file_exists", "path": "src/new-file.ts" }
] }
```

Other check types: `file_contains` (with `text`), `file_absent`, `result_contains`. A failed check ends the run as `ACCEPTANCE_FAILED`. When you report back, say what the receipt did not verify.

## Isolate risky or parallel edits

Pass `isolation: "worktree"` to `run_agent`. The agent works on its own branch `pm/<handle>` under `.puppet-master/worktrees`, so parallel agents never touch the same files. The receipt gains `worktree` with the change counts. Nothing is merged for you:

- `review_worktree(handle)` shows the diff.
- `review_worktree(handle, action: "merge")` merges it. `action: "discard"` deletes the work.

Merge or discard only when the user has said so.

## Ask the human

- `ask_user(question, choices?)` when only the person can decide. It shows in the PM Inbox and on a paired phone, and blocks until answered. Do not ask what you can find out yourself.
- If a run ends `needs_input`, it carries a `prompt`. Show it to the user with `ask_user`, then pass their choice to `answer_prompt(handle, prompt_id, choice)`. Never choose approvals for them.

## Etiquette

- Use `read_only: true` for reviews and questions.
- `interrupt_agent(handle)` stops a turn and keeps the handle.
- Do not close the user's own panes. Panes a run created close themselves; pass `keep_pane: true` to keep one.
- Reuse the same `idempotency_key` when you retry after a timeout, so no duplicate turn starts.
