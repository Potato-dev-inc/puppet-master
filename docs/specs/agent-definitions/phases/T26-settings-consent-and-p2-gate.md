# T26: Settings source toggles / consent / extra dirs; Phase 2 docs and gate

Size: M. Depends on: T25, T24. Covers (PLAN ids): P2-13 (second half), P2-17, P2-18. User-visible effect: Source toggles, extra directories and consent toggles in the UI; docs describe the feature.

Milestone: Phase 2 gate: Kilo headless + visible, 28/28

## Goal

Finish the Agents page (source toggles, extra dir add/remove, consent toggles), update docs to the implemented code, rebuild the dist exe, and repeat the Kilo live proof on a fresh build_id.

## Why now / depends on

Closes Phase 2 (the minimum path to the stated done-when). Depends on: T25, T24. Index: `phases/README.md`. Spec: `SPEC.md`; original tasks: `PLAN.md` (P2-13 (second half), P2-17, P2-18).

## In scope

- UI controls on existing routes.
- Docs: REPO_MAP, tui/README 'Adding an agent without code', DEFINITION-FORMAT diff against reality.
- Gate.

## Out of scope

- Project-consent banner flow (T34).

## Tasks

File allow-list = the union of the files named below (`src/` = `packages/app/src-tauri/src/`). Anything else needs a plan edit.

- [ ] Toggles and extra dirs — `packages/app/src/components/settings/tabs/AgentsTab.tsx`, `packages/app/src/lib/agentDefinitions.ts`, `src/commands/agents.rs`, component tests — done-when: each control calls its command; tests green
- [ ] Docs — `docs/REPO_MAP.md`, `packages/app/src-tauri/src/tui/README.md`, `docs/specs/agent-definitions/DEFINITION-FORMAT.md` — done-when: describe implemented code (verify by running examples through validate)
- [ ] Gate — no source files — done-when: dist exe rebuilt if any schema changed (confirm); pm-regression 28/28; Kilo headless + visible repeated on a fresh build_id

## Tests to write first

- `AgentsTab source toggle / extra dir / consent controls`: invoke the right command with the right args

## Verification commands

Run from `packages/app/src-tauri` with `CARGO_TARGET_DIR=C:\Users\Ren-pc\cargo-target-shared` unless noted.

- `npm run typecheck`
- `npm test`
- `cargo test --lib`
- `node scripts/pm-regression.mjs -> 28/28`
- live: Kilo headless + visible

End-of-phase invariant: the repo builds and all suites touched above pass; no half-migrated state.

## Risks and rollback

Doc drift vs code: validate DEFINITION-FORMAT examples through `/agents/definitions/validate`. Rollback: revert.

## Open questions this phase depends on

None. (SPEC section 11.)

## Agent prompt

Paste everything inside the block below.

````text
You are implementing phase T26 (Settings source toggles / consent / extra dirs; Phase 2 docs and gate) of the declarative agent definitions plan. Goal: Finish the Agents page (source toggles, extra dir add/remove, consent toggles), update docs to the implemented code, rebuild the dist exe, and repeat the Kilo live proof on a fresh build_id.

Read first: docs/REPO_MAP.md, docs/specs/agent-definitions/SPEC.md (requirements referenced by P2-13 (second half), P2-17, P2-18), docs/specs/agent-definitions/phases/T26-settings-consent-and-p2-gate.md (your task list, tests, allow-list, verification) and the relevant PLAN.md tasks (P2-13 (second half), P2-17, P2-18). Prerequisite phases already merged: T25, T24.

Work: 1) write the tests listed under "Tests to write first" and see them fail; 2) do the tasks in order, touching only the allow-listed files; 3) run every command under "Verification commands" and fix until green; 4) commit with explicit `git add <paths>` (1-4 commits). Out of scope: Project-consent banner flow (T34).

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
