# One brain: one project memory, every agent reads it

Branch `feat/one-brain`.

## What and why

Each agent CLI reads its own instruction file. Switching agents means re-explaining the project every time.
Puppet Master keeps ONE memory, `<project>/.puppet-master/memory.md`, and renders it into each agent's own
file inside a managed marker block, so every agent starts with the same knowledge.

- Personas: A (developers who switch agents), B (vibecoders who never wrote a CLAUDE.md).
- Agent-agnostic: which file an agent reads is DECLARED by the agent (`TuiProfile::instruction_files` or the
  TOML `[instructions]` table). Generic code never matches an agent name, and the generic default is "no files".

## Design

Rust (`packages/app/src-tauri/src/one_brain/`):

- `block.rs` (pure bytes in, bytes out): the marker block. Inserts or replaces
  `<!-- puppet-master:memory:start -->` ... `<!-- puppet-master:memory:end -->`. Bytes outside the block are never
  rewritten. Keeps CRLF vs LF and a BOM. Creates a missing file with only the block (plus an optional header).
  Removes the block cleanly when memory is empty (and deletes a file that only held our block/header). A start
  marker without an end marker is reported as damaged and the file is left alone.
- `mod.rs`: targets (merged from every registered profile; same path = one target), per-project opt-out
  (`.puppet-master/memory.off`), app-wide switch (`one_brain.json` in the app data dir, default on), `status()`
  for the UI, `sync()` (no write when unchanged, atomic write via temp + rename, `<file>.bak` once before the
  first block goes into an existing user file), `sync_on_spawn()`.
- `commands/one_brain.rs`: Tauri commands `memory_get`, `memory_save`, `memory_sync`,
  `memory_set_project_enabled`, `memory_global_enabled`, `memory_set_global_enabled`.

Declarations:

- Trait: `TuiProfile::instruction_files() -> Vec<InstructionFile { path, header }>`, default empty.
- TOML: `[instructions]` with `file = "AGENTS.md"` or `files = ["AGENTS.md", ".cursor/rules/x.mdc"]` and an
  optional `header` (written once when the file is created, for front-matter). Validation rejects absolute paths,
  drives and `..`. The table is part of the definition fingerprint only when present, so existing approvals stay valid.
- Shipped declarations are only the verified ones (table below). The Kilo declaration is in the example definition
  `tests/agent-definitions/kilo.toml`.

When it syncs:

- Before a pane spawns (`pty/registry/spawn.rs::spawn_pane`, after the cwd is resolved, before the PTY exists;
  not the PTY reader) and before a headless run (`agent_runs/headless/process.rs::execute_headless`).
- When memory is saved or "Sync now" is pressed in the UI, and when a switch changes.
- A spawn in a project with no `memory.md` writes nothing and creates nothing.
- No file watcher: editing `memory.md` by hand is picked up at the next spawn (a running agent has already read
  its file, so a live watcher would not change what it knows).

UI (monochrome, cockpit tokens): `components/cockpit/ProjectMemory.tsx`, opened from the sidebar footer
("Project memory") and the palette ("Project memory..."). Textarea editor (mono), Save (Ctrl/Cmd+S), Sync now,
a list of the files agents read with a status each (In sync, Will create, Will add a block, Will update, Will
remove, Markers damaged) and a "Has your own text" tag when the file holds text outside the block, a per-project
switch, and the Settings > General toggle "Shared project memory" (`SharedMemoryToggle.tsx`).

OSS: none needed.

## Verified file per agent

| Agent (id) | File written | Source |
|---|---|---|
| Claude Code (`claude`) | `CLAUDE.md` | https://code.claude.com/docs/en/memory (CLAUDE.md; AGENTS.md only via an import) |
| Codex (`codex`) | `AGENTS.md` | https://developers.openai.com/codex/guides/agents-md |
| OpenCode (`opencode`, `opencode_native`) | `AGENTS.md` | https://opencode.ai/docs/rules/ (AGENTS.md wins; CLAUDE.md is only a fallback when AGENTS.md is missing) |
| Cursor CLI (`cursor_agent`, `cursor`) | `.cursor/rules/puppet-master.mdc`, header `alwaysApply: true` | https://cursor.com/docs/context/rules and https://cursor.com/docs/cli/using (the CLI also reads AGENTS.md and CLAUDE.md at the root) |
| Kilo (TOML definition) | `AGENTS.md` | https://kilo.ai/docs/customize/agents-md |
| shells (`cmd`, `powershell`, `bash`) | none | not agents |

Note: OpenCode with both files present reads only AGENTS.md, and Cursor CLI reads CLAUDE.md and AGENTS.md as
well as the rule, so Cursor may see the same memory twice. Declaring the rule file only is the safe choice; a user can
add more files in a definition.

