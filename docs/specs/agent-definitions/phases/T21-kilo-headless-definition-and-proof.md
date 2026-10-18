# T21: Kilo definition (headless), real fixtures, live headless proof

Size: M. Depends on: T20, T05. Covers (PLAN ids): P2-14 (headless half), P2-15. User-visible effect: Kilo headless works from ONE TOML: run_agent + followup_task.

Milestone: ★: Kilo headless works via TOML (first end-to-end value)

## Goal

Ship `kilo.toml` (headless table first), capture real `kilo` 7.0.50 JSON fixtures, assert parsing in tests, and prove `run_agent`/`followup_task` live.

## Why now / depends on

Earliest point the headline value lands: everything needed for headless (schema, templates, parsers, resolver, profile, loader, approvals, routes, receipts) exists. Depends on: T20, T05. Index: `phases/README.md`. Spec: `SPEC.md`; original tasks: `PLAN.md` (P2-14 (headless half), P2-15).

## In scope

- Definition with `[headless]`; `[tui]` omitted or minimal (visible comes in T23-T24).
- Fixtures: success, tool_turn, error, resume, garbage.
- Live proof script run.

## Out of scope

- TUI regexes and visible mode.
- Settings UI.

## Tasks

File allow-list = the union of the files named below (`src/` = `packages/app/src-tauri/src/`). Anything else needs a plan edit.

- [ ] Capture real fixtures (scrub `sessionID`, paths) — `src/tui/declarative/fixtures/kilo/headless_success.jsonl`, `headless_tool_turn.jsonl`, `headless_error.jsonl`, `headless_resume.jsonl`, `headless_garbage.txt` — done-when: files exist; scrub list in commit body
- [ ] Definition — `packages/app/src-tauri/tests/agent-definitions/kilo.toml` (from the AGENT-CHOICE.md draft, updated by T05 results) — done-when: validates with no errors
- [ ] Fixture tests — `src/tui/declarative/tests/kilo.rs` — done-when: assert result, session id, done marker, error message, resume argv
- [ ] Live proof (no source edits): copy kilo.toml to `<app data>/agents/`, approve through T19, run — none (a scripted scenario may be added to `scripts/pm-regression/kilo.mjs`; if so that dir is the allow-list) — done-when: run_agent(kilo, model kilo/kilo-auto/free) returns expected text; followup_task answer depends on turn 1; both receipts carry agent_definition; killing mid-run yields a failed run, not a hang

## Tests to write first

- `declarative::kilo::headless_success_extracts_result_and_session`: values from the real capture
- `declarative::kilo::error_event_maps_to_reported_error`: message preserved
- `declarative::kilo::garbage_returns_none`: unparseable
- `declarative::kilo::resume_argv_contains_session`: -s <id>

## Verification commands

Run from `packages/app/src-tauri` with `CARGO_TARGET_DIR=C:\Users\you\cargo-target-shared` unless noted.

- `cargo test --lib tui::declarative::kilo`
- `cargo test --lib`
- live: run_agent(agent_type=kilo, ...) then followup_task on the handle; inspect receipts
- live: kill kilo.exe mid-run -> run fails

End-of-phase invariant: the repo builds and all suites touched above pass; no half-migrated state.

## Risks and rollback

Kilo free model availability; fixtures must be real not hand-written. Do not edit src-tauri while the live test runs (dev app restarts). Rollback: remove kilo.toml from the app data dir; revert commit.

## Open questions this phase depends on

SPEC 11 Q1 (bundled vs user-dir): default user-dir only; not blocking. (SPEC section 11.)

## Agent prompt

Paste everything inside the block below.

````text
You are implementing phase T21 (Kilo definition (headless), real fixtures, live headless proof) of the declarative agent definitions plan. Goal: Ship `kilo.toml` (headless table first), capture real `kilo` 7.0.50 JSON fixtures, assert parsing in tests, and prove `run_agent`/`followup_task` live.

Read first: docs/REPO_MAP.md, docs/specs/agent-definitions/SPEC.md (requirements referenced by P2-14 (headless half), P2-15), docs/specs/agent-definitions/phases/T21-kilo-headless-definition-and-proof.md (your task list, tests, allow-list, verification) and the relevant PLAN.md tasks (P2-14 (headless half), P2-15). Prerequisite phases already merged: T20, T05.

Work: 1) write the tests listed under "Tests to write first" and see them fail; 2) do the tasks in order, touching only the allow-listed files; 3) run every command under "Verification commands" and fix until green; 4) commit with explicit `git add <paths>` (1-4 commits). Out of scope: TUI regexes and visible mode.; Settings UI.
Phase note: Fixtures must be REAL captures from kilo 7.0.50, never invented. If kilo behaves differently from the draft, update the definition and note the SPEC 7.2 change for T26.
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
