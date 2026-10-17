# T13: Definition schema structs, strict parse, and validation

Size: M. Depends on: T12. Covers (PLAN ids): P2-1, P2-2. User-visible effect: none - internal (pure parsing, nothing registered).

## Goal

Parse SPEC 7.1 TOML into serde structs with `deny_unknown_fields` and implement the pure validator producing every SPEC 7.4 `AGENT_DEF_*` code.

## Why now / depends on

Foundation for every later declarative phase; pure functions with no I/O so it is safe and fast to test. Depends on: T12. Index: `phases/README.md`. Spec: `SPEC.md`; original tasks: `PLAN.md` (P2-1, P2-2).

## In scope

- `schema.rs`, `validate.rs`, test modules, `pub mod declarative;`, new crate deps.

## Out of scope

- Templating/selectors (T14, T15).
- Loading from disk (T17).
- Registering anything.

## Tasks

File allow-list = the union of the files named below (`src/` = `packages/app/src-tauri/src/`). Anything else needs a plan edit.

- [ ] Schema structs + strict parse; both SPEC examples parse — `src/tui/declarative/schema.rs`, `src/tui/declarative/mod.rs` (stub), `src/tui/mod.rs` (`pub mod declarative;`), `packages/app/src-tauri/Cargo.toml` (`toml`, `sha2`, `globset`/`glob` lines only), `src/tui/declarative/tests/schema.rs` — done-when: unknown field, wrong type and bad `schema_version` each have a test asserting code + path
- [ ] Validator (no I/O, VAL-8) — `src/tui/declarative/validate.rs`, `src/tui/declarative/tests/validate.rs` — done-when: each SPEC 7.4 code is produced by at least one test: id pattern, reserved ids, regex compile + size limit, regex count, placeholders and `{task}` count, `when` set, bare binary, interpreter denylist, env denylist, size limits, read-only note, resumable consistency

## Tests to write first

- `declarative::schema::kilo_example_parses / minimal_example_parses`: SPEC 7.2 and 7.2b parse
- `declarative::schema::unknown_field_rejected_with_path`: error code and field path
- `declarative::validate::<one test per AGENT_DEF_* code>`: each code reachable

## Verification commands

Run from `packages/app/src-tauri` with `CARGO_TARGET_DIR=C:\Users\you\cargo-target-shared` unless noted.

- `cargo test --lib tui::declarative`
- `cargo test --lib`

End-of-phase invariant: the repo builds and all suites touched above pass; no half-migrated state.

## Risks and rollback

Cargo.toml has unrelated user edits: use `git add -p` for your dependency lines only. Schema drift against DEFINITION-FORMAT.md: note differences, fix in T26. Rollback: revert.

## Open questions this phase depends on

None. (SPEC section 11.)

## Agent prompt

Paste everything inside the block below.

````text
You are implementing phase T13 (Definition schema structs, strict parse, and validation) of the declarative agent definitions plan. Goal: Parse SPEC 7.1 TOML into serde structs with `deny_unknown_fields` and implement the pure validator producing every SPEC 7.4 `AGENT_DEF_*` code.

Read first: docs/REPO_MAP.md, docs/specs/agent-definitions/SPEC.md (requirements referenced by P2-1, P2-2), docs/specs/agent-definitions/phases/T13-schema-and-validation.md (your task list, tests, allow-list, verification) and the relevant PLAN.md tasks (P2-1, P2-2). Prerequisite phases already merged: T12.

Work: 1) write the tests listed under "Tests to write first" and see them fail; 2) do the tasks in order, touching only the allow-listed files; 3) run every command under "Verification commands" and fix until green; 4) commit with explicit `git add <paths>` (1-4 commits). Out of scope: Templating/selectors (T14, T15).; Loading from disk (T17).; Registering anything.
Phase note: Cargo.toml (packages/app/src-tauri) has unrelated uncommitted edits: stage only your own lines with `git add -p`.
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
