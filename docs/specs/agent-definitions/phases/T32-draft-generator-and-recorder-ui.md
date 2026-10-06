# T32: Draft generator, recorder UI, Phase 4 gate

Size: M. Depends on: T31, T23, T26. Covers (PLAN ids): P4-4, P4-5, P4-6. User-visible effect: Record a session in a pane, get a draft TOML, validate it.

Milestone: ★: recorded Kilo session -> valid draft

## Goal

Generate a draft definition from recorded captures (busy/chrome/prompt regexes, headless skeleton from a probe) with provenance comments, add the pane toolbar button and drafts list, and verify the Phase 4 gate.

## Why now / depends on

Needs real Kilo captures (T23) and the Settings page (T26). Depends on: T31, T23, T26. Index: `phases/README.md`. Spec: `SPEC.md`; original tasks: `PLAN.md` (P4-4, P4-5, P4-6).

## In scope

- Draft generation to `<app data>/agents/drafts/`.
- UI: record/stop/see path/validate.

## Out of scope

- Auto-approval of drafts (never).

## Tasks

File allow-list = the union of the files named below (`src/` = `packages/app/src-tauri/src/`). Anything else needs a plan edit.

- [ ] Draft generator — `src/agent_definitions_recorder/draft.rs`, `draft_tests.rs`, `src/agent_definitions_recorder/fixtures/` — done-when: from Kilo captures: busy regex matches working not idle; chrome list; prompt table; headless skeleton parses the probe; every candidate has `# from:` comment (REC-2)
- [ ] Recorder UI — pane toolbar component (grep `kill_pane_process` UI), `packages/app/src/components/settings/tabs/AgentsTab.tsx`, `src/commands/agents.rs` — done-when: record, stop, see draft path, validate via `/agents/definitions/validate`
- [ ] Gate — no source files — done-when: recorded Kilo session yields a draft that validates and, after at most two human line edits, passes the Kilo fixtures; pm-regression 28/28

## Tests to write first

- `recorder::draft::kilo_captures_produce_valid_draft`: validates
- `recorder::draft::candidates_carry_provenance`: REC-2

## Verification commands

Run from `packages/app/src-tauri` with `CARGO_TARGET_DIR=C:\Users\Ren-pc\cargo-target-shared` unless noted.

- `cargo test --lib agent_definitions_recorder`
- `npm test (packages/app)`
- `node scripts/pm-regression.mjs -> 28/28`
- live: record a Kilo pane

End-of-phase invariant: the repo builds and all suites touched above pass; no half-migrated state.

## Risks and rollback

Plausible-but-wrong drafts: provenance and validation mitigate. Rollback: revert.

## Open questions this phase depends on

None. (SPEC section 11.)

## Agent prompt

Paste everything inside the block below.

````text
You are implementing phase T32 (Draft generator, recorder UI, Phase 4 gate) of the declarative agent definitions plan. Goal: Generate a draft definition from recorded captures (busy/chrome/prompt regexes, headless skeleton from a probe) with provenance comments, add the pane toolbar button and drafts list, and verify the Phase 4 gate.

Read first: docs/REPO_MAP.md, docs/specs/agent-definitions/SPEC.md (requirements referenced by P4-4, P4-5, P4-6), docs/specs/agent-definitions/phases/T32-draft-generator-and-recorder-ui.md (your task list, tests, allow-list, verification) and the relevant PLAN.md tasks (P4-4, P4-5, P4-6). Prerequisite phases already merged: T31, T23, T26.

Work: 1) write the tests listed under "Tests to write first" and see them fail; 2) do the tasks in order, touching only the allow-listed files; 3) run every command under "Verification commands" and fix until green; 4) commit with explicit `git add <paths>` (1-4 commits). Out of scope: Auto-approval of drafts (never).

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
