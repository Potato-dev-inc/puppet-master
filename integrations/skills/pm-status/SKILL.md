---
name: pm-status
description: Show which agents Puppet Master is running, what each is doing, and which need your answer.
disable-model-invocation: true
---
<!-- puppet-master-pack: 1 -->

Summarise the agents running in Puppet Master with its MCP tools. If they are missing, say the PM app is not running and stop.

1. Call `list_agents` with `active_only: true`. If it is empty, say so.
2. For handles that are still running, call `wait_agents` with a short `timeout_ms` (for example 2000) to refresh their state.
3. Show one line per agent: name or handle, agent type, status, and what it is working on. For `needs_input`, include the prompt and ask the user what to answer with `ask_user`; then use `answer_prompt(handle, prompt_id, choice)`.
4. For finished runs of interest, read `receipt.outcome` and `receipt.not_verified`.
