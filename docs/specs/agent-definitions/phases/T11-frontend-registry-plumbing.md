# T11: Tauri agents_list command, agentRegistry.ts, loosened shared schema

Size: M. Depends on: T04, T06. Covers (PLAN ids): P1-7 (plumbing half). User-visible effect: none visible (UI still shows the nine from the compiled fallback).

## Goal

Expose the registry listing to the frontend through a Tauri command and a typed `agentRegistry.ts` with a compiled-list offline fallback, and loosen `AgentTypeSchema` to any pattern-valid id.

## Why now / depends on

Splits the frontend change: plumbing first (no visual change), consumers second (T12). Depends on: T04, T06. Index: `phases/README.md`. Spec: `SPEC.md`; original tasks: `PLAN.md` (P1-7 (plumbing half)).

## In scope

- `agents_list` Tauri command wrapping the T04 listing.
- `agentRegistry.ts` (fetch, cache, fallback).
- Shared schema loosened; `presetFor(id)` fallback.

## Out of scope

- Changing the spawn menu/consumers (T12).
- Definition management commands (T19).

## Tasks

File allow-list = the union of the files named below (`src/` = `packages/app/src-tauri/src/`). Anything else needs a plan edit.

- [ ] Tauri command + handler registration — `src/commands/agents.rs` (new), `src/commands/mod.rs`, `src/lib.rs` (one registration line) — done-when: invocable; returns nine ids
- [ ] TS registry module with offline fallback — `packages/app/src/lib/agentRegistry.ts` (new) + its test — done-when: fallback used when the command rejects
- [ ] Loosen schema and presets — `packages/shared/src/agents.ts`, `packages/shared/src/agents.test.ts` — done-when: accepts `kilo`; rejects `Bad Id`; `presetFor('kilo')` returns a neutral preset

## Tests to write first

- `shared: AgentTypeSchema accepts pattern-valid ids / rejects invalid`: table
- `shared: presetFor unknown id fallback`: label=id, neutral icon
- `app: agentRegistry fallback`: returns compiled nine when invoke fails

## Verification commands

Run from `packages/app/src-tauri` with `CARGO_TARGET_DIR=C:\Users\you\cargo-target-shared` unless noted.

- `npm run typecheck (packages/shared, packages/app)`
- `npm test (packages/shared, packages/app)`
- `cargo test --lib commands::`

End-of-phase invariant: the repo builds and all suites touched above pass; no half-migrated state.

## Risks and rollback

Tauri handler registration is in a shared file (`lib.rs`): keep the edit to one line. Rollback: revert.

## Open questions this phase depends on

None. (SPEC section 11.)

## Agent prompt

Paste everything inside the block below.

````text
You are implementing phase T11 (Tauri agents_list command, agentRegistry.ts, loosened shared schema) of the declarative agent definitions plan. Goal: Expose the registry listing to the frontend through a Tauri command and a typed `agentRegistry.ts` with a compiled-list offline fallback, and loosen `AgentTypeSchema` to any pattern-valid id.

Read first: docs/REPO_MAP.md, docs/specs/agent-definitions/SPEC.md (requirements referenced by P1-7 (plumbing half)), docs/specs/agent-definitions/phases/T11-frontend-registry-plumbing.md (your task list, tests, allow-list, verification) and the relevant PLAN.md tasks (P1-7 (plumbing half)). Prerequisite phases already merged: T04, T06.

Work: 1) write the tests listed under "Tests to write first" and see them fail; 2) do the tasks in order, touching only the allow-listed files; 3) run every command under "Verification commands" and fix until green; 4) commit with explicit `git add <paths>` (1-4 commits). Out of scope: Changing the spawn menu/consumers (T12).; Definition management commands (T19).

Hard rules (non-negotiable):
- Repo: C:\Users\you\Desktop\work2\puppet-master, branch refactor/tui-profiles. `src/` means packages/app/src-tauri/src/. Read docs/REPO_MAP.md, docs/specs/agent-definitions/SPEC.md (the requirement ids your phase covers) and this phase file first.
- Tests first: write the failing tests listed, then implement, then run the verification commands. Commit finished work unasked, 1-4 commits.
- Touch only files in this phase's allow-list. If reality diverges from SPEC (a file/function is not where the plan says, a behaviour differs), STOP and ask the caller; do not improvise a redesign.
- No recursive deletes (no `rm -rf`, `Remove-Item -Recurse`), no `git worktree remove`, no junctions/symlink tricks. Delete only single files you created.
- NEVER stage the user's uncommitted README.md, docs/wiki/README.md, package-lock.json, or packages/app/src-tauri/Cargo.toml wholesale. Always `git add <explicit paths>`; if you must add a dependency line to Cargo.toml, use `git add -p` for only your hunk (or ask). `git status` must show those four files still modified-and-unstaged afterwards. Never `git add -A` / `git add .`.
- Commit message ends with the trailer line: `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Do not push.
- Use the shared target dir: `$env:CARGO_TARGET_DIR='C:\Users\you\cargo-target-shared'` (PowerShell) or `export CARGO_TARGET_DIR=/c/Users/you/cargo-target-shared` (bash); run cargo from packages/app/src-tauri.
- If this phase changes any MCP tool schema, rebuild the dist MCP exe (separate CARGO_TARGET_DIR + rename swap) and make sure catalog_version / the pinned test agree. Edits to src-tauri restart the live dev app (~1 minute): never edit while a live test runs.
- Each handoff/report needs a fresh build_id. Use unique handle names in live tests.

Final report: commits (hashes), test counts for each verification command, any divergence from SPEC, and the fresh build_id if a live check ran.
````
