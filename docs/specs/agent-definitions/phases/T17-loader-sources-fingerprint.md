# T17: Loader, sources, and fingerprint (read-only, nothing registered)

Size: M. Depends on: T16. Covers (PLAN ids): P2-10 (loader half). User-visible effect: none - internal.

## Goal

Scan definition directories per SRC-1..SRC-7, apply precedence/shadowing (REG-7/8), and compute a stable fingerprint, all against temp dirs.

## Why now / depends on

Isolates filesystem logic from the trust logic (T18) so each is reviewable. Depends on: T16. Index: `phases/README.md`. Spec: `SPEC.md`; original tasks: `PLAN.md` (P2-10 (loader half)).

## In scope

- Non-recursive scan, 64-file cap, symlink guard.
- `agents_dir()` helper.
- Fingerprint stable under whitespace/comments, changes with argv/binary/env.

## Out of scope

- Approvals (T18).
- Registry integration (T18).
- Watcher (T33).

## Tasks

File allow-list = the union of the files named below (`src/` = `packages/app/src-tauri/src/`). Anything else needs a plan edit.

- [ ] Loader + sources + precedence — `src/tui/declarative/loader.rs`, `src/tui/declarative/sources.rs`, `src/app_paths.rs` (`agents_dir()`), `src/tui/declarative/tests/loader.rs` — done-when: tests cover non-recursion, 64 files, symlink guard, shadowing; temp dirs only
- [ ] Fingerprint — `src/tui/declarative/fingerprint.rs`, `src/tui/declarative/tests/loader.rs` — done-when: whitespace/comment edit keeps hash; argv/binary/env edit changes it

## Tests to write first

- `declarative::loader::non_recursive_and_capped`: 65th file reported, subdir ignored
- `declarative::loader::symlink_guard`: linked file refused (skip on platforms that cannot create symlinks)
- `declarative::fingerprint::stable_under_whitespace`: equal
- `declarative::fingerprint::changes_with_argv`: different

## Verification commands

Run from `packages/app/src-tauri` with `CARGO_TARGET_DIR=C:\Users\you\cargo-target-shared` unless noted.

- `cargo test --lib tui::declarative::loader`
- `cargo test --lib tui::declarative::fingerprint`
- `cargo test --lib`

End-of-phase invariant: the repo builds and all suites touched above pass; no half-migrated state.

## Risks and rollback

Symlink tests on Windows need privileges: gate them. Never recursive-delete test dirs: use `tempfile` handles. Rollback: revert.

## Open questions this phase depends on

SPEC 11 Q3 (consent keyed by path only): default path-only; not blocking here. (SPEC section 11.)

## Agent prompt

Paste everything inside the block below.

````text
You are implementing phase T17 (Loader, sources, and fingerprint (read-only, nothing registered)) of the declarative agent definitions plan. Goal: Scan definition directories per SRC-1..SRC-7, apply precedence/shadowing (REG-7/8), and compute a stable fingerprint, all against temp dirs.

Read first: docs/REPO_MAP.md, docs/specs/agent-definitions/SPEC.md (requirements referenced by P2-10 (loader half)), docs/specs/agent-definitions/phases/T17-loader-sources-fingerprint.md (your task list, tests, allow-list, verification) and the relevant PLAN.md tasks (P2-10 (loader half)). Prerequisite phases already merged: T16.

Work: 1) write the tests listed under "Tests to write first" and see them fail; 2) do the tasks in order, touching only the allow-listed files; 3) run every command under "Verification commands" and fix until green; 4) commit with explicit `git add <paths>` (1-4 commits). Out of scope: Approvals (T18).; Registry integration (T18).; Watcher (T33).

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
