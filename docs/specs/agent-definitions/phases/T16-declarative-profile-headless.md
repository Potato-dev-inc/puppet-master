# T16: DeclarativeProfile implementing TuiProfile (headless side)

Size: M. Depends on: T14, T15, T09. Covers (PLAN ids): P2-8 (headless half). User-visible effect: none - internal (constructible in tests, not loaded).

## Goal

Build `DeclarativeProfile` mapping definition tables onto `TuiProfile`: features, run_options, headless launch via T14, parsing via T15, `is_installed()` via the resolver, `spawn_command` via T09; TUI methods use trait defaults for now.

## Why now / depends on

First runnable profile; testable by constructing it from a TOML string in a unit test, before any file loading or approval exists. Depends on: T14, T15, T09. Index: `phases/README.md`. Spec: `SPEC.md`; original tasks: `PLAN.md` (P2-8 (headless half)).

## In scope

- Every trait method maps to a table or to the default.
- Shared `headless_launch(id, ..)` policy applies.

## Out of scope

- TUI tables (T22).
- Loader, approvals, registry load (T17-T19).

## Tasks

File allow-list = the union of the files named below (`src/` = `packages/app/src-tauri/src/`). Anything else needs a plan edit.

- [ ] Implement the profile — `src/tui/declarative/mod.rs`, `src/tui/types.rs` (only if a field is missing) — done-when: builds; headless methods behave per SPEC
- [ ] Profile tests using inline TOML — `src/tui/declarative/tests/profile.rs` — done-when: features, run_options, `accepts_headless_model`, `standby_support`, `capability_notes`, `is_installed()` behave per SPEC

## Tests to write first

- `declarative::profile::features_follow_definition`: headless/visible flags match tables
- `declarative::profile::headless_launch_renders_argv`: model/resume/read_only placement
- `declarative::profile::tui_methods_default_when_tui_absent`: defaults, no panics

## Verification commands

Run from `packages/app/src-tauri` with `CARGO_TARGET_DIR=C:\Users\Ren-pc\cargo-target-shared` unless noted.

- `cargo test --lib tui::declarative`
- `cargo test --lib`

End-of-phase invariant: the repo builds and all suites touched above pass; no half-migrated state.

## Risks and rollback

A trait method silently mapped wrongly; the T22 conformance suite is the backstop. Rollback: revert.

## Open questions this phase depends on

None. (SPEC section 11.)

## Agent prompt

Paste everything inside the block below.

````text
You are implementing phase T16 (DeclarativeProfile implementing TuiProfile (headless side)) of the declarative agent definitions plan. Goal: Build `DeclarativeProfile` mapping definition tables onto `TuiProfile`: features, run_options, headless launch via T14, parsing via T15, `is_installed()` via the resolver, `spawn_command` via T09; TUI methods use trait defaults for now.

Read first: docs/REPO_MAP.md, docs/specs/agent-definitions/SPEC.md (requirements referenced by P2-8 (headless half)), docs/specs/agent-definitions/phases/T16-declarative-profile-headless.md (your task list, tests, allow-list, verification) and the relevant PLAN.md tasks (P2-8 (headless half)). Prerequisite phases already merged: T14, T15, T09.

Work: 1) write the tests listed under "Tests to write first" and see them fail; 2) do the tasks in order, touching only the allow-listed files; 3) run every command under "Verification commands" and fix until green; 4) commit with explicit `git add <paths>` (1-4 commits). Out of scope: TUI tables (T22).; Loader, approvals, registry load (T17-T19).

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
