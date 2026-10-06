# T04: GET /agents/types listing the nine code profiles

Size: S. Depends on: T02. Covers (PLAN ids): P0-4. User-visible effect: new read-only endpoint listing agent types (no UI yet).

## Goal

Add the `GET /agents/types` route returning the SPEC 7.5 shape for the existing nine profiles, so later phases (registry, frontend, schema description) have one source to read.

## Why now / depends on

Needed by the frontend plumbing (T11) and the schema description (T03 points at it); static content only, independent of the registry. Depends on: T02. Index: `phases/README.md`. Spec: `SPEC.md`; original tasks: `PLAN.md` (P0-4).

## In scope

- `Descriptor` struct and builder from `Features`/headless support.
- Route returning `installed` from `is_installed()` and derived `modes`.

## Out of scope

- `source`/`state` for declarative agents (T19).
- Frontend use (T11).

## Tasks

File allow-list = the union of the files named below (`src/` = `packages/app/src-tauri/src/`). Anything else needs a plan edit.

- [ ] Add `Descriptor` and builder — `src/tui/types.rs`, `src/tui/mod.rs` — done-when: unit test builds all nine
- [ ] Add the GET route (find the file holding `/agents` GET routes first: `src/agent_runs/routes.rs` or `src/agent_runs/routes/list.rs`; `src/bridge/routes/mcp.rs` only if that is where it belongs) — `src/agent_runs/routes.rs`, `src/agent_runs/routes/list.rs`, `src/bridge/routes/mcp.rs` — done-when: route returns 200 with the nine ids
- [ ] Route test — `src/bridge/tests.rs` — done-when: asserts every `tui::profiles()` id appears with `modes`, `installed`

## Tests to write first

- `bridge::agents_types_lists_every_code_profile`: response ids == profiles() ids, in registry order
- `tui::descriptor_modes_follow_features`: headless-capable profiles list `headless`; visible always `visible`

## Verification commands

Run from `packages/app/src-tauri` with `CARGO_TARGET_DIR=C:\Users\Ren-pc\cargo-target-shared` unless noted.

- `cargo test --lib bridge::`
- `cargo test --lib tui::`
- live: GET http://127.0.0.1:<port>/agents/types

End-of-phase invariant: the repo builds and all suites touched above pass; no half-migrated state.

## Risks and rollback

Auth: the new route must sit behind the same session gate as `/agents/*`. Rollback: revert.

## Open questions this phase depends on

None. (SPEC section 11.)

## Agent prompt

Paste everything inside the block below.

````text
You are implementing phase T04 (GET /agents/types listing the nine code profiles) of the declarative agent definitions plan. Goal: Add the `GET /agents/types` route returning the SPEC 7.5 shape for the existing nine profiles, so later phases (registry, frontend, schema description) have one source to read.

Read first: docs/REPO_MAP.md, docs/specs/agent-definitions/SPEC.md (requirements referenced by P0-4), docs/specs/agent-definitions/phases/T04-agents-types-route.md (your task list, tests, allow-list, verification) and the relevant PLAN.md tasks (P0-4). Prerequisite phases already merged: T02.

Work: 1) write the tests listed under "Tests to write first" and see them fail; 2) do the tasks in order, touching only the allow-listed files; 3) run every command under "Verification commands" and fix until green; 4) commit with explicit `git add <paths>` (1-4 commits). Out of scope: `source`/`state` for declarative agents (T19).; Frontend use (T11).

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
