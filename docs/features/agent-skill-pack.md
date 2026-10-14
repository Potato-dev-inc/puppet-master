# Agent skill pack: Puppet Master inside each agent

Branch `feat/agent-skill-pack`. Pack files: `integrations/`. Installer: `packages/app/src-tauri/src/agent_pack/`. Evidence: `D:\pm-night\p0\skill-pack\`.

## 1. What and why

Personas A and B find Puppet Master (PM) where they already are (their agent's plugin store, their config) and their agent uses PM well without guessing: one short skill teaches the five-tool path (`quick_ask`, `run_agent`, `wait_agents`, `followup_task`, `list_agents`), receipts and checks, worktree isolation with `review_worktree`, and `ask_user`. Persona C gets one row in Settings that does it for every agent on the computer, with a review first and an Undo.

Agent-agnostic: no generic code names an agent. Each agent's profile declares where it reads skills (`TuiProfile::skills_host`, a TOML const like `mcp_host`); the MCP entry uses the toolbelt's existing `[mcp_host]` table, renderer and splice editor.

## 2. Design

- `integrations/skills/` is canonical (4 skills). `scripts/sync-skill-pack.mjs` copies it into the Claude Code, Codex and Cursor plugin folders; `agent_pack::tests::pack_files_match_canonical` fails on drift. The app embeds the canonical files with `include_str!`.
- Skills folders: `~/.claude/skills` (Claude Code), `~/.agents/skills` (Codex, Cursor, OpenCode; all four read it). Agents that share a folder or a config file share one write.
- MCP entry: added only when the agent's config has no entry under its key. An existing one is never rewritten.
- Pack files carry `<!-- puppet-master-pack: N -->`. An older pack copy is updated; a foreign file at a pack path is replaced only after a backup (`kind: replace`).
- Undo uses `<app data>/agent-pack.json`: created files (and the directories apply created) are removed, replaced files restored from `agent-pack-backups/`, an MCP config restored from the `.bak` apply made (or only the key removed when the file changed). A file edited after install is left alone and reported.
- UI: `components/settings/pack/AgentPackGroup.tsx`, `lib/agent-pack.ts`; Tauri commands `agent_pack_preview|apply|undo`. No bridge route, so MCP sessions and phones cannot call it.

OSS: none new (`sha2`, `serde_json`, `toml` already in the crate). Formats follow the vendors' docs, cited in `integrations/README.md`.

## 3. Evidence

- `cargo test --lib`: `1899 passed; 0 failed; 13 ignored` twice. `cargo test --bins`: `46 passed; 0 failed`.
- `npx vitest run`: `141 files, 1159 tests passed`. `npx tsc --noEmit -p packages/app`: clean.
- New Rust tests (15, `cargo test --lib agent_pack`): `golden_files_per_agent` (claude, codex, cursor_agent, opencode against `agent_pack/golden/*`, preview writes nothing, second apply changes nothing, `.bak` equals the user's file), `undo_restores_the_originals_byte_for_byte` (files, bytes and directories, including a foreign skill that was replaced), `an_entry_that_is_already_there_is_never_rewritten`, `undo_leaves_files_the_user_changed_since`, `agents_that_read_one_folder_share_one_write`, `an_agent_that_is_not_installed_is_skipped_and_nothing_is_created`, `a_config_that_is_not_valid_is_left_untouched_and_the_skills_still_install`, `a_folder_outside_home_is_refused`, `the_record_survives_a_restart`, and the text tests: `the_pack_teaches_real_tools_and_real_parameters` (every taught tool is in the default listed set and every named parameter exists in its schema; no hidden alias, read from `tool_registry::ALIASES`), `pack_files_match_canonical`, `skill_files_follow_the_skill_format`, `manifests_are_consistent`.
- Fails before, passes after: with `list_workers` put into `pm-status/SKILL.md` the two text tests fail (`fail-before.txt`: "pm-status/SKILL.md teaches the hidden alias list_workers" and "plugin copy of pm-status/SKILL.md drifted").
- Vitest: `AgentPackGroup.test.tsx` (7 tests).
- Real CLI validation: `claude plugin validate integrations/claude-code/plugin` and `claude plugin validate .` (marketplace) both print "Validation passed". `server.json` validates against the vendored 2025-12-11 schema with ajv.
- Screenshots (`shots/`): `settings-agent-pack.png`, `settings-agent-pack-preview.png`.
- Live check (one `claude -p` call, `live-claude.jsonl`): the plugin loaded through `--plugin-dir` from a scratch folder (nothing in `~/.claude` was changed); init lists `puppet-master:puppet-master`, `:pm-review`, `:pm-delegate`, `:pm-status` as skills and slash commands. Claude invoked the `puppet-master` skill on its own for "ask Codex what 2+2 is" and named `quick_ask`, `run_agent`, `wait_agents`. The MCP server did not connect (`--strict-mcp-config` dropped the plugin's server), so no tool call happened and the skill's "tools are missing: say so, do not guess" path ran. To finish the check, run the same command without `--strict-mcp-config`.

## 4. Limits and follow-ups

- The published `@puppet-master/mcp` (npm 0.1.3) starts the legacy server with 17 shell tools and no `run_agent`/`quick_ask`. The static plugin and config files use `npx -y @puppet-master/mcp` and are right only after a release with the Rust shim. The in-app installer writes the installed app's program path, so it is not affected.
- The app's own Codex registration (`mcp_install/codex.rs`) sets no `tool_timeout_sec`; Codex's default is 60 s while `run_agent` may wait 300 s. The pack's `codex/config.toml` sets 330.
- Not verified against the real stores: Codex plugin MCP bundling (skills-only here), the Cursor plugin, MCP Registry publishing (needs `mcpName` in `packages/mcp-server/package.json` and a name decision after the rename).
- `skills_host` is declared by the built-in profiles only, not yet by user TOML definitions.
- "Run in Puppet Master" badge: snippet links to the installers; the `puppetmaster://open` deep link and HTTPS redirect page do not exist yet.
- Cursor and OpenCode also read `~/.claude/skills`, so with Claude Code present they may list a skill twice.

## 5. File overlap with other branches

Shared files, all additive: `tui/profile.rs` (one default trait method), `tui/{claude,codex,cursor,opencode}/mod.rs` (a const and a method each), `lib.rs` (`mod agent_pack;` and three command registrations), `commands/mod.rs`, `components/settings/sections/BridgeSection.tsx` (import and one row), `dev/fixtures.ts`, `dev/mock-tauri.ts`, `.gitattributes`, `README.md`, `docs/REPO_MAP.md`. The MCP tool catalog is not touched.
