# T37: Final verification (V-1..V-9)

Size: M. Depends on: T26, T27, T28, T30, T32, T36. Covers (PLAN ids): V-1..V-9. User-visible effect: none - verification, docs, memory.

Milestone: ★: SPEC section 12 done-when met

## Goal

Run every suite, rebuild the dist exe, check SCHEMA_DRIFT, run pm-regression, repeat the Kilo proof from a clean state, do the security spot checks, finalize docs and memory, and commit.

## Why now / depends on

Last phase; no new features. Depends on: T26, T27, T28, T30, T32, T36. Index: `phases/README.md`. Spec: `SPEC.md`; original tasks: `PLAN.md` (V-1..V-9).

## In scope

- Verification, doc corrections, memory notes.

## Out of scope

- New features (file an issue/plan edit instead).

## Tasks

File allow-list = the union of the files named below (`src/` = `packages/app/src-tauri/src/`). Anything else needs a plan edit.

- [ ] V-1 suites — none — done-when: all green; counts recorded
- [ ] V-2/V-3 exe rebuild and drift check — none — done-when: new build_id; catalog_version equals shim's
- [ ] V-4 regression — none — done-when: 28/28
- [ ] V-5 clean-state Kilo proof (delete agent-approvals.json only - a single file) — none — done-when: needs_approval -> approve -> headless run+followup (same sessionID) -> visible run+followup -> UNKNOWN_AGENT_TYPE lists valid -> edited argv gives AGENT_DEF_APPROVAL_STALE
- [ ] V-6 security spot checks — none — done-when: project-local OFF by default; hostile project file not loaded; `python`/`cmd` binary rejected; task `-x; rm -rf /` arrives as one argv element; MCP session cannot approve
- [ ] V-7 docs — `docs/REPO_MAP.md`, `packages/app/src-tauri/src/tui/README.md`, `docs/specs/agent-definitions/DEFINITION-FORMAT.md`, `docs/specs/agent-definitions/SPEC.md` (assumptions only) — done-when: examples pass `/agents/definitions/validate`; SPEC assumptions updated with verified facts
- [ ] V-8 memory — `C:\Users\Ren-pc\.claude\projects\C--Users-Ren-pc-Desktop-work2-puppet-master\memory\` (MEMORY.md + one new note) — done-when: registry location, definitions dir, Kilo .exe gotcha, catalog_version pin policy
- [ ] V-9 commit — the files above — done-when: fresh build_id in the message body; nothing unrelated staged

## Tests to write first

- `(verification phase)`: no new tests; failures found here get their own fix commits with tests

## Verification commands

Run from `packages/app/src-tauri` with `CARGO_TARGET_DIR=C:\Users\Ren-pc\cargo-target-shared` unless noted.

- `cargo test --lib`
- `cargo test --lib tui::`
- `cargo test --lib tool_registry`
- `cargo test --lib standby`
- npm test + npm run typecheck in packages/shared, packages/app, packages/mcp-server
- `node scripts/pm-regression.mjs -> 28/28`
- bridge_health: no SCHEMA_DRIFT

End-of-phase invariant: the repo builds and all suites touched above pass; no half-migrated state.

## Risks and rollback

Live-test intermittents (see memory notes): retry once before filing. Deleting agent-approvals.json: delete that single file only, never a directory. Rollback: n/a.

## Open questions this phase depends on

Reports SPEC 11 Q6, Q7 as unscheduled follow-ups. (SPEC section 11.)

## Agent prompt

Paste everything inside the block below.

````text
You are implementing phase T37 (Final verification (V-1..V-9)) of the declarative agent definitions plan. Goal: Run every suite, rebuild the dist exe, check SCHEMA_DRIFT, run pm-regression, repeat the Kilo proof from a clean state, do the security spot checks, finalize docs and memory, and commit.

Read first: docs/REPO_MAP.md, docs/specs/agent-definitions/SPEC.md (requirements referenced by V-1..V-9), docs/specs/agent-definitions/phases/T37-final-verification.md (your task list, tests, allow-list, verification) and the relevant PLAN.md tasks (V-1..V-9). Prerequisite phases already merged: T26, T27, T28, T30, T32, T36.

Work: 1) write the tests listed under "Tests to write first" and see them fail; 2) do the tasks in order, touching only the allow-listed files; 3) run every command under "Verification commands" and fix until green; 4) commit with explicit `git add <paths>` (1-4 commits). Out of scope: New features (file an issue/plan edit instead).
Phase note: Delete only the single file agent-approvals.json, never a directory.
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
