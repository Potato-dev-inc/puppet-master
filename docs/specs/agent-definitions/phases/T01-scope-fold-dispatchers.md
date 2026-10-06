# T01: Scope the five fold dispatchers to one profile

Size: S. Depends on: none. Covers (PLAN ids): P0-1. User-visible effect: none - internal (fixes cross-profile bleed).

## Goal

Make `tui::is_chrome_line/strip_dialogs/tidy_reply/is_tool_activity_line/looks_like_menu` take an `agent_type` and consult only that profile, so adding many profiles later cannot change another agent's screen handling.

## Why now / depends on

First groundwork step; pure refactor plus one bug fix, no dependency on anything. Must land before any new profile can be registered (SPEC 2.2, done-when 10). Depends on: none. Index: `phases/README.md`. Spec: `SPEC.md`; original tasks: `PLAN.md` (P0-1).

## In scope

- Add `agent_type: &str` to the five free dispatchers and route to `profile_for(agent_type)` only.
- Thread the agent type from the pane / run snapshot to every caller (known: `agent_runs/completion/clean.rs`, `pane_wait/check.rs`).

## Out of scope

- Any registry or `AgentId` work (T06).
- Changing any profile's own rules.
- New MCP schema fields.

## Tasks

File allow-list = the union of the files named below (`src/` = `packages/app/src-tauri/src/`). Anything else needs a plan edit.

- [ ] Inventory callers: grep `tui::(is_chrome_line|strip_dialogs|tidy_reply|is_tool_activity_line|looks_like_menu)` under `src/` and record which have an agent type in scope — none (notes in commit body) — done-when: list written; callers lacking a type are named
- [ ] Change signatures and bodies of the five dispatchers — `src/tui/mod.rs` — done-when: no `PROFILES.iter().any/fold` remains in those five fns
- [ ] Update callers to pass the pane/run agent type (thread it from `PaneInfo`/run snapshot; never default to 'all profiles') — `src/agent_runs/completion/clean.rs`, `src/pane_wait/check.rs`, any other caller the grep finds — done-when: `cargo check` clean
- [ ] Update conformance helpers that call the dispatchers — `src/tui/conformance.rs` — done-when: conformance still green
- [ ] Add the scoping test — `src/tui/mod.rs` (tests) or `src/tui/conformance.rs` — done-when: test below passes

## Tests to write first

- `tui::scoping_chrome_rule_of_a_does_not_touch_b`: a line that profile A treats as chrome is kept by `tidy_reply("<line>", "B")` and dropped for A
- `tui::scoping_strip_dialogs_only_runs_selected_profile`: a dialog block known to Cursor survives `strip_dialogs(text, "claude")`
- `existing Cursor/OpenCode fixture replays`: unchanged outputs (no edits to fixtures)

## Verification commands

Run from `packages/app/src-tauri` with `CARGO_TARGET_DIR=C:\Users\Ren-pc\cargo-target-shared` unless noted.

- `cargo test --lib tui::`
- `cargo test --lib agent_runs::completion`
- `cargo test --lib pane_wait`
- `cargo test --lib`

End-of-phase invariant: the repo builds and all suites touched above pass; no half-migrated state.

## Risks and rollback

A caller with no agent type tempts a fallback to all profiles; do not. Rollback: revert the commit (pure refactor). Cursor/OpenCode screen-cleaning is the most sensitive; fixtures must replay byte-identical.

## Open questions this phase depends on

None. (SPEC section 11.)

## Agent prompt

Paste everything inside the block below.

````text
You are implementing phase T01 (Scope the five fold dispatchers to one profile) of the declarative agent definitions plan. Goal: Make `tui::is_chrome_line/strip_dialogs/tidy_reply/is_tool_activity_line/looks_like_menu` take an `agent_type` and consult only that profile, so adding many profiles later cannot change another agent's screen handling.

Read first: docs/REPO_MAP.md, docs/specs/agent-definitions/SPEC.md (requirements referenced by P0-1), docs/specs/agent-definitions/phases/T01-scope-fold-dispatchers.md (your task list, tests, allow-list, verification) and the relevant PLAN.md tasks (P0-1). Prerequisite phases already merged: none.

Work: 1) write the tests listed under "Tests to write first" and see them fail; 2) do the tasks in order, touching only the allow-listed files; 3) run every command under "Verification commands" and fix until green; 4) commit with explicit `git add <paths>` (1-4 commits). Out of scope: Any registry or `AgentId` work (T06).; Changing any profile's own rules.; New MCP schema fields.
Phase note: Check that each caller really has the agent type before editing; if a caller truly cannot know it, stop and ask rather than defaulting.
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
