# Worktree-isolated agents: review, merge, discard

Branch `feat/worktree-isolation`. Available to every agent type and every MCP host.

## 1. What and why

Parallel agents in one folder clobber each other's files. This is the number one multi-agent pain in the research. With
`isolation: "worktree"` each agent works in its own `git worktree` on its own branch. Nothing the agent does touches the
user's checkout until someone reviews the diff and merges it.

- **A developers:** `run_agent(..., isolation: "worktree")` gives each subagent its own branch `pm/<handle>`. The cockpit
  has Review, Merge and Discard. An orchestrating agent can do the same through `review_worktree`.
- **B vibecoders:** the sidebar "+" menu has an "Isolated (worktree)" switch. Try something risky, look at the changes,
  keep them or throw them away.
- **C non-technical:** plain copy: "Changes", "Merge", "Discard", with a confirm sheet that says what will happen.
- **Agent-agnostic:** it is a filesystem concern. No `TuiProfile` capability and no TOML field is needed, and generic
  code never matches agent strings. Claude, Codex, OpenCode, Kilo, a custom TOML agent and plain shells all work, because
  the only thing that changes is the directory the process starts in.

## 2. Design

### Files
| Path (under `packages/app/src-tauri/src`) | Role |
|---|---|
| `worktrees/mod.rs` | git layer: `create`, `changes`, `diff`, `merge` (`merge_keep` + `cleanup_merged`), `discard`, `list`, `sweep`, `origin_of_cwd`. Pure git and filesystem, no agent knowledge. |
| `worktrees/git.rs` | the one place that spawns `git` (no console window, no prompts, `LC_ALL=C`, capped output). |
| `worktrees/service.rs` | by `(project, handle)`: guards (`RUN_ACTIVE`), closes panes in the worktree, records `merged` / `discarded` on the operation. |
| `worktrees/routes.rs`, `bridge/routes/worktrees.rs` | bridge routes (glue is 20 lines). |
| `commands/worktrees.rs` | Tauri commands `worktree_list`, `pane_worktree`, `worktree_diff`, `worktree_merge`, `worktree_discard`; `new_pane` gained `isolation`. |
| `operations/worktree.rs`, `operations/model.rs` | `WorkerPersist.worktree`, `OperationSnapshot::work_dir()`, `close_worktree`, `record_worktree_changes`. |
| `agent_runs/headless/entry.rs` | `isolation` validation, creates the worktree as the last step that can still be refused. |
| `agent_runs/runtime/receipt.rs` | `receipt.worktree`. |
| `tool_registry/catalog/agents.rs` | MCP: `isolation` on `run_agent`, new tool `review_worktree` (catalog 66 to 67). |
| UI: `components/cockpit/Changes.tsx`, `diff.ts`, `styles/cockpit-changes.css` | Receipt tab "Changes", monochrome diff viewer, confirm sheet. `Sidebar.tsx` has the toggle. |

### Behaviour
- **Create:** `git worktree add -b pm/<handle> <project>/.puppet-master/worktrees/<short> <HEAD>`. `<short>` is at most
  10 characters of the handle plus a 4-hex hash (about 17 characters) to keep Windows paths under MAX_PATH. The path is
  added to the repo's `.git/info/exclude` (never `.gitignore`). A retry for the same handle gets the same worktree.
  A project inside a repo subfolder runs in the matching subfolder of the worktree. Uncommitted edits in the origin are
  not copied: the worktree starts from `HEAD`.
- **Request plumbing:** the operation's `project_path` stays the ORIGIN repo, so `list_agents`, waits, locks and
  idempotency are unchanged. Only the spawn cwd moves to the worktree: visible panes (`assign_pane`) and headless
  children (`execute_headless`). Acceptance `checks` and `file_exists:` criteria are evaluated in the worktree, because
  that is where the files are. An isolated run never reuses a shared pane and never claims a warm standby worker
  (those were launched in the project folder). Follow-up turns inherit the worktree through the previous turn's worker
  record, so a restart does not lose it.
- **Lifecycle:** the worktree is kept when the run ends. The only things that remove it are an explicit Discard or a
  successful Merge. A pane closed without review leaves it listed as `unreviewed`. After a merge or discard the latest
  operation records `closed: merged|discarded`, so a later turn on that handle falls back to the project folder.
