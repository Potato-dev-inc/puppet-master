# T27: *.expect.toml fixtures and in-app 'Check with fixtures'

Size: M. Depends on: T21, T22, T26. Covers (PLAN ids): P3-1, P3-2. User-visible effect: Authors can verify a definition against a folder of captures without cargo.

## Goal

Let each fixture carry a sibling `*.expect.toml` walked by one generic test, and expose the same runner through `agents_check_file(path, fixtures_dir?)` and a button in Settings.

## Why now / depends on

Makes adding fixtures Rust-free and gives authors a UI check; depends on real Kilo fixtures and the Settings page. Depends on: T21, T22, T26. Index: `phases/README.md`. Spec: `SPEC.md`; original tasks: `PLAN.md` (P3-1, P3-2).

## In scope

- Expectation format and runner.
- Tauri command and button.

## Out of scope

- Parity tests (T28).

## Tasks

File allow-list = the union of the files named below (`src/` = `packages/app/src-tauri/src/`). Anything else needs a plan edit.

- [ ] Expectation runner — `src/tui/declarative/tests/expect.rs`, `src/tui/declarative/fixtures/**/*.expect.toml`, `docs/specs/agent-definitions/DEFINITION-FORMAT.md` (section 10) — done-when: one test walks every definition and its fixtures; Kilo fixtures gain expect files
- [ ] Command + button — `src/commands/agents.rs`, `packages/app/src/components/settings/tabs/AgentsTab.tsx` — done-when: per-fixture pass/fail displayed

## Tests to write first

- `declarative::expect::all_fixtures_meet_expectations`: walk and assert
- `declarative::expect::missing_expect_file_is_reported`: clear failure
- `AgentsTab check button shows per-fixture results`: mocked command

## Verification commands

Run from `packages/app/src-tauri` with `CARGO_TARGET_DIR=C:\Users\Ren-pc\cargo-target-shared` unless noted.

- `cargo test --lib tui::declarative::expect`
- `npm test (packages/app)`

End-of-phase invariant: the repo builds and all suites touched above pass; no half-migrated state.

## Risks and rollback

Runner reused by command must not read outside the given dir. Rollback: revert.

## Open questions this phase depends on

None. (SPEC section 11.)

## Agent prompt

Paste everything inside the block below.

````text
You are implementing phase T27 (*.expect.toml fixtures and in-app 'Check with fixtures') of the declarative agent definitions plan. Goal: Let each fixture carry a sibling `*.expect.toml` walked by one generic test, and expose the same runner through `agents_check_file(path, fixtures_dir?)` and a button in Settings.

Read first: docs/REPO_MAP.md, docs/specs/agent-definitions/SPEC.md (requirements referenced by P3-1, P3-2), docs/specs/agent-definitions/phases/T27-fixture-expectations-and-author-check.md (your task list, tests, allow-list, verification) and the relevant PLAN.md tasks (P3-1, P3-2). Prerequisite phases already merged: T21, T22, T26.

Work: 1) write the tests listed under "Tests to write first" and see them fail; 2) do the tasks in order, touching only the allow-listed files; 3) run every command under "Verification commands" and fix until green; 4) commit with explicit `git add <paths>` (1-4 commits). Out of scope: Parity tests (T28).

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
