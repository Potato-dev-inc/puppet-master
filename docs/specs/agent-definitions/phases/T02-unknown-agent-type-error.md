# T02: UNKNOWN_AGENT_TYPE error and shared lookup helper

Size: S. Depends on: none. Covers (PLAN ids): P0-3. User-visible effect: none - internal (new error code for bad ids; INVALID_AGENT_TYPE kept on delegate_work).

## Goal

Add `tui::require_registered(id)` and the HTTP 400 `UNKNOWN_AGENT_TYPE` error (with `context.valid`) at every id gate, so the schema can later stop enumerating ids.

## Why now / depends on

The catalog enum (T03) cannot be loosened until the server rejects bad ids with a useful error. Depends on: none. Index: `phases/README.md`. Spec: `SPEC.md`; original tasks: `PLAN.md` (P0-3).

## In scope

- `require_registered` returning the profile or `UnknownAgentType{valid}`.
- Use it at the run/spawn/delegate gates.
- Keep `INVALID_AGENT_TYPE` on the `delegate_work` path with identical body.

## Out of scope

- Registry/Arc (T06+).
- Replacing `AgentType::parse` (T08) - the gates here still sit on the enum if they do today.
- Schema changes (T03).

## Tasks

File allow-list = the union of the files named below (`src/` = `packages/app/src-tauri/src/`). Anything else needs a plan edit.

- [ ] Add `UnknownAgentType` + `require_registered` — `src/tui/mod.rs` — done-when: unit test returns valid ids list
- [ ] Map to HTTP 400 `UNKNOWN_AGENT_TYPE` with `context.valid` — `src/bridge/errors.rs`, `src/operations/` (only the file holding `OperationError`) — done-when: error shape asserted
- [ ] Use at gate sites — `src/bridge/dispatch.rs`, `src/bridge/routes/panes.rs`, `src/pty/registry/spawn.rs` — done-when: gates call the helper; delegate path still emits INVALID_AGENT_TYPE
- [ ] Bridge tests for run / spawn / delegate — `src/bridge/tests.rs` — done-when: three paths covered

## Tests to write first

- `bridge::run_agent_unknown_type_returns_unknown_agent_type`: 400, code UNKNOWN_AGENT_TYPE, context.valid lists the nine ids
- `bridge::spawn_unknown_type_returns_unknown_agent_type`: same for the spawn route
- `bridge::delegate_work_unknown_type_keeps_invalid_agent_type`: old code and body unchanged

## Verification commands

Run from `packages/app/src-tauri` with `CARGO_TARGET_DIR=C:\Users\you\cargo-target-shared` unless noted.

- `cargo test --lib bridge::`
- `cargo test --lib tui::`
- `cargo test --lib agent_runs`

End-of-phase invariant: the repo builds and all suites touched above pass; no half-migrated state.

## Risks and rollback

Clients matching on the old code: only delegate_work keeps it. Rollback: revert.

## Open questions this phase depends on

None. (SPEC section 11.)

## Agent prompt

Paste everything inside the block below.

````text
You are implementing phase T02 (UNKNOWN_AGENT_TYPE error and shared lookup helper) of the declarative agent definitions plan. Goal: Add `tui::require_registered(id)` and the HTTP 400 `UNKNOWN_AGENT_TYPE` error (with `context.valid`) at every id gate, so the schema can later stop enumerating ids.

Read first: docs/REPO_MAP.md, docs/specs/agent-definitions/SPEC.md (requirements referenced by P0-3), docs/specs/agent-definitions/phases/T02-unknown-agent-type-error.md (your task list, tests, allow-list, verification) and the relevant PLAN.md tasks (P0-3). Prerequisite phases already merged: none.

Work: 1) write the tests listed under "Tests to write first" and see them fail; 2) do the tasks in order, touching only the allow-listed files; 3) run every command under "Verification commands" and fix until green; 4) commit with explicit `git add <paths>` (1-4 commits). Out of scope: Registry/Arc (T06+).; Replacing `AgentType::parse` (T08) - the gates here still sit on the enum if they do today.; Schema changes (T03).

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
