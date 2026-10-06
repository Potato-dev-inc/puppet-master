# T19: /agents/definitions routes and Tauri commands, app-session gate

Size: M. Depends on: T18. Covers (PLAN ids): P2-11 (routes half). User-visible effect: definitions can be listed, validated, reloaded, approved and revoked over HTTP / Tauri; GET /agents/types now includes declarative agents.

Milestone: Approve a definition without hand-editing JSON

## Goal

Add `GET /agents/definitions`, `POST .../reload|validate|approve|revoke|consent` per SPEC 7.5, plus thin Tauri command wrappers, with approval restricted to the app session (APR-7).

## Why now / depends on

Makes the declarative backend operable (approve from the webview console or tests) before any Settings UI exists, which unblocks the live Kilo proof (T21). Depends on: T18. Index: `phases/README.md`. Spec: `SPEC.md`; original tasks: `PLAN.md` (P2-11 (routes half)).

## In scope

- Routes + commands + `/agents/types` source/state fields.
- 403 for an MCP session on approve/revoke/consent.

## Out of scope

- Settings UI (T25).
- Project consent flows (T34).

## Tasks

File allow-list = the union of the files named below (`src/` = `packages/app/src-tauri/src/`). Anything else needs a plan edit.

- [ ] Routes — `src/agent_runs/routes.rs` (or `src/bridge/routes/mcp.rs`, wherever T04 placed `/agents` GET), `src/bridge/tests.rs` — done-when: all seven routes behave per SPEC 7.5
- [ ] Tauri wrappers — `src/commands/agents.rs`, `src/lib.rs` (registration lines) — done-when: approve/revoke/reload callable from the webview
- [ ] Auth — `src/mcp_sessions/authorize.rs` only if the gate needs a route-class entry — done-when: MCP session gets 403 on approve (test)

## Tests to write first

- `bridge::agents_definitions_lists_state_and_errors`: unapproved definition listed with state and errors with file/line
- `bridge::approve_requires_app_session`: MCP session -> 403 (APR-7)
- `bridge::agents_types_includes_declarative_after_approval`: source user, state active

## Verification commands

Run from `packages/app/src-tauri` with `CARGO_TARGET_DIR=C:\Users\Ren-pc\cargo-target-shared` unless noted.

- `cargo test --lib bridge::`
- `cargo test --lib tui::`
- `cargo test --lib`

End-of-phase invariant: the repo builds and all suites touched above pass; no half-migrated state.

## Risks and rollback

Auth mistakes are the main risk: this is the trust boundary. Review the gate by test, not by reading. Rollback: revert.

## Open questions this phase depends on

None. (SPEC section 11.)

## Agent prompt

Paste everything inside the block below.

````text
You are implementing phase T19 (/agents/definitions routes and Tauri commands, app-session gate) of the declarative agent definitions plan. Goal: Add `GET /agents/definitions`, `POST .../reload|validate|approve|revoke|consent` per SPEC 7.5, plus thin Tauri command wrappers, with approval restricted to the app session (APR-7).

Read first: docs/REPO_MAP.md, docs/specs/agent-definitions/SPEC.md (requirements referenced by P2-11 (routes half)), docs/specs/agent-definitions/phases/T19-definition-routes-and-commands.md (your task list, tests, allow-list, verification) and the relevant PLAN.md tasks (P2-11 (routes half)). Prerequisite phases already merged: T18.

Work: 1) write the tests listed under "Tests to write first" and see them fail; 2) do the tasks in order, touching only the allow-listed files; 3) run every command under "Verification commands" and fix until green; 4) commit with explicit `git add <paths>` (1-4 commits). Out of scope: Settings UI (T25).; Project consent flows (T34).
Phase note: The approve route is the security boundary: an MCP-session caller must never approve. Write that test first.
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