- **Diff:** the whole state of the worktree (committed, uncommitted, untracked, ignore rules honoured) is snapshotted as a
  tree object through a temporary copy of the index, so the agent's own index is never touched. The stat is complete; the
  patch is capped at 256 KiB.
- **Merge:** refuses when the origin is detached (`ORIGIN_DETACHED`), mid-merge, or has uncommitted changes to files
  the worktree also changed (`ORIGIN_DIRTY_OVERLAP`, listing the files). Otherwise it commits the agent's work on its
  branch and runs `git merge --no-ff` into the origin's current branch. A conflict is aborted
  (`merge --abort`, then `reset --merge`), HEAD is verified unchanged, and `MERGE_CONFLICT` lists the conflicted
  files. The origin is never left mid-merge, and the work stays safe on the branch. Success removes the worktree and
  branch. No changes returns `merged:false, reason:"no_changes"`. Merge and Discard return `RUN_ACTIVE` while the agent is
  working, and close panes sitting in the worktree first (Windows cannot delete a directory in use).
- **Orphan sweep:** at startup, for every known project: prune git registrations whose checkout is gone, remove
  folders git does not know and no description names (half-created), drop descriptions of vanished checkouts (the branch
  is deleted only if it holds no commits). It never touches work.
- **Non-git project:** `NOT_A_GIT_REPO` ("run `git init` and make a first commit, or run without isolation").
  No commits is `NO_COMMITS`. Missing git is `GIT_UNAVAILABLE`.

### Surfaces
- **MCP:** `run_agent(isolation: "none" | "worktree")`; `review_worktree(handle, action: diff|merge|discard, path?)`
  for handles the connection owns (the standard `check_run_access` gate). The receipt gains
  `worktree {path, branch, base, state, files_changed, additions, deletions}`.
- **Bridge (app session only, not exposed to MCP sessions):** `GET /worktrees`, `GET /worktrees/{handle}/diff`,
  `POST /worktrees/{handle}/merge`, `POST /worktrees/{handle}/discard`.
