# T23: Real Kilo PTY captures and [tui] tables

Size: M. Depends on: T22, T21. Covers (PLAN ids): P2-14 (TUI half). User-visible effect: none visible yet (visible mode proven in T24).

## Goal

Capture real Kilo TUI screens through the app's pane tooling, save them as fixtures, and replace the guessed `[tui]` regexes in `kilo.toml` (and SPEC 7.2) with ones the captures prove.

## Why now / depends on

The visible-TUI regexes in the draft are guesses; this phase turns them into evidence before any live visible test. Depends on: T22, T21. Index: `phases/README.md`. Spec: `SPEC.md`; original tasks: `PLAN.md` (P2-14 (TUI half)).

## In scope

- Fixtures: `tui_idle`, `tui_working`, `tui_permission`, `tui_reply`.
- TUI assertions on chrome/busy/prompt.
- SPEC 7.2 / DEFINITION-FORMAT Kilo example updated if regexes change.

## Out of scope

- Live run_agent visible proof (T24).

## Tasks

File allow-list = the union of the files named below (`src/` = `packages/app/src-tauri/src/`). Anything else needs a plan edit.

- [ ] Capture via a spawned Kilo pane (`read_terminal_buffer` or the recorder tap if present) — `src/tui/declarative/fixtures/kilo/tui_idle.txt`, `tui_working.txt`, `tui_permission.txt`, `tui_reply.txt` — done-when: four real captures exist; secrets/paths scrubbed
- [ ] Update `[tui]` tables — `packages/app/src-tauri/tests/agent-definitions/kilo.toml` — done-when: validates
- [ ] TUI tests on captures — `src/tui/declarative/tests/kilo.rs` — done-when: busy matches working not idle; permission detected; reply tidy keeps only the answer; conformance still green
- [ ] Sync docs if regexes changed — `docs/specs/agent-definitions/SPEC.md` (7.2 only), `docs/specs/agent-definitions/DEFINITION-FORMAT.md` (section 12 only) — done-when: examples equal kilo.toml

## Tests to write first

- `declarative::kilo::tui_busy_matches_working_not_idle`: captures
- `declarative::kilo::tui_permission_prompt_detected`: kind + keys
- `declarative::kilo::tui_reply_tidied`: chrome stripped

## Verification commands

Run from `packages/app/src-tauri` with `CARGO_TARGET_DIR=C:\Users\Ren-pc\cargo-target-shared` unless noted.

- `cargo test --lib tui::declarative`
- `cargo test --lib tui::`

End-of-phase invariant: the repo builds and all suites touched above pass; no half-migrated state.

## Risks and rollback

Kilo TUI may change across versions: record the version in the fixture header. Rollback: revert.

## Open questions this phase depends on

None. (SPEC section 11.)

## Agent prompt

Paste everything inside the block below.

````text
You are implementing phase T23 (Real Kilo PTY captures and [tui] tables) of the declarative agent definitions plan. Goal: Capture real Kilo TUI screens through the app's pane tooling, save them as fixtures, and replace the guessed `[tui]` regexes in `kilo.toml` (and SPEC 7.2) with ones the captures prove.

Read first: docs/REPO_MAP.md, docs/specs/agent-definitions/SPEC.md (requirements referenced by P2-14 (TUI half)), docs/specs/agent-definitions/phases/T23-kilo-tui-captures-and-tables.md (your task list, tests, allow-list, verification) and the relevant PLAN.md tasks (P2-14 (TUI half)). Prerequisite phases already merged: T22, T21.

Work: 1) write the tests listed under "Tests to write first" and see them fail; 2) do the tasks in order, touching only the allow-listed files; 3) run every command under "Verification commands" and fix until green; 4) commit with explicit `git add <paths>` (1-4 commits). Out of scope: Live run_agent visible proof (T24).
Phase note: Captures must come from the real TUI. If the TUI cannot be driven, stop and ask.
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
