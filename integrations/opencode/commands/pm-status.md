---
description: Which agents Puppet Master is running and which need an answer
---

Summarise the agents running in Puppet Master. Call `list_agents` with `active_only: true`; refresh running handles with `wait_agents` and a short `timeout_ms`. Show one line per agent: name or handle, type, status, current work. For `needs_input`, show the prompt and ask the user with `ask_user`, then use `answer_prompt(handle, prompt_id, choice)`. $ARGUMENTS
