# T12: Spawn menu and viewers read the registry; Phase 1 docs and gate

Size: M. Depends on: T11, T08, T09, T10. Covers (PLAN ids): P1-7 (consumers half), P1-8, P1-9. User-visible effect: none - UI identical for the nine agents.

Milestone: Phase 1 gate: registry-driven, behaviour identical

## Goal

Make the spawn menu, `TUI_AGENTS`, `ansi.ts`, `TerminalViewport` and standby UI read `agentRegistry`, update docs, rebuild, and prove 28/28.

## Why now / depends on

Closes Phase 1; after this a profile outside the source tree can exist end to end (backend + UI). Depends on: T11, T08, T09, T10. Index: `phases/README.md`. Spec: `SPEC.md`; original tasks: `PLAN.md` (P1-7 (consumers half), P1-8, P1-9).

## In scope

- Consumer switch-over with the compiled list as fallback.
- REPO_MAP and tui/README wording.
- Gate: tests, dist exe, regression.

## Out of scope

- Any declarative code.

## Tasks

File allow-list = the union of the files named below (`src/` = `packages/app/src-tauri/src/`). Anything else needs a plan edit.

- [ ] Switch consumers (find with grep `AGENT_META|AgentTypeSchema|TUI_AGENTS`) — `packages/app/src/lib/ansi.ts`, `packages/app/src/components/TerminalViewport.tsx`, `packages/app/src/lib/standby.ts`, the spawn-menu component(s) the grep finds — done-when: typecheck + tests green; manual screenshot shows the nine agents unchanged
- [ ] Docs — `docs/REPO_MAP.md` (registry row, `tui/` row), `packages/app/src-tauri/src/tui/README.md` ('Adding a TUI' step 1) — done-when: wording matches code
- [ ] Gate: restart app, rebuild dist exe only if a schema changed (none expected: confirm), run regression — no source files — done-when: pm-regression 28/28 on a fresh build_id

## Tests to write first

- `app: spawn menu renders nine entries from fallback and from a mocked registry`: DOM assertions
- `shared tests`: green

## Verification commands

Run from `packages/app/src-tauri` with `CARGO_TARGET_DIR=C:\Users\you\cargo-target-shared` unless noted.

- `npm run typecheck`
- `npm test (shared, app, mcp-server)`
- `cargo test --lib`
- `node scripts/pm-regression.mjs -> 28/28`
- manual: before/after screenshot of the spawn menu

End-of-phase invariant: the repo builds and all suites touched above pass; no half-migrated state.

## Risks and rollback

UI regressions are invisible to unit tests: take a screenshot before and after. Rollback: revert.

## Open questions this phase depends on

None. (SPEC section 11.)

## Agent prompt

Paste everything inside the block below.

````text
You are implementing phase T12 (Spawn menu and viewers read the registry; Phase 1 docs and gate) of the declarative agent definitions plan. Goal: Make the spawn menu, `TUI_AGENTS`, `ansi.ts`, `TerminalViewport` and standby UI read `agentRegistry`, update docs, rebuild, and prove 28/28.

Read first: docs/REPO_MAP.md, docs/specs/agent-definitions/SPEC.md (requirements referenced by P1-7 (consumers half), P1-8, P1-9), docs/specs/agent-definitions/phases/T12-frontend-consumers-and-p1-gate.md (your task list, tests, allow-list, verification) and the relevant PLAN.md tasks (P1-7 (consumers half), P1-8, P1-9). Prerequisite phases already merged: T11, T08, T09, T10.

Work: 1) write the tests listed under "Tests to write first" and see them fail; 2) do the tasks in order, touching only the allow-listed files; 3) run every command under "Verification commands" and fix until green; 4) commit with explicit `git add <paths>` (1-4 commits). Out of scope: Any declarative code.
Phase note: Take a before screenshot of the spawn menu first.
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
