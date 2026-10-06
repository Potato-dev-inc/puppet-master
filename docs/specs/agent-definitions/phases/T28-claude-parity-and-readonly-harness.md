# T28: Claude parity test and live read-only harness

Size: M. Depends on: T16, T21. Covers (PLAN ids): P3-3, P3-4. User-visible effect: none - tests/scripts only.

## Goal

Prove a declarative profile reproduces the Claude headless contract on existing Claude fixtures (gaps documented), and add a live script that fails if a `read_only_headless` agent writes a file.

## Why now / depends on

Confidence that the engine is as strict as code profiles; SEC-8 harness mandatory for bundled definitions (SPEC 11 Q4 default yes). Depends on: T16, T21. Index: `phases/README.md`. Spec: `SPEC.md`; original tasks: `PLAN.md` (P3-3, P3-4).

## In scope

- Parity over `extract_result/provider_session/model/headless_result_ready` (and Codex json_lines if tests allow).
- `readonly-check.mjs`.

## Out of scope

- Changing code profiles (document gaps instead).

## Tasks

File allow-list = the union of the files named below (`src/` = `packages/app/src-tauri/src/`). Anything else needs a plan edit.

- [ ] Parity test + test-only definition — `src/tui/declarative/tests/parity_claude.rs`, `src/tui/declarative/fixtures/definitions/claude-parity.toml` — done-when: same outputs as `tui::claude::Claude` on its headless test strings; gaps listed in the test
- [ ] Read-only live harness — `scripts/pm-regression/readonly-check.mjs` (new) — done-when: creates-file task under read_only=true fails the script if the file appears; documented

## Tests to write first

- `declarative::parity_claude::matches_code_profile_on_fixtures`: equality or documented gap

## Verification commands

Run from `packages/app/src-tauri` with `CARGO_TARGET_DIR=C:\Users\Ren-pc\cargo-target-shared` unless noted.

- `cargo test --lib tui::declarative::parity`
- `node scripts/pm-regression/readonly-check.mjs --agent claude (live)`

End-of-phase invariant: the repo builds and all suites touched above pass; no half-migrated state.

## Risks and rollback

Parity tests can overfit; document gaps. Rollback: revert.

## Open questions this phase depends on

SPEC 11 Q4: default yes for bundled; not blocking. (SPEC section 11.)

## Agent prompt

Paste everything inside the block below.

````text
You are implementing phase T28 (Claude parity test and live read-only harness) of the declarative agent definitions plan. Goal: Prove a declarative profile reproduces the Claude headless contract on existing Claude fixtures (gaps documented), and add a live script that fails if a `read_only_headless` agent writes a file.

Read first: docs/REPO_MAP.md, docs/specs/agent-definitions/SPEC.md (requirements referenced by P3-3, P3-4), docs/specs/agent-definitions/phases/T28-claude-parity-and-readonly-harness.md (your task list, tests, allow-list, verification) and the relevant PLAN.md tasks (P3-3, P3-4). Prerequisite phases already merged: T16, T21.

Work: 1) write the tests listed under "Tests to write first" and see them fail; 2) do the tasks in order, touching only the allow-listed files; 3) run every command under "Verification commands" and fix until green; 4) commit with explicit `git add <paths>` (1-4 commits). Out of scope: Changing code profiles (document gaps instead).

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
