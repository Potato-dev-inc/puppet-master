# T07: Return Arc from profile_for and fix callers

Size: M. Depends on: T06. Covers (PLAN ids): P1-2. User-visible effect: none - internal.

## Goal

Switch `profile_for`/`profiles()` to the registry (`Arc<dyn TuiProfile>`) and mechanically fix every caller that `cargo check` flags, with no logic changes.

## Why now / depends on

The widest compile-fix surface (~50 sites); isolating it keeps the diff reviewable and behaviour-neutral. Depends on: T06. Index: `phases/README.md`. Spec: `SPEC.md`; original tasks: `PLAN.md` (P1-2).

## In scope

- Return-type change; deref/borrow fixes only.

## Out of scope

- Replacing `AgentType::parse` gates (T08).
- Any behaviour change.

## Tasks

File allow-list = the union of the files named below (`src/` = `packages/app/src-tauri/src/`). Anything else needs a plan edit.

- [ ] Change `profile_for` and `profiles()` to read the registry snapshot — `src/tui/mod.rs`, `src/tui/registry.rs` — done-when: compiles in tui/
- [ ] Fix flagged callers (allow-list is exactly these dirs) — `src/agent_runs/`, `src/bridge/`, `src/pane_wait/`, `src/pty/`, `src/standby/`, `src/agent_contexts/`, `src/commands/` — done-when: `cargo check` and `cargo test --lib` green
- [ ] Hot-path sanity test — `src/tui/registry.rs` (tests) — done-when: repeated `profile_for` returns the same Arc until a swap

## Tests to write first

- `existing suites unchanged`: no test edited except mechanical type fixes
- `tui::registry::profile_for_returns_same_arc_until_swap`: pointer equality between calls

## Verification commands

Run from `packages/app/src-tauri` with `CARGO_TARGET_DIR=C:\Users\you\cargo-target-shared` unless noted.

- `cargo test --lib`
- `cargo check --all-targets`

End-of-phase invariant: the repo builds and all suites touched above pass; no half-migrated state.

## Risks and rollback

Hot-path cost (profile_for is called from pane loops): keep it a single atomic load. Rollback: revert.

## Open questions this phase depends on

None. (SPEC section 11.)

## Agent prompt

Paste everything inside the block below.

````text
You are implementing phase T07 (Return Arc from profile_for and fix callers) of the declarative agent definitions plan. Goal: Switch `profile_for`/`profiles()` to the registry (`Arc<dyn TuiProfile>`) and mechanically fix every caller that `cargo check` flags, with no logic changes.

Read first: docs/REPO_MAP.md, docs/specs/agent-definitions/SPEC.md (requirements referenced by P1-2), docs/specs/agent-definitions/phases/T07-arc-callers-migration.md (your task list, tests, allow-list, verification) and the relevant PLAN.md tasks (P1-2). Prerequisite phases already merged: T06.

Work: 1) write the tests listed under "Tests to write first" and see them fail; 2) do the tasks in order, touching only the allow-listed files; 3) run every command under "Verification commands" and fix until green; 4) commit with explicit `git add <paths>` (1-4 commits). Out of scope: Replacing `AgentType::parse` gates (T08).; Any behaviour change.
Phase note: Do not change logic. If a fix needs more than a deref/clone, stop and ask.
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
