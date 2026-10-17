# T25: Settings > Agents page (list, errors, reload, approve)

Size: M. Depends on: T19. Covers (PLAN ids): P2-13 (first half). User-visible effect: Settings > Agents lists definitions with source badge/state/errors, Reload, and an Approve dialog showing the rendered argv.

## Goal

Add the Settings tab: list definitions with source badge and state, show errors with code/file/line, a Reload button, and the Approve dialog with rendered argv and warnings (APR-3).

## Why now / depends on

Frontend only on top of T19; can run in parallel with T21-T24. Depends on: T19. Index: `phases/README.md`. Spec: `SPEC.md`; original tasks: `PLAN.md` (P2-13 (first half)).

## In scope

- List, errors, Reload, Approve/Revoke dialog.
- UI-4 test: `<img onerror>` label is inert.

## Out of scope

- Source toggles, extra dirs, consent toggles (T26).
- Diff view (T34).

## Tasks

File allow-list = the union of the files named below (`src/` = `packages/app/src-tauri/src/`). Anything else needs a plan edit.

- [ ] Component + wiring (follow the Performance tab pattern from commit 1b52915: `settings/tabs/` and `settings-tab-panels.tsx`) — `packages/app/src/components/settings/tabs/AgentsTab.tsx` (new), `packages/app/src/components/settings/settings-tab-panels.tsx`, `packages/app/src/lib/agentDefinitions.ts` (new) — done-when: tab renders list from a mocked command
- [ ] Commands (if any missing) — `src/commands/agents.rs` — done-when: reload/approve/revoke exposed
- [ ] Component tests — `packages/app/src/components/settings/tabs/AgentsTab.test.tsx` — done-when: list, errors, approve dialog showing argv, HTML-injection test

## Tests to write first

- `AgentsTab renders source badge and state`: per fixture list
- `AgentsTab approve dialog shows rendered argv and warnings`: text present
- `AgentsTab escapes hostile label`: no <img> element created

## Verification commands

Run from `packages/app/src-tauri` with `CARGO_TARGET_DIR=C:\Users\you\cargo-target-shared` unless noted.

- `npm run typecheck`
- `npm test (packages/app)`
- manual: screenshot of the tab with kilo.toml present

End-of-phase invariant: the repo builds and all suites touched above pass; no half-migrated state.

## Risks and rollback

XSS through labels: render as text only. Rollback: revert.

## Open questions this phase depends on

None. (SPEC section 11.)

## Agent prompt

Paste everything inside the block below.

````text
You are implementing phase T25 (Settings > Agents page (list, errors, reload, approve)) of the declarative agent definitions plan. Goal: Add the Settings tab: list definitions with source badge and state, show errors with code/file/line, a Reload button, and the Approve dialog with rendered argv and warnings (APR-3).

Read first: docs/REPO_MAP.md, docs/specs/agent-definitions/SPEC.md (requirements referenced by P2-13 (first half)), docs/specs/agent-definitions/phases/T25-settings-agents-page.md (your task list, tests, allow-list, verification) and the relevant PLAN.md tasks (P2-13 (first half)). Prerequisite phases already merged: T19.

Work: 1) write the tests listed under "Tests to write first" and see them fail; 2) do the tasks in order, touching only the allow-listed files; 3) run every command under "Verification commands" and fix until green; 4) commit with explicit `git add <paths>` (1-4 commits). Out of scope: Source toggles, extra dirs, consent toggles (T26).; Diff view (T34).

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
