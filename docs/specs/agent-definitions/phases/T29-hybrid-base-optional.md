# T29: OPTIONAL: hybrid base = code profile

Size: S. Depends on: T16, T13. Covers (PLAN ids): P3-5. User-visible effect: optional: a TOML can override only data fields of a code profile.

Milestone: optional

**STATUS: BLOCKED on an unanswered open question (see below). Do not start.**

## Goal

Allow `base = "<code profile id>"` where only data fields override and everything else delegates to the base.

## Why now / depends on

Only if SPEC 11 Q5 is answered yes. Depends on: T16, T13. Index: `phases/README.md`. Spec: `SPEC.md`; original tasks: `PLAN.md` (P3-5).

## In scope

- Schema, validation, delegation, tests.

## Out of scope

- Anything beyond data-field overrides.

## Tasks

File allow-list = the union of the files named below (`src/` = `packages/app/src-tauri/src/`). Anything else needs a plan edit.

- [ ] Hybrid support — `src/tui/declarative/schema.rs`, `src/tui/declarative/mod.rs`, `src/tui/declarative/validate.rs`, `src/tui/declarative/tests/hybrid.rs`, `docs/specs/agent-definitions/DEFINITION-FORMAT.md` — done-when: hybrid of `claude` with another binary name works; `base` on a non-code id is AGENT_DEF_RESERVED_ID

## Tests to write first

- `declarative::hybrid::claude_with_other_binary`: delegates non-data methods
- `declarative::hybrid::non_code_base_rejected`: RESERVED_ID

## Verification commands

Run from `packages/app/src-tauri` with `CARGO_TARGET_DIR=C:\Users\you\cargo-target-shared` unless noted.

- `cargo test --lib tui::declarative::hybrid`

End-of-phase invariant: the repo builds and all suites touched above pass; no half-migrated state.

## Risks and rollback

Scope creep. Rollback: revert.

## Open questions this phase depends on

BLOCKED on SPEC 11 Q5 (is hybrid wanted this iteration?). Do not start until the user answers yes. (SPEC section 11.)

## Agent prompt

Paste everything inside the block below.

````text
You are implementing phase T29 (OPTIONAL: hybrid base = code profile) of the declarative agent definitions plan. Goal: Allow `base = "<code profile id>"` where only data fields override and everything else delegates to the base.

Read first: docs/REPO_MAP.md, docs/specs/agent-definitions/SPEC.md (requirements referenced by P3-5), docs/specs/agent-definitions/phases/T29-hybrid-base-optional.md (your task list, tests, allow-list, verification) and the relevant PLAN.md tasks (P3-5). Prerequisite phases already merged: T16, T13.

Work: 1) write the tests listed under "Tests to write first" and see them fail; 2) do the tasks in order, touching only the allow-listed files; 3) run every command under "Verification commands" and fix until green; 4) commit with explicit `git add <paths>` (1-4 commits). Out of scope: Anything beyond data-field overrides.
Phase note: Do not start unless the caller confirms SPEC 11 Q5 is answered yes.
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
