# Orchestrator guides

Markdown playbooks for **external MCP orchestrators** (Cursor, Claude Desktop, Codex) driving Puppet Master worker panes — especially `opencode_native`.

## Start here

| Doc | When to read |
|-----|----------------|
| [Quickstart](quickstart.md) | First session: health check → list panes → delegate → wait |
| [OpenCode native worker](opencode-native-worker.md) | API vs TUI, yes/no questions, permissions, models |
| [MCP tool cheatsheet](mcp-cheatsheet.md) | Which tool to call and what **not** to call |
| [Workflows](workflows.md) | Copy-paste flows: implement, yes/no, permission, model switch |
| [Token efficiency](token-efficiency.md) | Why orchestrator + workers beats polling and Cursor subagents |

## One-paragraph contract

You are the **orchestrator**. Workers (`opencode_native`, Claude, Codex, …) hold implementation context. Your job: pick/reuse a pane, delegate with `write_terminal_input`, **wait once** (`wait_for_worker` / `suggested_wait`), read **structured** output (`read_opencode_messages`), unblock API prompts (`reply_opencode_question`, `reply_opencode_permission`), and hand off — not re-read scrollback in a loop.

## Prerequisites

1. Puppet Master desktop running (`npm run tauri dev` or `npx puppet-master`).
2. MCP registered — see [MCP_HOSTS.md](../../MCP_HOSTS.md).
3. For local dev: `npm run build:mcp` and use `legacy.js` (loads tool catalog from bridge) or toggle MCP after rebuild.

## Related

- [MCP_HOSTS.md](../../MCP_HOSTS.md) — host registration and tool reference
- [ROUTING.md](../../ROUTING.md) — API vs CLI orchestrator backends
- [ROADMAP.md](../../ROADMAP.md) — planned session context, skills, UI
- `.cursor/rules/imported/puppet-master/mcp-wait-not-poll.mdc` — always-on Cursor rule

## Next improvements

Prioritized project work is listed in [opencode-native-worker.md § Next steps](opencode-native-worker.md#next-steps-for-this-project).
