# T36: Export/import, Phase 5 docs, Phase 5 gate

Size: M. Depends on: T34, T35. Covers (PLAN ids): P5-6, P5-7, P5-8. User-visible effect: Export a definition (+fixtures zip) and import into the user dir unapproved.

Milestone: Phase 5 gate: live edit during a run is safe

## Goal

Add export/import of one definition (no network), update docs for watcher/consent/writer, and verify that editing `kilo.toml` during a run leaves it unaffected and requires re-approval for new runs.

## Why now / depends on

Closes the trust phases. Depends on: T34, T35. Index: `phases/README.md`. Spec: `SPEC.md`; original tasks: `PLAN.md` (P5-6, P5-7, P5-8).

## In scope

- Export/import commands and UI.
- Docs.
- Live edit-during-run check.

## Out of scope

- Any network fetch.

## Tasks

File allow-list = the union of the files named below (`src/` = `packages/app/src-tauri/src/`). Anything else needs a plan edit.

- [ ] Sharing — `src/commands/agents.rs`, `packages/app/src/components/settings/tabs/AgentsTab.tsx`, `src/tui/declarative/loader.rs` — done-when: export writes TOML (+ fixtures zip) to a user-chosen file; import validates and places it UNAPPROVED; no network
- [ ] Docs — `docs/REPO_MAP.md`, `packages/app/src-tauri/src/tui/README.md`, `docs/specs/agent-definitions/DEFINITION-FORMAT.md` — done-when: reflect watcher, consent, writer
- [ ] Gate — no source files — done-when: pm-regression 28/28; live: edit kilo.toml mid-run -> run unaffected; next run blocked until re-approval

## Tests to write first

- `declarative::share::import_is_unapproved`: state
- `AgentsTab export/import buttons`: invoke commands

## Verification commands

Run from `packages/app/src-tauri` with `CARGO_TARGET_DIR=C:\Users\Ren-pc\cargo-target-shared` unless noted.

- `cargo test --lib`
- `npm test`
- `node scripts/pm-regression.mjs -> 28/28`
- live: edit-during-run check

End-of-phase invariant: the repo builds and all suites touched above pass; no half-migrated state.

## Risks and rollback

Zip handling: write only the chosen target file. Rollback: revert.

## Open questions this phase depends on

None. (SPEC section 11.)

## Agent prompt

Paste everything inside the block below.

````text
You are implementing phase T36 (Export/import, Phase 5 docs, Phase 5 gate) of the declarative agent definitions plan. Goal: Add export/import of one definition (no network), update docs for watcher/consent/writer, and verify that editing `kilo.toml` during a run leaves it unaffected and requires re-approval for new runs.

Read first: docs/REPO_MAP.md, docs/specs/agent-definitions/SPEC.md (requirements referenced by P5-6, P5-7, P5-8), docs/specs/agent-definitions/phases/T36-sharing-and-p5-gate.md (your task list, tests, allow-list, verification) and the relevant PLAN.md tasks (P5-6, P5-7, P5-8). Prerequisite phases already merged: T34, T35.

Work: 1) write the tests listed under "Tests to write first" and see them fail; 2) do the tasks in order, touching only the allow-listed files; 3) run every command under "Verification commands" and fix until green; 4) commit with explicit `git add <paths>` (1-4 commits). Out of scope: Any network fetch.

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
