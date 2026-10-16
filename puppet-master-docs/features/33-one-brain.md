# One brain (shared project memory)

**Status:** Works today. Puppet Master (PM) writes one memory into each agent's own instruction file. It is checked live and by automated tests.

## What it is

Every agent program reads its own instruction file: Claude Code reads `CLAUDE.md`, Codex reads `AGENTS.md`, and Cursor reads rule files. If you switch agents, you must explain your project again.

With **One brain**, you write the facts about your project once, in PM. PM copies them into each agent's file. Every agent then starts with the same knowledge. The memory itself is stored in your project at `.puppet-master/memory.md`.

## Why it matters

- If you use more than one agent, you write your notes only once.
- If you have never written a `CLAUDE.md`, PM creates the file for you.
- Your own text is safe. PM only changes a marked block inside each file. It never rewrites the rest.

## How to use it

1. At the bottom of the sidebar, click **Project**. PM uses the folder of the agent you selected. If none is selected, it uses the open project. You can also press Ctrl+K and search for "memory" (the command is **Project…**).
2. Stay on the **Memory** tab.
3. Type what every agent should know: the stack, the commands, the rules, past decisions.
4. Click **Save** (or press Ctrl+S). Agents that start later in this project read it.
5. Look at **Files agents read**. It shows each file and a status: In sync, Will create, Will add a block, Will update, Will remove, or Markers damaged. A tag "Has your own text" means the file also holds text that you wrote.
6. Click **Sync now** if you want to write the files right away.

To turn it off:

- For one project: use the switch **Share memory in this project**. PM removes its block from the agent files.
- For all projects: **Settings > General > Shared project memory**.

The **Copies** tab on the same page is a different thing. It controls how a new separate copy of the project gets ready. See the feature docs about separate copies.

## How it works (simple)

PM puts its text between two marker lines in each file:

```
<!-- puppet-master:memory:start -->
...your memory...
<!-- puppet-master:memory:end -->
```

PM only changes bytes between the markers. If you had a file before, PM saves a `.bak` copy once, before it adds the first block. If the memory is empty, PM removes the block, and it deletes a file that PM itself created.

Which file an agent reads is declared by the agent's profile, not guessed by PM:

| Agent | File PM writes |
|---|---|
| Claude Code | `CLAUDE.md` |
| Codex | `AGENTS.md` |
| OpenCode | `AGENTS.md` |
| Cursor Agent | `.cursor/rules/puppet-master.mdc` |

PM syncs when an agent pane starts, when a headless run starts, when you save, and when you click **Sync now**. If a project has no memory file, a start writes nothing.

## Limits and what is not done yet

- There is no live file watcher. If you edit `memory.md` by hand, agents get the change at the next start. An agent that is already running has already read its file.
- Agents cannot write to the memory yet. A tool for this (something like `remember`) is not built.
- Only the files in the table are written. Nested `AGENTS.md` files in sub-folders are not.
- Kilo has an example definition for `AGENTS.md`, but PM does not ship it as a built-in. It only works if you add that definition.
- Cursor also reads `CLAUDE.md` and `AGENTS.md`, so it may see the memory twice.
- If a marker line is damaged, PM leaves the file alone and shows "Markers damaged".

## For developers

- Code: `packages/app/src-tauri/src/one_brain/` (`block.rs` for the marker block, `mod.rs` for targets, status and sync).
- Tauri commands: `memory_get`, `memory_save`, `memory_sync`, `memory_set_project_enabled`, `memory_global_enabled`, `memory_set_global_enabled` (`commands/one_brain.rs`).
- Agent profiles declare files with `TuiProfile::instruction_files`, or a `[instructions]` table in a TOML agent definition.
- Switches: `.puppet-master/memory.off` (per project) and `one_brain.json` in the app data folder (app wide, default on).
- UI: `components/cockpit/ProjectMemory.tsx`, `components/settings/blocks/SharedMemoryToggle.tsx`.
- Spawn hooks: `pty/registry/spawn.rs` and `agent_runs/headless/process.rs`.

## Related

- [Agents controlling agents (MCP)](32-agents-controlling-agents-mcp.md)
- [Agent skill pack](34-agent-skill-pack.md)
- [Toolbelt](35-toolbelt.md)
