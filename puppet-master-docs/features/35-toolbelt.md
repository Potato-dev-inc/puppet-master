# Toolbelt (add an MCP server once, for every agent)

**Status:** Works today, but checked by automated tests only. It was tested on temporary config files. It was not tried against the real config files of each agent.

## What it is

MCP servers are add-on tools for agents: a browser tool, a database tool, a documentation tool. Each agent keeps its own list of MCP servers, in its own file and its own format. Normally you edit four files by hand.

The **Toolbelt** is one list inside Puppet Master (PM). You add a server once. PM then writes it into the config of every agent that supports it. Before PM changes anything, it shows you the exact change for each agent.

## Why it matters

If you use Claude Code, Codex, Cursor Agent and OpenCode, you do not repeat the same setup four times. You also do not break your config files: PM only touches its own entries, and it keeps a backup.

## How to use it

1. Open **Settings > Bridge & MCP**. The **Toolbelt** block is at the top. (You can also press Ctrl+K and choose **Toolbelt…**.)
2. Click **Add server**. Fill in:
   - **Name**, for example "Browser".
   - **Type**: **Command** (a program PM starts, with its arguments and environment variables) or **Web address** (a URL, with headers).
   - **Scope**: **All projects** or **This project only**.
3. Click **Save**. Or click **Import from agents** to copy servers that your agents already use.
4. Use the switch beside a server to turn it on or off.
5. Click **Apply to all agents**. A sheet shows each config file that will change. Click **Show file changes** to see the lines. Secret values are hidden as `********`.
6. Optional: tick **Also register Puppet Master itself in these agents**.
7. Click **Apply**. Restart agents that are already open.
8. To remove a server, click **Remove** or switch it off, then click **Apply to all agents** again. PM removes it from every agent.

## How it works (simple)

- PM writes only entries named `pm-toolbelt-<id>`. Your own entries and the rest of the file stay exactly as they were. If you add servers and then remove them all, the file is byte for byte the same as before.
- The first time PM changes a file, it saves a copy next to it with the ending `.bak`. Later changes keep that first copy. A file that did not exist gets no `.bak`.
- A second **Apply** with no changes writes nothing.
- A config file with broken JSON or TOML is left untouched, and PM shows an error row.
- Keys, tokens and passwords are stored in Windows Credential Manager and hidden in the screen. Each agent's config file still needs the real value, so PM writes it there as plain text. The screen says this.
- Agents that share one config file are shown as one row.

Supported agents today: Claude Code, Codex, Cursor Agent and OpenCode. Each one declares where its MCP list lives, so a custom agent definition can become a target without new code.

## Limits and what is not done yet

- **Kilo is not supported.** It has no declared MCP file yet. The sheet lists it under "Not supported".
- A web-address server is skipped for an agent that cannot use URL servers. The sheet tells you.
- If you already added the same server by hand, the agent may see it twice. Use **Import from agents**, then delete the hand-made entry.
- Special login types, such as OAuth for a remote server, and per-tool allow lists are not handled.
- "This project only" needs a project to be selected.
- If an agent changes its config format, PM's built-in location for it needs an update.
- Not tested live against the real agents' files.

## For developers

- Engine: `packages/app/src-tauri/src/toolbelt/` (`store.rs`, `edit.rs`, `sync.rs`, `import.rs`, `service.rs`). Server list file: `toolbelt.json` in the app data folder.
- Tauri commands: `toolbelt_list`, `toolbelt_add`, `toolbelt_update`, `toolbelt_remove`, `toolbelt_sync_preview`, `toolbelt_sync_apply`, `toolbelt_import_preview`, `toolbelt_import_apply` (`commands/toolbelt.rs`).
- Bridge routes (app session only; MCP sessions and phones get 403): `GET/POST /toolbelt`, `/toolbelt/update`, `/toolbelt/remove`, `/toolbelt/sync/preview`, `/toolbelt/sync/apply`, `/toolbelt/import/preview`, `/toolbelt/import/apply`.
- Agent declaration: `TuiProfile::mcp_host()` or a `[mcp_host]` table in a TOML agent definition (`config_path`, `format`, `key_path`, `entry`, `project_config_path`, `entry_http`).
- UI: `components/settings/toolbelt/`. Secrets in the credential store use `pm-keyring://` references in `toolbelt.json`.

## Related

- [Agent skill pack](34-agent-skill-pack.md)
- [Agents controlling agents (MCP)](32-agents-controlling-agents-mcp.md)
- [One brain](33-one-brain.md)
