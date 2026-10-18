# T15: JSON selector engine and the three output parsers

Size: M. Depends on: T13. Covers (PLAN ids): P2-4, P2-5. User-visible effect: none - internal.

## Goal

Implement `where/where_regex/~exists/pick/regex/trim` selectors and the `plain_text`, `json_single`, `json_lines` parsers with size caps and exit-code handling.

## Why now / depends on

This is how a declarative profile reads an agent's headless output; pure and fixture-driven, parallel to T14. Depends on: T13. Index: `phases/README.md`. Spec: `SPEC.md`; original tasks: `PLAN.md` (P2-4, P2-5).

## In scope

- Selector engine with depth cap 64 and 1 MiB line safety.
- Parsers for result, provider session, model, ready marker, rejected-call warning, error selector.

## Out of scope

- Kilo fixtures (T21).
- Profile wiring (T16).

## Tasks

File allow-list = the union of the files named below (`src/` = `packages/app/src-tauri/src/`). Anything else needs a plan edit.

- [ ] Selector engine — `src/tui/declarative/select.rs`, `src/tui/declarative/tests/select.rs` — done-when: 30-case table incl. deeply nested JSON and a 1 MiB line; garbage and missing paths return `None`
- [ ] Output parsers wired to extract_result/provider_session/model/headless_result_ready/rejected_call_warning and the error selector — `src/tui/declarative/output.rs`, `src/tui/declarative/tests/output.rs` — done-when: exit != 0 returns `None`; `max_output_bytes` enforced; ANSI stripped; `unparseable_lines` counted

## Tests to write first

- `declarative::select::table_30_cases`: documented inputs and expected picks
- `declarative::output::nonzero_exit_returns_none`: no result on failure
- `declarative::output::max_output_bytes_enforced`: oversize input truncated or rejected per SPEC
- `declarative::output::ansi_stripped_and_garbage_counted`: unparseable_lines == n

## Verification commands

Run from `packages/app/src-tauri` with `CARGO_TARGET_DIR=C:\Users\you\cargo-target-shared` unless noted.

- `cargo test --lib tui::declarative::select`
- `cargo test --lib tui::declarative::output`
- `cargo test --lib`

End-of-phase invariant: the repo builds and all suites touched above pass; no half-migrated state.

## Risks and rollback

Regex/JSON DoS: keep caps in tests. Rollback: revert.

## Open questions this phase depends on

None. (SPEC section 11.)

## Agent prompt

Paste everything inside the block below.

````text
You are implementing phase T15 (JSON selector engine and the three output parsers) of the declarative agent definitions plan. Goal: Implement `where/where_regex/~exists/pick/regex/trim` selectors and the `plain_text`, `json_single`, `json_lines` parsers with size caps and exit-code handling.

Read first: docs/REPO_MAP.md, docs/specs/agent-definitions/SPEC.md (requirements referenced by P2-4, P2-5), docs/specs/agent-definitions/phases/T15-selectors-and-output-parsers.md (your task list, tests, allow-list, verification) and the relevant PLAN.md tasks (P2-4, P2-5). Prerequisite phases already merged: T13.

Work: 1) write the tests listed under "Tests to write first" and see them fail; 2) do the tasks in order, touching only the allow-listed files; 3) run every command under "Verification commands" and fix until green; 4) commit with explicit `git add <paths>` (1-4 commits). Out of scope: Kilo fixtures (T21).; Profile wiring (T16).

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
