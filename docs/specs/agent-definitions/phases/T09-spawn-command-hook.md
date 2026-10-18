# T09: TuiProfile::spawn_command for non-enum ids

Size: M. Depends on: T08. Covers (PLAN ids): P1-4. User-visible effect: none - internal (nine ids spawn exactly as before).

## Goal

Add `TuiProfile::spawn_command(&SpawnRequest) -> Option<LaunchSpec>` (default `None` = existing `resolve_command`) so a profile outside the enum can spawn a PTY with no `AgentType`.

## Why now / depends on

Last registry plumbing step before any non-code profile can run. Depends on: T08. Index: `phases/README.md`. Spec: `SPEC.md`; original tasks: `PLAN.md` (P1-4).

## In scope

- Trait method and `LaunchSpec`/`SpawnRequest` types.
- Spawn path uses it when `Some`.

## Out of scope

- Declarative argv rendering (T14/T16).
- Resolver.

## Tasks

File allow-list = the union of the files named below (`src/` = `packages/app/src-tauri/src/`). Anything else needs a plan edit.

- [ ] Add types and trait default — `src/tui/types.rs`, `src/tui/profile.rs` — done-when: nine impls untouched and compile
- [ ] Use in the spawn path — `src/pty/agents.rs`, `src/pty/registry/spawn.rs`, `src/pty/registry/command.rs` — done-when: a test profile spawns without `AgentType`
- [ ] Regression for code ids — `src/pty/registry/tests.rs` — done-when: nine ids produce the same command as before (snapshot)

## Tests to write first

- `pty::registry::test_profile_spawns_without_agent_type`: spawns a trivial echo process via spawn_command
- `pty::registry::code_profiles_command_unchanged`: argv/env snapshot equals pre-change values

## Verification commands

Run from `packages/app/src-tauri` with `CARGO_TARGET_DIR=C:\Users\you\cargo-target-shared` unless noted.

- `cargo test --lib pty::`
- `cargo test --lib`
- live: pm-regression subset (spawn/close scenarios) or scripts/spawn-close-agent.py

End-of-phase invariant: the repo builds and all suites touched above pass; no half-migrated state.

## Risks and rollback

Windows PTY spawn is sensitive; keep the code path untouched for `None`. Rollback: revert.

## Open questions this phase depends on

None. (SPEC section 11.)

## Agent prompt

Paste everything inside the block below.

````text
You are implementing phase T09 (TuiProfile::spawn_command for non-enum ids) of the declarative agent definitions plan. Goal: Add `TuiProfile::spawn_command(&SpawnRequest) -> Option<LaunchSpec>` (default `None` = existing `resolve_command`) so a profile outside the enum can spawn a PTY with no `AgentType`.

Read first: docs/REPO_MAP.md, docs/specs/agent-definitions/SPEC.md (requirements referenced by P1-4), docs/specs/agent-definitions/phases/T09-spawn-command-hook.md (your task list, tests, allow-list, verification) and the relevant PLAN.md tasks (P1-4). Prerequisite phases already merged: T08.

Work: 1) write the tests listed under "Tests to write first" and see them fail; 2) do the tasks in order, touching only the allow-listed files; 3) run every command under "Verification commands" and fix until green; 4) commit with explicit `git add <paths>` (1-4 commits). Out of scope: Declarative argv rendering (T14/T16).; Resolver.

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
