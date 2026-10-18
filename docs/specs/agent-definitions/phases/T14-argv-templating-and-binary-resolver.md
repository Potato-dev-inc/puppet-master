# T14: Argv templating and binary resolver

Size: M. Depends on: T13. Covers (PLAN ids): P2-3, P2-6. User-visible effect: none - internal.

## Goal

Add the injection-proof argv renderer and the Windows-aware binary resolver as pure/unit-tested modules.

## Why now / depends on

Both are small pure modules that block the first runnable profile (T16) and the Kilo proof; independent of each other so one agent can do both in a day. Depends on: T13. Index: `phases/README.md`. Spec: `SPEC.md`; original tasks: `PLAN.md` (P2-3, P2-6).

## In scope

- `render(def, HeadlessLaunch) -> Vec<String>`.
- `resolve_binary` (PATH scan excluding cwd/project dir, Windows `.exe` > shim glob > refuse).

## Out of scope

- Wiring into the profile (T16).
- stdin task delivery for shim-only installs (SPEC 11 Q2: refuse for now).

## Tasks

File allow-list = the union of the files named below (`src/` = `packages/app/src-tauri/src/`). Anything else needs a plan edit.

- [ ] Argv renderer: placeholders per element, `when` segments in declaration order, `task_separator`/`task_flag` — `src/tui/declarative/template.rs`, `src/tui/declarative/tests/template.rs` — done-when: property test: a task with spaces, quotes, `;`, `&&`, newline, leading `-`, 100 KiB stays exactly one argv element and never changes other elements
- [ ] Binary resolver + `resolve_binary` next to `executable` (existing fns untouched) — `src/tui/declarative/resolve.rs`, `src/tui/mod.rs`, `src/tui/declarative/tests/resolve.rs` — done-when: temp-dir tree mimicking `%APPDATA%\npm\node_modules\@kilocode\cli\node_modules\@kilocode\cli-windows-x64\bin\kilo.exe` resolves; shim-only returns `AGENT_DEF_SHIM_NEEDS_NATIVE`; cwd/project dir excluded
- [ ] Live check on this machine — none — done-when: resolver finds the real `kilo.exe` (record the path in the commit body)

## Tests to write first

- `declarative::template::task_is_one_argv_element (property)`: hostile task strings stay one element
- `declarative::template::when_segments_in_order`: model/resume/allow_shell ordering
- `declarative::resolve::windows_exe_beats_shim`: native exe preferred
- `declarative::resolve::shim_only_is_refused`: error code
- `declarative::resolve::cwd_not_searched`: planted binary in cwd ignored

## Verification commands

Run from `packages/app/src-tauri` with `CARGO_TARGET_DIR=C:\Users\you\cargo-target-shared` unless noted.

- `cargo test --lib tui::declarative::template`
- `cargo test --lib tui::declarative::resolve`
- `cargo test --lib tui::`
- live: resolver test against %APPDATA%\npm (ignored-by-default test or a one-off command)

End-of-phase invariant: the repo builds and all suites touched above pass; no half-migrated state.

## Risks and rollback

Resolver safety: never search cwd or project dir (SEC). Windows path quirks (spaces, 8.3 names). Rollback: revert.

## Open questions this phase depends on

SPEC 11 Q2 (shim-only installs): default is refuse; not blocking. (SPEC section 11.)

## Agent prompt

Paste everything inside the block below.

````text
You are implementing phase T14 (Argv templating and binary resolver) of the declarative agent definitions plan. Goal: Add the injection-proof argv renderer and the Windows-aware binary resolver as pure/unit-tested modules.

Read first: docs/REPO_MAP.md, docs/specs/agent-definitions/SPEC.md (requirements referenced by P2-3, P2-6), docs/specs/agent-definitions/phases/T14-argv-templating-and-binary-resolver.md (your task list, tests, allow-list, verification) and the relevant PLAN.md tasks (P2-3, P2-6). Prerequisite phases already merged: T13.

Work: 1) write the tests listed under "Tests to write first" and see them fail; 2) do the tasks in order, touching only the allow-listed files; 3) run every command under "Verification commands" and fix until green; 4) commit with explicit `git add <paths>` (1-4 commits). Out of scope: Wiring into the profile (T16).; stdin task delivery for shim-only installs (SPEC 11 Q2: refuse for now).
Phase note: Use only temp dirs in tests; never recursive-delete anything outside your own tempdir handle.
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
