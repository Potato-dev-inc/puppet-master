# T31: Recorder core with redaction

Size: M. Depends on: T12. Covers (PLAN ids): P4-3. User-visible effect: none visible (no UI yet; engine only).

## Goal

Add the recorder module: per-pane start/stop, snapshots on idle/working/prompt transitions, and strict redaction, never running on standby panes.

## Why now / depends on

Independent of declarative work beyond the registry; isolating it lets the redaction tests be exhaustive. Depends on: T12. Index: `phases/README.md`. Spec: `SPEC.md`; original tasks: `PLAN.md` (P4-3).

## In scope

- `agent_definitions_recorder/` module; tap in `pty/registry/mod.rs` (read-only screen text).

## Out of scope

- Draft generation (T32).
- UI (T32).

## Tasks

File allow-list = the union of the files named below (`src/` = `packages/app/src-tauri/src/`). Anything else needs a plan edit.

- [ ] Recorder + redaction — `src/agent_definitions_recorder/mod.rs`, `redact.rs`, `snapshots.rs`, `tests.rs` (all new), `src/lib.rs` (mod decl), `src/pty/registry/mod.rs` (tap point) — done-when: start/stop per pane; redaction tests for env values, `sk-...`, `ghp_...`, `AKIA...`, 40+ char base64/hex, home path; never runs on standby panes (REC-5)

## Tests to write first

- `recorder::redact::table`: each secret shape replaced
- `recorder::standby_pane_never_recorded`: REC-5
- `recorder::snapshot_on_transitions`: idle/working/prompt

## Verification commands

Run from `packages/app/src-tauri` with `CARGO_TARGET_DIR=C:\Users\you\cargo-target-shared` unless noted.

- `cargo test --lib agent_definitions_recorder`
- `cargo test --lib pty::`

End-of-phase invariant: the repo builds and all suites touched above pass; no half-migrated state.

## Risks and rollback

Redaction gaps leak secrets: err toward over-redaction; raw captures kept in memory only until the app closes (SPEC 11 Q8 default). Rollback: revert.

## Open questions this phase depends on

SPEC 11 Q8 retention: default until app closes; not blocking. (SPEC section 11.)

## Agent prompt

Paste everything inside the block below.

````text
You are implementing phase T31 (Recorder core with redaction) of the declarative agent definitions plan. Goal: Add the recorder module: per-pane start/stop, snapshots on idle/working/prompt transitions, and strict redaction, never running on standby panes.

Read first: docs/REPO_MAP.md, docs/specs/agent-definitions/SPEC.md (requirements referenced by P4-3), docs/specs/agent-definitions/phases/T31-recorder-core.md (your task list, tests, allow-list, verification) and the relevant PLAN.md tasks (P4-3). Prerequisite phases already merged: T12.

Work: 1) write the tests listed under "Tests to write first" and see them fail; 2) do the tasks in order, touching only the allow-listed files; 3) run every command under "Verification commands" and fix until green; 4) commit with explicit `git add <paths>` (1-4 commits). Out of scope: Draft generation (T32).; UI (T32).

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
