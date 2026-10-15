# T22: TUI table engine and conformance over definitions

Size: M. Depends on: T16. Covers (PLAN ids): P2-7, P2-9. User-visible effect: none - internal (visible-mode machinery, tested on synthetic definitions).

## Goal

Implement the `[tui]` table engine (chrome, tool-activity, menu, dialogs, busy marker, screen classification, footer model, prompts, standby screen, clear-input) and extend `conformance.rs` to run every rule over code profiles AND test definitions.

## Why now / depends on

Needed for visible mode and as the safety net for every future definition. Can run in parallel with T17-T21 (touches `screen.rs`, `mod.rs`, `conformance.rs`). Depends on: T16. Index: `phases/README.md`. Spec: `SPEC.md`; original tasks: `PLAN.md` (P2-7, P2-9).

## In scope

- All TUI trait methods mapped to tables; 256 KiB screen cap; idle windows clamped (TUI-4).
- Test definitions: kilo-like, minimal-tui, generic; a deliberately bad set.

## Out of scope

- Real Kilo regexes from PTY captures (T23).
- Live visible proof (T24).

## Tasks

File allow-list = the union of the files named below (`src/` = `packages/app/src-tauri/src/`). Anything else needs a plan edit.

- [ ] Table engine — `src/tui/declarative/screen.rs`, `src/tui/declarative/mod.rs` (wiring), `src/tui/declarative/tests/screen.rs` — done-when: each method has a table-driven test; `broad`/`allow_broad` semantics identical to `generic::prompt_reply`
- [ ] Conformance over definitions — `src/tui/conformance.rs`, `src/tui/declarative/tests/conformance.rs`, `src/tui/declarative/fixtures/definitions/*.toml`, `src/tui/declarative/fixtures/definitions-bad/*.toml` — done-when: every conformance rule iterates code profiles and every test definition; each bad definition fails the rules it should

## Tests to write first

- `declarative::screen::busy_marker_detects_working_not_idle`: synthetic screens
- `declarative::conformance::extract_result_rejects_garbage_and_failed_exits`: per definition
- `declarative::conformance::prompts_round_trip`: detect then answer
- `declarative::conformance::empty_screen_never_blocked`: idle bounds respected
- `declarative::conformance::bad_definitions_fail_expected_rules`: each bad file triggers its rule

## Verification commands

Run from `packages/app/src-tauri` with `CARGO_TARGET_DIR=C:\Users\Ren-pc\cargo-target-shared` unless noted.

- `cargo test --lib tui::declarative`
- `cargo test --lib tui::`
- `cargo test --lib`

End-of-phase invariant: the repo builds and all suites touched above pass; no half-migrated state.

## Risks and rollback

Over-fitting synthetic screens to the engine: real captures arrive in T23. Rollback: revert.

## Open questions this phase depends on

None. (SPEC section 11.)

## Agent prompt

Paste everything inside the block below.

````text
You are implementing phase T22 (TUI table engine and conformance over definitions) of the declarative agent definitions plan. Goal: Implement the `[tui]` table engine (chrome, tool-activity, menu, dialogs, busy marker, screen classification, footer model, prompts, standby screen, clear-input) and extend `conformance.rs` to run every rule over code profiles AND test definitions.

Read first: docs/REPO_MAP.md, docs/specs/agent-definitions/SPEC.md (requirements referenced by P2-7, P2-9), docs/specs/agent-definitions/phases/T22-tui-table-engine-and-conformance.md (your task list, tests, allow-list, verification) and the relevant PLAN.md tasks (P2-7, P2-9). Prerequisite phases already merged: T16.

Work: 1) write the tests listed under "Tests to write first" and see them fail; 2) do the tasks in order, touching only the allow-listed files; 3) run every command under "Verification commands" and fix until green; 4) commit with explicit `git add <paths>` (1-4 commits). Out of scope: Real Kilo regexes from PTY captures (T23).; Live visible proof (T24).
Phase note: Parallel-safe with T17-T21 only if you rebase carefully on `decl/mod.rs`.
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
