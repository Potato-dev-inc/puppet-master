# T35: Declarative [mcp_host] writer

Size: M. Depends on: T13, T19. Covers (PLAN ids): P5-5. User-visible effect: A definition can register Puppet Master's MCP server in another host's config file.

## Goal

Implement JSON/TOML/YAML writers driven by `[mcp_host]` (key path, `.bak`, home/project-only paths, idempotent) and validate the table.

## Why now / depends on

Self-contained; touches `mcp_install` and validation only. Depends on: T13, T19. Index: `phases/README.md`. Spec: `SPEC.md`; original tasks: `PLAN.md` (P5-5).

## In scope

- Writers and tests on temp files.

## Out of scope

- Changing existing claude/codex/opencode writers.

## Tasks

File allow-list = the union of the files named below (`src/` = `packages/app/src-tauri/src/`). Anything else needs a plan edit.

- [ ] Writers — `src/tui/declarative/schema.rs`, `src/tui/declarative/validate.rs`, `src/mcp_install/declarative.rs` (new), `src/mcp_install/mod.rs`, `src/mcp_install/tests.rs` — done-when: fake-host definition writes the right entry into a temp file; refuses paths outside home/project (HST-3); idempotent on re-run; `.bak` created; existing writers untouched

## Tests to write first

- `mcp_install::declarative::json_toml_yaml_writers`: temp file entries
- `mcp_install::declarative::refuses_outside_home`: HST-3
- `mcp_install::declarative::idempotent`: second run no diff

## Verification commands

Run from `packages/app/src-tauri` with `CARGO_TARGET_DIR=C:\Users\Ren-pc\cargo-target-shared` unless noted.

- `cargo test --lib mcp_install`
- `cargo test --lib tui::declarative`

End-of-phase invariant: the repo builds and all suites touched above pass; no half-migrated state.

## Risks and rollback

Writing to user config files: temp-dir tests only, never touch real configs in tests. Rollback: revert.

## Open questions this phase depends on

None. (SPEC section 11.)

## Agent prompt

Paste everything inside the block below.

````text
You are implementing phase T35 (Declarative [mcp_host] writer) of the declarative agent definitions plan. Goal: Implement JSON/TOML/YAML writers driven by `[mcp_host]` (key path, `.bak`, home/project-only paths, idempotent) and validate the table.

Read first: docs/REPO_MAP.md, docs/specs/agent-definitions/SPEC.md (requirements referenced by P5-5), docs/specs/agent-definitions/phases/T35-declarative-mcp-host-writer.md (your task list, tests, allow-list, verification) and the relevant PLAN.md tasks (P5-5). Prerequisite phases already merged: T13, T19.

Work: 1) write the tests listed under "Tests to write first" and see them fail; 2) do the tasks in order, touching only the allow-listed files; 3) run every command under "Verification commands" and fix until green; 4) commit with explicit `git add <paths>` (1-4 commits). Out of scope: Changing existing claude/codex/opencode writers.
Phase note: Never write to real host config files in tests or live checks.
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
