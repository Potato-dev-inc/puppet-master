# T18: Approvals store and startup registry integration

Size: M. Depends on: T17. Covers (PLAN ids): P2-10 (approvals half), P2-11 (registry half). User-visible effect: a TOML in the user agents dir becomes a registered agent once its approval record exists (no UI/route yet).

## Goal

Persist approvals in `agent-approvals.json` (same load/save/bad-file semantics as `standby/config.rs`) and load approved definitions into the registry at startup; unapproved/stale ones stay unregistered with the right state.

## Why now / depends on

After this the backend path to a registered declarative agent exists; routes (T19) and UI (T25) only add control surfaces. Depends on: T17. Index: `phases/README.md`. Spec: `SPEC.md`; original tasks: `PLAN.md` (P2-10 (approvals half), P2-11 (registry half)).

## In scope

- `approvals.rs` store + states (`unapproved`, `approved`, `approval_stale`).
- Startup load + `reload()` building a new registry snapshot.

## Out of scope

- HTTP routes (T19).
- UI.
- Hot reload (T33).

## Tasks

File allow-list = the union of the files named below (`src/` = `packages/app/src-tauri/src/`). Anything else needs a plan edit.

- [ ] Approvals store — `src/tui/declarative/approvals.rs`, `src/tui/declarative/tests/loader.rs` — done-when: persist/load; bad file means empty (test); stale on fingerprint change
- [ ] Registry integration: load at startup, `reload()` swaps the snapshot — `src/tui/registry.rs`, `src/tui/declarative/loader.rs`, `src/lib.rs` (one init call) — done-when: a test definition + approval record registers; without the record it does not and state is `unapproved`

## Tests to write first

- `declarative::approvals::bad_file_means_empty`: no panic, empty set
- `declarative::approvals::stale_blocks_registration`: state approval_stale, not in registry
- `tui::registry::reload_adds_and_removes_declarative`: code profiles remain first and unchanged

## Verification commands

Run from `packages/app/src-tauri` with `CARGO_TARGET_DIR=C:\Users\Ren-pc\cargo-target-shared` unless noted.

- `cargo test --lib tui::`
- `cargo test --lib`

End-of-phase invariant: the repo builds and all suites touched above pass; no half-migrated state.

## Risks and rollback

Registration order must keep code profiles first (read_only_backends order). Rollback: revert.

## Open questions this phase depends on

None. (SPEC section 11.)

## Agent prompt

Paste everything inside the block below.

````text
You are implementing phase T18 (Approvals store and startup registry integration) of the declarative agent definitions plan. Goal: Persist approvals in `agent-approvals.json` (same load/save/bad-file semantics as `standby/config.rs`) and load approved definitions into the registry at startup; unapproved/stale ones stay unregistered with the right state.

Read first: docs/REPO_MAP.md, docs/specs/agent-definitions/SPEC.md (requirements referenced by P2-10 (approvals half), P2-11 (registry half)), docs/specs/agent-definitions/phases/T18-approvals-and-registry-load.md (your task list, tests, allow-list, verification) and the relevant PLAN.md tasks (P2-10 (approvals half), P2-11 (registry half)). Prerequisite phases already merged: T17.

Work: 1) write the tests listed under "Tests to write first" and see them fail; 2) do the tasks in order, touching only the allow-listed files; 3) run every command under "Verification commands" and fix until green; 4) commit with explicit `git add <paths>` (1-4 commits). Out of scope: HTTP routes (T19).; UI.; Hot reload (T33).

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
