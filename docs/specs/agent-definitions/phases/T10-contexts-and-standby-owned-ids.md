# T10: agent_contexts owned strings; standby keyed by id string

Size: M. Depends on: T07. Covers (PLAN ids): P1-5, P1-6. User-visible effect: none - internal (JSON/files unchanged).

## Goal

Make `AgentContextProfile` use owned/Cow strings and registry-backed listing, and key the standby pool by id string, with identical JSON and file formats.

## Why now / depends on

Both are enum-to-string conversions with snapshot tests proving no output change; can run in parallel with T08/T09 after T07 (disjoint dirs). Depends on: T07. Index: `phases/README.md`. Spec: `SPEC.md`; original tasks: `PLAN.md` (P1-5, P1-6).

## In scope

- `agent_contexts` Cow strings + `fn context(&self) -> Option<ContextProfile>`.
- `standby/` drops `AgentType`.

## Out of scope

- New fields in either.
- UI.

## Tasks

File allow-list = the union of the files named below (`src/` = `packages/app/src-tauri/src/`). Anything else needs a plan edit.

- [ ] Snapshot the current agent_contexts JSON for the nine profiles first (test) — `src/agent_contexts/tests.rs` — done-when: snapshot test green before the change
- [ ] Convert to `Cow<'static,str>` + `agent_type: AgentId`; list from registry — `src/agent_contexts/mod.rs`, `src/agent_contexts/profiles.rs`, `src/agent_contexts/inspect.rs`, `src/tui/profile.rs` — done-when: same snapshot still passes
- [ ] Standby keyed by id string; `standby-pool.json` loads unchanged — `src/standby/config.rs`, `src/standby/key.rs`, `src/standby/pool.rs`, `src/standby/host.rs`, `src/standby/tests.rs` — done-when: no `AgentType` in `standby/`; a file from the previous build loads

## Tests to write first

- `agent_contexts::json_snapshot_unchanged_for_nine_profiles`: exact equality with the pre-change JSON
- `standby::loads_legacy_pool_file`: old `standby-pool.json` parses and yields the same keys
- `standby suite`: unchanged and green

## Verification commands

Run from `packages/app/src-tauri` with `CARGO_TARGET_DIR=C:\Users\you\cargo-target-shared` unless noted.

- `cargo test --lib agent_contexts`
- `cargo test --lib standby`
- `cargo test --lib`

End-of-phase invariant: the repo builds and all suites touched above pass; no half-migrated state.

## Risks and rollback

Serde renames silently changing JSON; the snapshot test catches it. Rollback: revert.

## Open questions this phase depends on

None. (SPEC section 11.)

## Agent prompt

Paste everything inside the block below.

````text
You are implementing phase T10 (agent_contexts owned strings; standby keyed by id string) of the declarative agent definitions plan. Goal: Make `AgentContextProfile` use owned/Cow strings and registry-backed listing, and key the standby pool by id string, with identical JSON and file formats.

Read first: docs/REPO_MAP.md, docs/specs/agent-definitions/SPEC.md (requirements referenced by P1-5, P1-6), docs/specs/agent-definitions/phases/T10-contexts-and-standby-owned-ids.md (your task list, tests, allow-list, verification) and the relevant PLAN.md tasks (P1-5, P1-6). Prerequisite phases already merged: T07.

Work: 1) write the tests listed under "Tests to write first" and see them fail; 2) do the tasks in order, touching only the allow-listed files; 3) run every command under "Verification commands" and fix until green; 4) commit with explicit `git add <paths>` (1-4 commits). Out of scope: New fields in either.; UI.
Phase note: Write the snapshot test before touching the structs.
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
