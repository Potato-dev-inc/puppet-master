# T05: Verify the unverified Kilo facts

Size: S. Depends on: none. Covers (PLAN ids): P0-5. User-visible effect: none - docs only.

## Goal

Run the cheap live checks listed under 'Not verified' in AGENT-CHOICE.md and record the results (or mark them as explicit assumptions) so T21 is not built on guesses.

## Why now / depends on

No code dependency; can run any time before T21. Doing it early de-risks the first Kilo proof. Depends on: none. Index: `phases/README.md`. Spec: `SPEC.md`; original tasks: `PLAN.md` (P0-5).

## In scope

- Check `kilo run --auto` vs shell tools; `--agent plan` read-only behaviour; exit code on an `error` event; `-s` with TUI and `--prompt`.
- Record outputs in AGENT-CHOICE.md.

## Out of scope

- Saving fixtures (T21/T23).
- Any source change.

## Tasks

File allow-list = the union of the files named below (`src/` = `packages/app/src-tauri/src/`). Anything else needs a plan edit.

- [ ] Run each unverified check in a scratch dir (`C:\Users\you\AppData\Local\Temp\kilotest`) with a free model — none — done-when: each fact has a recorded command + observed result
- [ ] Update 'Not verified' into 'Verified' / 'Assumption' sections — `docs/specs/agent-definitions/AGENT-CHOICE.md` — done-when: no fact left ambiguous
- [ ] If a result contradicts the draft `kilo.toml` or SPEC 7.2, list the needed edit (do not edit SPEC here) — `docs/specs/agent-definitions/AGENT-CHOICE.md` — done-when: contradictions listed

## Tests to write first

- `(none - docs)`: n/a

## Verification commands

Run from `packages/app/src-tauri` with `CARGO_TARGET_DIR=C:\Users\you\cargo-target-shared` unless noted.

- kilo --version
- kilo run --format json ... (commands recorded in the doc)

End-of-phase invariant: the repo builds and all suites touched above pass; no half-migrated state.

## Risks and rollback

Kilo may need a provider credential; use a `kilo/...` free model. Never create or delete files outside the scratch dir.

## Open questions this phase depends on

None (feeds SPEC 10 assumption 4). (SPEC section 11.)

## Agent prompt

Paste everything inside the block below.

````text
You are implementing phase T05 (Verify the unverified Kilo facts) of the declarative agent definitions plan. Goal: Run the cheap live checks listed under 'Not verified' in AGENT-CHOICE.md and record the results (or mark them as explicit assumptions) so T21 is not built on guesses.

Read first: docs/REPO_MAP.md, docs/specs/agent-definitions/SPEC.md (requirements referenced by P0-5), docs/specs/agent-definitions/phases/T05-kilo-preflight-facts.md (your task list, tests, allow-list, verification) and the relevant PLAN.md tasks (P0-5). Prerequisite phases already merged: none.

Work: 1) write the tests listed under "Tests to write first" and see them fail; 2) do the tasks in order, touching only the allow-listed files; 3) run every command under "Verification commands" and fix until green; 4) commit with explicit `git add <paths>` (1-4 commits). Out of scope: Saving fixtures (T21/T23).; Any source change.
Phase note: Docs-only: edit AGENT-CHOICE.md and nothing else. Do not delete the scratch dir; leave it.
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