- **Tauri:** `worktree_list`, `pane_worktree` (finds the worktree from the pane's cwd, so panes opened by hand work too),
  `worktree_diff`, `worktree_merge`, `worktree_discard`, `new_pane(agentId, isolation)`.
- **Cockpit:** Receipt tab, "Changes": branch, base, files with `+`/`-`, an expandable diff (additions are heavier with
  a solid left rule, deletions dimmer with a hairline rule, no red or green), Merge and Discard behind a confirm sheet,
  disabled while the agent works. Sidebar "+" menu: quiet "Isolated (worktree)" checkbox.

### OSS and the git choice
No new crate. It uses the `git` CLI like the rest of the codebase. `gix` (MIT/Apache-2.0) does not yet cover
`worktree add`, `merge --no-ff` and abort end to end, and `git2` links a C libgit2 build (slow and fragile on Windows).
The CLI is what the developer would run by hand and is present wherever an agent CLI is. No third-party code was copied.

## 3. Evidence

Commands (in `packages/app/src-tauri` with `CARGO_TARGET_DIR=D:/pm-cargo-target CARGO_BUILD_JOBS=4`):

```
cargo test --lib
  test result: FAILED. 1033 passed; 1 failed; 2 ignored   (the one failure is the known bridge::tests::definitions::approve_requires_app_session)
cargo test --lib worktrees
  test result: ok. 18 passed; 0 failed
cargo test --bins
  test result: ok. 30 passed; 0 failed
npx tsc --noEmit -p packages/app          -> no output (clean)
npm test --workspace=@puppet-master/app   -> Test Files 48 passed (48); Tests 383 passed (383)
```

Rust integration tests (real temp git repos, `worktrees/tests.rs`):
- `create_makes_a_worktree_on_its_own_branch_and_hides_it_from_git`: worktree + branch `pm/run-1`, `.git/info/exclude`
  has the line, no `.gitignore`, origin `status` clean, retry returns the same worktree.
- `changes_in_the_worktree_do_not_touch_the_origin_and_the_diff_shows_them`: origin files, HEAD and status unchanged;
  diff shows modified, added and deleted files with counts; the agent's index is untouched.
- `merge_brings_the_changes_in_with_a_merge_commit_and_closes_the_worktree`: two-parent merge commit, worktree and branch gone.
- `a_conflict_aborts_cleanly_and_leaves_the_origin_unchanged`: `MERGE_CONFLICT` with `["a.txt"]`, HEAD identical, no
  `MERGE_HEAD`, no markers, work still on the branch.
- `merge_refuses_when_the_origin_has_uncommitted_changes_to_the_same_files` (and an unrelated dirty file does not block).
- `discard_removes_the_worktree_and_the_branch_even_with_unmerged_work`, `discard_refuses_a_path_outside_the_worktrees_folder`.
- `a_non_git_project_is_refused_with_a_clear_code` (`NOT_A_GIT_REPO`, nothing created), `a_repo_without_commits_is_refused`.
- `the_startup_sweep_removes_leftovers_but_never_unmerged_work`, `a_vanished_checkout_whose_branch_has_commits_keeps_the_branch`.
- `route_transcript_list_diff_conflict_merge_discard`: request to response for `GET /worktrees`, `GET .../diff`,
  `POST .../merge` (merged, then a conflict), the MCP `POST /worktrees/review`, `.../discard`, and `WORKTREE_NOT_FOUND`.
- Wiring: `an_isolated_run_receipt_names_its_worktree_and_stats_and_the_run_keeps_the_origin_project`,
  `the_worktree_survives_a_snapshot_round_trip...` (restart persistence, old snapshots still load),
  `isolation_tests::*` (validation), `run_request_isolation_defaults_to_none_and_parses_worktree`,
  `worktree_refusals_map_to_http_statuses_with_their_context`.
- MCP: `run_agent_advertises_isolation_and_review_worktree_is_exposed`; `catalog_version_is_pinned` updated
  `66-46d53ccddbd2b4dc` to `67-293f032d5ca3f51d`.

Fails before, passes after: `tool_registry::tests::run_agent_advertises_isolation_and_review_worktree_is_exposed`.
With `tool_registry/catalog/agents.rs` restored from `9cd5234` it fails (`assertion left == right failed, left: Null`
at `tests.rs:465`); with the branch version it passes (`1 passed`).

Frontend: `components/cockpit/Changes.test.tsx` (10 tests): hidden without a worktree; branch, totals and per-file
counts; diff lines classified by weight and rule with no colour classes; Merge and Discard confirm sheets and the calls
made; Cancel changes nothing; a conflict shows as an alert with the files; buttons disabled while working; patch splitting.

Screenshots (headless harness, merged `c3babea`): `D:\pm-night\evidence\worktree-isolation\cockpit-changes.png` (dark,
file expanded) and `cockpit-changes-merge.png` (light, confirm sheet). Scenarios: `src/dev/scenarios/cockpit-changes*.ts`.

## 4. Limits and follow-ups

- No end-to-end `run_agent` test through a real pane: it needs a Tauri `AppHandle` and a real agent. The pieces are
  tested (validation, git layer, persistence, receipt, routes); the live path was not exercised. Needs a manual run.
- The MCP exe in `packages/mcp-server/dist` was not rebuilt (the catalog changed): rebuild before using it from a host.
- Uncommitted edits in the origin are not in the worktree (it starts from `HEAD`); Merge refuses overlapping edits.
- A worktree created for a run whose dispatch then fails is left listed as an empty `unreviewed` worktree (Discard
  removes it); only the startup sweep of vanished checkouts cleans automatically.
- `run_agent(pane_id=<raw pane id of an isolated pane>)` adopts the pane with the worktree path as its project; use the
  run handle instead.
- Agents or tools that scan the project folder will also see `.puppet-master/worktrees/*` copies.
- Windows MAX_PATH: the worktree root is short, but deep paths inside a project can still exceed it
  (`core.longpaths=true` is set for creation).
- Merge uses `--no-ff` only; no squash or rebase option, and no per-file accept.
- The sidebar list does not yet show an "unreviewed" badge; `worktree_list` / `GET /worktrees` already return the state.
