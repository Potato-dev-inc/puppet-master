# T03: One agent_type schema helper, single catalog_version bump

Size: M. Depends on: T02. Covers (PLAN ids): P0-2, P0-6. User-visible effect: MCP clients may pass any pattern-valid agent_type; errors come from the server.

Milestone: Phase 0 gate (schema stable forever)

## Goal

Replace the three hard-coded `agent_type`/`backend` enums with one `agent_type_schema()` (string + pattern + description), bump and pin `catalog_version` once, rebuild the MCP exe and prove no SCHEMA_DRIFT.

## Why now / depends on

After this, adding an agent never changes the MCP schema again. Depends on T02 so loosened input still gets a clear error. Depends on: T02. Index: `phases/README.md`. Spec: `SPEC.md`; original tasks: `PLAN.md` (P0-2, P0-6).

## In scope

- `agent_type_schema()` helper used by every tool taking `agent_type`/`backend`.
- TS catalog parity.
- Pinned test update (once).
- Rebuild dist MCP exe, restart app, run pm-regression (the old P0-6 gate).

## Out of scope

- `GET /agents/types` (T04).
- Any behaviour change in dispatch.

## Tasks

File allow-list = the union of the files named below (`src/` = `packages/app/src-tauri/src/`). Anything else needs a plan edit.

- [ ] Add `agent_type_schema()` with pattern `^[a-z][a-z0-9_]{1,31}$` and description pointing at `GET /agents/types` — `src/tool_registry/schema.rs` — done-when: helper unit test
- [ ] Use it in all tools; delete the three enums — `src/tool_registry/catalog/agents.rs`, `src/tool_registry/catalog/operations.rs`, `src/tool_registry/catalog/panes.rs` — done-when: grep for the agent-id enum literal finds none in catalog
- [ ] Update the pinned version once — `src/tool_registry/tests.rs` — done-when: `catalog_version_is_pinned` green with the new value
- [ ] Mirror in the TS catalog — `packages/mcp-server/src/catalog.ts` — done-when: `npm test` in packages/mcp-server green
- [ ] Rebuild dist MCP exe (separate CARGO_TARGET_DIR + rename swap), restart app, run regression — no source files — done-when: /health.catalog_version == shim catalog_version(); pm-regression 28/28

## Tests to write first

- `tool_registry::agent_type_schema_has_pattern_not_enum`: every tool with agent_type/backend uses the helper output
- `tool_registry::catalog_version_is_pinned`: updated value
- `mcp-server catalog test`: TS catalog agent_type entry matches the Rust output

## Verification commands

Run from `packages/app/src-tauri` with `CARGO_TARGET_DIR=C:\Users\Ren-pc\cargo-target-shared` unless noted.

- `cargo test --lib tool_registry`
- `npm test (in packages/mcp-server)`
- `node scripts/pm-regression.mjs  -> 28/28`
- GET /health vs shim catalog_version(): equal; bridge_health shows no SCHEMA_DRIFT

End-of-phase invariant: the repo builds and all suites touched above pass; no half-migrated state.

## Risks and rollback

The bump invalidates running shims (SCHEMA_DRIFT) until rebuilt: do the exe rebuild in this phase. Never edit src-tauri while a live test runs. Rollback: revert and rebuild exe.

## Open questions this phase depends on

None. (SPEC section 11.)

## Agent prompt

Paste everything inside the block below.

````text
You are implementing phase T03 (One agent_type schema helper, single catalog_version bump) of the declarative agent definitions plan. Goal: Replace the three hard-coded `agent_type`/`backend` enums with one `agent_type_schema()` (string + pattern + description), bump and pin `catalog_version` once, rebuild the MCP exe and prove no SCHEMA_DRIFT.

Read first: docs/REPO_MAP.md, docs/specs/agent-definitions/SPEC.md (requirements referenced by P0-2, P0-6), docs/specs/agent-definitions/phases/T03-agent-type-schema-unify.md (your task list, tests, allow-list, verification) and the relevant PLAN.md tasks (P0-2, P0-6). Prerequisite phases already merged: T02.

Work: 1) write the tests listed under "Tests to write first" and see them fail; 2) do the tasks in order, touching only the allow-listed files; 3) run every command under "Verification commands" and fix until green; 4) commit with explicit `git add <paths>` (1-4 commits). Out of scope: `GET /agents/types` (T04).; Any behaviour change in dispatch.
Phase note: This phase touches MCP schemas: after the code change, rebuild the dist MCP exe with a separate CARGO_TARGET_DIR and rename-swap it, then restart the app and confirm no SCHEMA_DRIFT.
Hard rules (non-negotiable):
- Repo: C:\Users\Ren-pc\Desktop\work2\puppet-master, branch refactor/tui-profiles. `src/` means packages/app/src-tauri/src/. Read docs/REPO_MAP.md, docs/specs/agent-definitions/SPEC.md (the requirement ids your phase covers) and this phase file first.
- Tests first: write the failing tests listed, then implement, then run the verification commands. Commit finished work unasked, 1-4 commits.
- Touch only files in this phase's allow-list. If reality diverges from SPEC (a file/function is not where the plan says, a behaviour differs), STOP and ask the caller; do not improvise a redesign.
- No recursive deletes (no `rm -rf`, `Remove-Item -Recurse`), no `git worktree remove`, no junctions/symlink tricks. Delete only single files you created.
- NEVER stage the user's uncommitted README.md, docs/wiki/README.md, package-lock.json, or packages/app/src-tauri/Cargo.toml wholesale. Always `git add <explicit paths>`; if you must add a dependency line to Cargo.toml, use `git add -p` for only your hunk (or ask). `git status` must show those four files still modified-and-unstaged afterwards. Never `git add -A` / `git add .`.
- Commit message ends with the trailer line: `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Do not push.
- Use the shared target dir: `$env:CARGO_TARGET_DIR='C:\Users\Ren-pc\cargo-target-shared'` (PowerShell) or `export CARGO_TARGET_DIR=/c/Users/Ren-pc/cargo-target-shared` (bash); run cargo from packages/app/src-tauri.
- If this phase changes any MCP tool schema, rebuild the dist MCP exe (separate CARGO_TARGET_DIR + rename swap) and make sure catalog_version / the pinned test agree. Edits to src-tauri restart the live dev app (~1 minute): never edit while a live test runs.
- Each handoff/report needs a fresh build_id. Use unique handle names in live tests.

Final report: commits (hashes), test counts for each verification command, any divergence from SPEC, and the fresh build_id if a live check ran.
````
