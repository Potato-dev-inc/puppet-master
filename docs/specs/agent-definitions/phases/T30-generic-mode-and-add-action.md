# T30: Generic zero-config mode and 'Add as generic agent'

Size: M. Depends on: T22, T26. Covers (PLAN ids): P4-1, P4-2. User-visible effect: Try any CLI in 30 seconds: Settings creates an unapproved generic definition.

Milestone: ★: generic try-any-CLI

## Goal

Support `kind = "generic"` definitions (visible-only, experimental, shared heuristics) and an Add-as-generic action that writes `<app data>/agents/<id>.toml` requiring approval.

## Why now / depends on

Cheap extra value on top of the TUI engine and Settings page. Depends on: T22, T26. Index: `phases/README.md`. Spec: `SPEC.md`; original tasks: `PLAN.md` (P4-1, P4-2).

## In scope

- Generic profile mode (GEN-1..3).
- UI action writing the file, not auto-approved.

## Out of scope

- Recorder (T31).

## Tasks

File allow-list = the union of the files named below (`src/` = `packages/app/src-tauri/src/`). Anything else needs a plan edit.

- [ ] Generic mode — `src/tui/declarative/mod.rs`, `src/tui/declarative/schema.rs`, `src/tui/declarative/validate.rs`, `src/tui/declarative/tests/generic.rs` — done-when: 3-line definition yields a visible-only profile; dispatchable only when experimental; receipts show result_inferred and the experimental warning
- [ ] Add action — `packages/app/src/components/settings/tabs/AgentsTab.tsx`, `src/commands/agents.rs`, `src/tui/declarative/loader.rs` — done-when: file created from a command name; NOT auto-approved (test)

## Tests to write first

- `declarative::generic::three_line_definition`: profile built
- `declarative::generic::requires_experimental_for_dispatch`: AGENT_NOT_DISPATCHABLE otherwise
- `AgentsTab add generic creates unapproved file`: state unapproved

## Verification commands

Run from `packages/app/src-tauri` with `CARGO_TARGET_DIR=C:\Users\Ren-pc\cargo-target-shared` unless noted.

- `cargo test --lib tui::declarative::generic`
- `npm test (packages/app)`

End-of-phase invariant: the repo builds and all suites touched above pass; no half-migrated state.

## Risks and rollback

Heuristics may mis-read screens: hence experimental flag. Rollback: revert.

## Open questions this phase depends on

None. (SPEC section 11.)

## Agent prompt

Paste everything inside the block below.

````text
You are implementing phase T30 (Generic zero-config mode and 'Add as generic agent') of the declarative agent definitions plan. Goal: Support `kind = "generic"` definitions (visible-only, experimental, shared heuristics) and an Add-as-generic action that writes `<app data>/agents/<id>.toml` requiring approval.

Read first: docs/REPO_MAP.md, docs/specs/agent-definitions/SPEC.md (requirements referenced by P4-1, P4-2), docs/specs/agent-definitions/phases/T30-generic-mode-and-add-action.md (your task list, tests, allow-list, verification) and the relevant PLAN.md tasks (P4-1, P4-2). Prerequisite phases already merged: T22, T26.

Work: 1) write the tests listed under "Tests to write first" and see them fail; 2) do the tasks in order, touching only the allow-listed files; 3) run every command under "Verification commands" and fix until green; 4) commit with explicit `git add <paths>` (1-4 commits). Out of scope: Recorder (T31).

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