## Evidence

Commands run in `D:\pm-one-brain`:

```
cd packages/app/src-tauri && CARGO_BUILD_JOBS=2 cargo test --lib
  test result: FAILED. 1041 passed; 1 failed; 2 ignored   (the one failure is the known
  bridge::tests::definitions::approve_requires_app_session)
cargo test --lib one_brain
  test result: ok. 33 passed; 0 failed
npm test --workspace=@puppet-master/app     Test Files 48 passed (48), Tests 379 passed (379)
npx tsc --noEmit -p packages/app            exit 0
node scripts/evidence/screenshot.mjs --out D:\pm-night\evidence\one-brain cockpit-memory
  ok   cockpit-memory -> D:\pm-night\evidence\one-brain\cockpit-memory.png
```

Rust tests (temp projects), in `one_brain/tests.rs` and `one_brain/block.rs`:

- insert into an existing CLAUDE.md with user text above and below: slices outside the block compared equal
  (`insert_keeps_user_content_above_and_below_byte_for_byte`)
- update, idempotent no-write (mtime unchanged), remove-on-empty restores the exact bytes and deletes a file we
  created, create-missing (and no memory creates nothing), CRLF preserved (no bare LF), BOM kept and never added,
  `.bak` made once and not overwritten, damaged markers left alone, per-project and app-wide off remove blocks
- multiple targets from TOML with a header (`toml_declares_several_files_with_a_header_and_no_table_means_none`),
  an agent without a declaration has no files and writes none, an out-of-project path is rejected at load
- `code_profiles_declare_only_verified_files`: claude, codex, opencode, cursor_agent declare exactly the table
  above; cmd, powershell, bash and an unknown id declare nothing
- spawn hook: `the_spawn_hook_syncs_every_registered_agents_file_in_the_cwd` (a spawn in a project with memory
  writes all declared files; a project without memory is untouched) and `spawn_paths_call_the_hook`.
  Fail-before: with the one hook line removed from `execute_headless`, `spawn_paths_call_the_hook` fails
  (`assertion failed: headless.contains("crate::one_brain::sync_on_spawn(&project)")`) and passes with it.
  The wiring test is a source check because `spawn_pane` needs a Tauri `AppHandle`; the behaviour itself is tested
  through `sync_for_spawn`.

Frontend (`ProjectMemory.test.tsx`, 6 tests): the editor shows the memory and the copy; Save is enabled only when the
text changed and saves what was typed; the file list shows each status and the "Has your own text" tag; Sync now
keeps an unsaved draft; a backend sync error is shown instead of success; the per-project switch calls the backend.

Screenshot: `D:\pm-night\evidence\one-brain\cockpit-memory.png`.

## Limits / follow-ups

- Agent write-back (a `remember` MCP tool or `POST /memory/append`) is NOT built: no catalog changes tonight.
  Follow-up: a `remember` tool that appends a line to memory.md and syncs.
- No live file watcher (see above). `notify` is already a dependency, so a debounced watch of the active project is a
  small follow-up.
- Only the files declared by profiles are written. Subfolder instruction files such as nested `AGENTS.md` are not.
- Syncing runs for every registered agent's files on any spawn (including a shell), but only when the project has a
  memory (or a block to remove). Turning the app-wide switch off removes blocks in the open project at once and in
  other projects the next time something spawns there.
- Round trip is exact for files that end in a newline; a file without a final newline gains one when a block is
  inserted and keeps it after removal.
- In the cockpit, pressing Inbox while no pane is selected does not close the memory view (selecting a pane does).

## File overlap with other branches

Shared files touched with small append-only edits (merge hot-spots): `src-tauri/src/lib.rs` (one `mod`, six
handler lines), `commands/mod.rs` (two lines), `tui/profile.rs` (one trait method), `tui/types.rs` (one struct),
`tui/declarative/{schema,validate,profile,fingerprint}.rs`, `tui/{claude,codex,opencode,cursor}/mod.rs`,
`tests/agent-definitions/kilo.toml`, `tui/README.md`, `pty/registry/spawn.rs` (one line in `spawn_pane`, outside the
PTY reader), `agent_runs/headless/process.rs` (one line), cockpit `CockpitApp.tsx`, `Sidebar.tsx`, `Palette.tsx`,
`Icons.tsx`, `styles/cockpit.css` (appended), `dev/mock-tauri.ts` (cases + fixture appended),
`components/settings/tabs/GeneralTab.tsx` (one line). The MCP catalog is untouched.
New files: `one_brain/*`, `commands/one_brain.rs`, `lib/memory-api.ts`, `ProjectMemory(.test).tsx`,
`SharedMemoryToggle.tsx`, `dev/scenarios/cockpit-memory.ts`.
