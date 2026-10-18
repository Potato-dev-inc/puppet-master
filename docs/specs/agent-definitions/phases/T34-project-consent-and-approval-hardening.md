# T34: Project/extra-dir consent banner and approval hardening

Size: M. Depends on: T33, T20. Covers (PLAN ids): P5-3, P5-4. User-visible effect: Opening a project with .puppet-master/agents/ shows a consent banner; approval shows an argv diff and warnings; binary swaps re-prompt.

## Goal

Implement per-canonical-path consent for project-local and extra dirs (banner, not modal; nothing loads until consent + approval), and harden approvals (diff view, warnings, resolved-binary pinning).

## Why now / depends on

Trust model for shared definitions; needs routes, UI and run-time enforcement already in place. Depends on: T33, T20. Index: `phases/README.md`. Spec: `SPEC.md`; original tasks: `PLAN.md` (P5-3, P5-4).

## In scope

- Consent flow and banner.
- Diff view, warning list, binary pin.

## Out of scope

- Sharing/export (T36).

## Tasks

File allow-list = the union of the files named below (`src/` = `packages/app/src-tauri/src/`). Anything else needs a plan edit.

- [ ] Consent flows — `src/tui/declarative/sources.rs`, `src/tui/declarative/approvals.rs`, `src/agent_runs/routes.rs` (consent route from T19), `packages/app/src/components/settings/tabs/AgentsTab.tsx`, inline banner component — done-when: banner lists files; nothing loads until consent+approval; revoke unregisters; cloned repo at a new path asks again
- [ ] Approval hardening — `packages/app/src/components/settings/tabs/AgentsTab.tsx`, `src/tui/declarative/approvals.rs`, `src/tui/declarative/fingerprint.rs` — done-when: diff between approved and current argv; warnings for `--auto`, `--dangerously`, `--yolo`, `--force`, env keys, interpreters; resolved binary recorded; swap triggers BINARY_CHANGED + re-approve

## Tests to write first

- `declarative::consent::matrix`: per-path consent, revoke, new path asks again
- `declarative::approvals::binary_swap_requires_reapproval`: BINARY_CHANGED
- `AgentsTab diff view and warnings`: text present

## Verification commands

Run from `packages/app/src-tauri` with `CARGO_TARGET_DIR=C:\Users\you\cargo-target-shared` unless noted.

- `cargo test --lib tui::declarative`
- `npm test (packages/app)`
- live: open a project with a hostile .puppet-master/agents/evil.toml -> not loaded

End-of-phase invariant: the repo builds and all suites touched above pass; no half-migrated state.

## Risks and rollback

Consent fatigue; banner not modal. SPEC 11 Q3 default path-only. Rollback: revert.

## Open questions this phase depends on

SPEC 11 Q3 (path only vs git remote): default path-only; not blocking. (SPEC section 11.)

## Agent prompt

Paste everything inside the block below.

````text
You are implementing phase T34 (Project/extra-dir consent banner and approval hardening) of the declarative agent definitions plan. Goal: Implement per-canonical-path consent for project-local and extra dirs (banner, not modal; nothing loads until consent + approval), and harden approvals (diff view, warnings, resolved-binary pinning).

Read first: docs/REPO_MAP.md, docs/specs/agent-definitions/SPEC.md (requirements referenced by P5-3, P5-4), docs/specs/agent-definitions/phases/T34-project-consent-and-approval-hardening.md (your task list, tests, allow-list, verification) and the relevant PLAN.md tasks (P5-3, P5-4). Prerequisite phases already merged: T33, T20.

Work: 1) write the tests listed under "Tests to write first" and see them fail; 2) do the tasks in order, touching only the allow-listed files; 3) run every command under "Verification commands" and fix until green; 4) commit with explicit `git add <paths>` (1-4 commits). Out of scope: Sharing/export (T36).

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
