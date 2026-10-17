# T33: Hot reload watcher and agent_definitions_changed event

Size: M. Depends on: T18, T19, T26. Covers (PLAN ids): P5-1, P5-2. User-visible effect: Saving a definition updates the registry within 1 s; UI refreshes without polling; standby workers invalidated on fingerprint change.

## Goal

Watch enabled source dirs with debounce and atomic registry swap, emit `agent_definitions_changed` (SSE + Tauri) with the diff, and drop an agent's standby workers when its fingerprint changes.

## Why now / depends on

Live iteration for authors; builds on the loader/approvals and Settings page. Depends on: T18, T19, T26. Index: `phases/README.md`. Spec: `SPEC.md`; original tasks: `PLAN.md` (P5-1, P5-2).

## In scope

- `notify` watcher, debounce, atomic swap.
- SSE/Tauri event.
- Standby invalidation.

## Out of scope

- Consent flows (T34).

## Tasks

File allow-list = the union of the files named below (`src/` = `packages/app/src-tauri/src/`). Anything else needs a plan edit.

- [ ] Watcher — `src/tui/declarative/watch.rs`, `src/tui/declarative/loader.rs`, `src/tui/registry.rs`, `packages/app/src-tauri/Cargo.toml` (`notify` line only), `src/tui/declarative/tests/watch.rs` — done-when: save -> registry updated within 1 s; half-written file keeps previous snapshot; temp/rename saves handled; never reads outside enabled dirs
- [ ] Event + standby — `src/bridge/sse.rs`, `src/standby/pool.rs`, `src/standby/tests.rs`, `packages/app/src/lib/agentRegistry.ts` — done-when: event carries diff; changed fingerprint discards that agent's standby workers (fake host test); UI refreshes without polling

## Tests to write first

- `declarative::watch::save_updates_within_1s`: temp dir
- `declarative::watch::half_written_keeps_previous`: no registry change
- `standby::fingerprint_change_discards_workers`: fake host

## Verification commands

Run from `packages/app/src-tauri` with `CARGO_TARGET_DIR=C:\Users\you\cargo-target-shared` unless noted.

- `cargo test --lib tui::declarative::watch`
- `cargo test --lib standby`
- `npm test (packages/app)`

End-of-phase invariant: the repo builds and all suites touched above pass; no half-migrated state.

## Risks and rollback

Windows file locks and editor temp files cause flakiness: use retry/debounce. Cargo.toml has user edits (git add -p). Rollback: revert.

## Open questions this phase depends on

None. (SPEC section 11.)

## Agent prompt

Paste everything inside the block below.

````text
You are implementing phase T33 (Hot reload watcher and agent_definitions_changed event) of the declarative agent definitions plan. Goal: Watch enabled source dirs with debounce and atomic registry swap, emit `agent_definitions_changed` (SSE + Tauri) with the diff, and drop an agent's standby workers when its fingerprint changes.

Read first: docs/REPO_MAP.md, docs/specs/agent-definitions/SPEC.md (requirements referenced by P5-1, P5-2), docs/specs/agent-definitions/phases/T33-watcher-and-change-events.md (your task list, tests, allow-list, verification) and the relevant PLAN.md tasks (P5-1, P5-2). Prerequisite phases already merged: T18, T19, T26.

Work: 1) write the tests listed under "Tests to write first" and see them fail; 2) do the tasks in order, touching only the allow-listed files; 3) run every command under "Verification commands" and fix until green; 4) commit with explicit `git add <paths>` (1-4 commits). Out of scope: Consent flows (T34).
Phase note: Cargo.toml has unrelated user edits: stage only your `notify` line with `git add -p`.
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
