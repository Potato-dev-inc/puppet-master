# T24: Live visible Kilo proof

Size: S. Depends on: T23. Covers (PLAN ids): P2-16. User-visible effect: Kilo visible panes work: run_agent headless=false, followup, permission answering.

Milestone: ★: Kilo visible works via TOML

## Goal

Prove on the live bridge that `headless=false` creates a Kilo pane, delivers the task, returns a (result_inferred) result, supports followup_task, and answers a permission dialog via `answer_prompt` without `STALE_PROMPT`.

## Why now / depends on

Completes the stated definition of done for Kilo; separated from T23 so any regex fixes are small commits. Depends on: T23. Index: `phases/README.md`. Spec: `SPEC.md`; original tasks: `PLAN.md` (P2-16).

## In scope

- Live test; regex fixes in kilo.toml/fixtures if the live run exposes mismatches.

## Out of scope

- Settings UI.

## Tasks

File allow-list = the union of the files named below (`src/` = `packages/app/src-tauri/src/`). Anything else needs a plan edit.

- [ ] Run the visible scenario (optionally scripted as `scripts/pm-regression/kilo.mjs`) — `scripts/pm-regression/` (only if scripting) — done-when: all four SPEC 12 item-4 behaviours observed
- [ ] Fix any definition mismatches found — `packages/app/src-tauri/tests/agent-definitions/kilo.toml`, `src/tui/declarative/fixtures/kilo/*` — done-when: tests still green

## Tests to write first

- `(live) kilo visible run`: task delivered, result_inferred, followup works, permission answered with no STALE_PROMPT

## Verification commands

Run from `packages/app/src-tauri` with `CARGO_TARGET_DIR=C:\Users\you\cargo-target-shared` unless noted.

- live: run_agent(agent_type=kilo, headless=false, ...); followup_task; trigger a permission dialog; answer_prompt
- `cargo test --lib tui::declarative::kilo`

End-of-phase invariant: the repo builds and all suites touched above pass; no half-migrated state.

## Risks and rollback

Known intermittent live-test flakiness (see memory notes): retry before assuming a regression. Do not edit src-tauri during the live run. Rollback: revert regex edits.

## Open questions this phase depends on

None. (SPEC section 11.)

## Agent prompt

Paste everything inside the block below.

````text
You are implementing phase T24 (Live visible Kilo proof) of the declarative agent definitions plan. Goal: Prove on the live bridge that `headless=false` creates a Kilo pane, delivers the task, returns a (result_inferred) result, supports followup_task, and answers a permission dialog via `answer_prompt` without `STALE_PROMPT`.

Read first: docs/REPO_MAP.md, docs/specs/agent-definitions/SPEC.md (requirements referenced by P2-16), docs/specs/agent-definitions/phases/T24-kilo-visible-live-proof.md (your task list, tests, allow-list, verification) and the relevant PLAN.md tasks (P2-16). Prerequisite phases already merged: T23.

Work: 1) write the tests listed under "Tests to write first" and see them fail; 2) do the tasks in order, touching only the allow-listed files; 3) run every command under "Verification commands" and fix until green; 4) commit with explicit `git add <paths>` (1-4 commits). Out of scope: Settings UI.
Phase note: If verifies_prompt_delivery cannot be made to work, turn it off in the definition and prove delivery through the result; record the decision.
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
