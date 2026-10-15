# T08: Registry lookup replaces AgentType::parse gates

Size: M. Depends on: T07, T02. Covers (PLAN ids): P1-3. User-visible effect: none - internal (persisted runs for removed agents degrade to agent_gone).

## Goal

Every validity gate asks the registry instead of `AgentType::parse`; `AgentType` survives only in `pty/agents.rs` and the code-profile launch path; persisted-run reload tolerates an unregistered id.

## Why now / depends on

Required before a non-enum id can pass any gate; split from T07 so the Arc migration stays mechanical. Depends on: T07, T02. Index: `phases/README.md`. Spec: `SPEC.md`; original tasks: `PLAN.md` (P1-3).

## In scope

- Gates in spawn/dispatch/panes/agent_models/persist/inspect/conformance.

## Out of scope

- Spawning non-enum ids (T09).
- agent_contexts strings (T10).

## Tasks

File allow-list = the union of the files named below (`src/` = `packages/app/src-tauri/src/`). Anything else needs a plan edit.

- [ ] Replace gates with registry lookups (`require_registered` from T02) — `src/pty/registry/spawn.rs`, `src/bridge/dispatch.rs`, `src/bridge/routes/panes.rs`, `src/commands/agent_models.rs`, `src/agent_contexts/inspect.rs` — done-when: grep `AgentType::parse` outside pty/agents.rs shows only the launch path
- [ ] Persisted-run reload tolerates an unregistered id (`agent_gone`) — `src/agent_runs/persist.rs` — done-when: test: reload a snapshot with id `zzz` does not panic and reports agent_gone
- [ ] Conformance: 'every code profile id resolves through the registry' — `src/tui/conformance.rs` — done-when: rule replaces the `AgentType::parse` check

## Tests to write first

- `agent_runs::persist::reload_with_unregistered_agent_type_is_agent_gone`: no panic, status explained
- `tui::conformance::every_code_id_resolves_in_registry`: all nine ids resolve
- `bridge::gates_use_registry`: a registered test-only profile passes the gate (inject into the registry in the test)

## Verification commands

Run from `packages/app/src-tauri` with `CARGO_TARGET_DIR=C:\Users\Ren-pc\cargo-target-shared` unless noted.

- `cargo test --lib agent_runs`
- `cargo test --lib bridge::`
- `cargo test --lib tui::`
- `cargo test --lib`

End-of-phase invariant: the repo builds and all suites touched above pass; no half-migrated state.

## Risks and rollback

A gate missed leaves an enum-only path (caught by T09's test). Rollback: revert.

## Open questions this phase depends on

None. (SPEC section 11.)

## Agent prompt

Paste everything inside the block below.

````text
You are implementing phase T08 (Registry lookup replaces AgentType::parse gates) of the declarative agent definitions plan. Goal: Every validity gate asks the registry instead of `AgentType::parse`; `AgentType` survives only in `pty/agents.rs` and the code-profile launch path; persisted-run reload tolerates an unregistered id.

Read first: docs/REPO_MAP.md, docs/specs/agent-definitions/SPEC.md (requirements referenced by P1-3), docs/specs/agent-definitions/phases/T08-registry-gates-replace-agenttype-parse.md (your task list, tests, allow-list, verification) and the relevant PLAN.md tasks (P1-3). Prerequisite phases already merged: T07, T02.

Work: 1) write the tests listed under "Tests to write first" and see them fail; 2) do the tasks in order, touching only the allow-listed files; 3) run every command under "Verification commands" and fix until green; 4) commit with explicit `git add <paths>` (1-4 commits). Out of scope: Spawning non-enum ids (T09).; agent_contexts strings (T10).

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
