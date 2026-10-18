# T06: AgentId and registry skeleton (not yet wired)

Size: M. Depends on: T03, T04. Covers (PLAN ids): P1-1. User-visible effect: none - internal.

## Goal

Add `TuiProfile::id()` and a `Registry` (ArcSwap snapshot seeded with the nine code profiles) with tests, while `profile_for` keeps returning `&'static dyn TuiProfile`.

## Why now / depends on

Isolates the new data structure from the caller migration (T07) so each phase is independently green. Depends on: T03, T04. Index: `phases/README.md`. Spec: `SPEC.md`; original tasks: `PLAN.md` (P1-1).

## In scope

- `id()` on the trait and the profile impls.
- `registry.rs` with `snapshot()`, ordered list, `profile_for_arc`.
- Order-preservation and stress tests.

## Out of scope

- Changing `profile_for`'s return type or callers (T07).
- Declarative profiles.

## Tasks

File allow-list = the union of the files named below (`src/` = `packages/app/src-tauri/src/`). Anything else needs a plan edit.

- [ ] Add `fn id(&self) -> &str` to the trait and implement for the nine profiles + generic — `src/tui/profile.rs`, `src/tui/claude/mod.rs`, `src/tui/codex/mod.rs`, `src/tui/cursor/mod.rs`, `src/tui/opencode/mod.rs`, `src/tui/shell/mod.rs`, `src/tui/generic.rs` — done-when: conformance id test green
- [ ] Create `Registry` over `ArcSwap<Vec<Arc<dyn TuiProfile>>>` seeded in existing order — `src/tui/registry.rs` (new), `src/tui/mod.rs`, `src/lib.rs` (mod decl only), `packages/app/src-tauri/Cargo.toml` (`arc-swap` line only) — done-when: builds; `read_only_backends()` order unchanged (test)
- [ ] Stress test: swap 10k times while 8 threads read — `src/tui/registry.rs` (tests) — done-when: no panic, no stale beyond one call

## Tests to write first

- `tui::registry::seeded_in_code_order`: ids equal current `profiles()` order
- `tui::registry::stress_swap_while_reading`: 10k swaps, 8 reader threads, no panic
- `tui::read_only_backends_order_unchanged`: same order as before

## Verification commands

Run from `packages/app/src-tauri` with `CARGO_TARGET_DIR=C:\Users\you\cargo-target-shared` unless noted.

- `cargo test --lib tui::`
- `cargo test --lib`

End-of-phase invariant: the repo builds and all suites touched above pass; no half-migrated state.

## Risks and rollback

`packages/app/src-tauri/Cargo.toml` has unrelated uncommitted user edits: stage only your hunk with `git add -p`; if that is impossible, stop and ask. Rollback: revert.

## Open questions this phase depends on

None. (SPEC section 11.)

## Agent prompt

Paste everything inside the block below.

````text
You are implementing phase T06 (AgentId and registry skeleton (not yet wired)) of the declarative agent definitions plan. Goal: Add `TuiProfile::id()` and a `Registry` (ArcSwap snapshot seeded with the nine code profiles) with tests, while `profile_for` keeps returning `&'static dyn TuiProfile`.

Read first: docs/REPO_MAP.md, docs/specs/agent-definitions/SPEC.md (requirements referenced by P1-1), docs/specs/agent-definitions/phases/T06-registry-skeleton-agentid.md (your task list, tests, allow-list, verification) and the relevant PLAN.md tasks (P1-1). Prerequisite phases already merged: T03, T04.

Work: 1) write the tests listed under "Tests to write first" and see them fail; 2) do the tasks in order, touching only the allow-listed files; 3) run every command under "Verification commands" and fix until green; 4) commit with explicit `git add <paths>` (1-4 commits). Out of scope: Changing `profile_for`'s return type or callers (T07).; Declarative profiles.
Phase note: IMPORTANT: packages/app/src-tauri/Cargo.toml has pre-existing uncommitted user changes. Never `git add` it wholesale; use `git add -p` for only the dependency line you add, or ask the caller.
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
