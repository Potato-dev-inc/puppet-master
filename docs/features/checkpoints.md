# Checkpoints and one-click undo

Branch `feat/checkpoints`. Automatic file snapshots at every agent turn, and "Restore" per checkpoint, for **every
agent, in any folder, including folders that are not git repos**.

See also [What broke it](what-broke-it.md): finds the turn that broke your test command, using these save points, and can undo only that turn.

## 1. What and why

- **Personas:** B (vibecoders) and C (non-technical) lose work and cannot roll back; A (developers) get a safety net
  that never touches their git history.
- **Story:** "The agent broke my project. I open History, pick *Before turn 3*, press Restore. My files are back."
- **Agent-agnostic:** it is a filesystem concern plus pane status transitions. No code matches an agent name. The only
  per-agent knob is `TuiProfile::checkpoints()` (plain shells opt out by *kind*, declarative agents by TOML).
- **Plain wording, used everywhere:** "This restores your files. It doesn't change what the agent remembers."

## 2. Design

### Shadow repository
- `GIT_DIR` = `<project>/.puppet-master/checkpoints.git`, `GIT_WORK_TREE` = the project folder. Every git call
  (`checkpoints/git.rs`) passes `--git-dir` and `--work-tree`, strips inherited `GIT_*` variables, and replaces user and
  system git config with an empty file (no autocrlf, hooks, fsmonitor). The user's `.git` is never named: HEAD, index,
  branches and refs are untouched (asserted by a test, see Evidence).
- **Snapshot** (`checkpoints/engine.rs`): `ls-files -o -m` (size guard) -> `add -A` into a **private per-pane index**
  (`pm-index-<pane>`, also acts as a stat cache so later snapshots are fast) -> `write-tree` -> `commit-tree` ->
  `update-ref refs/pm/<pane>/<n>`. Commits are orphans (no parents), otherwise pruning a ref would not free objects.
  Metadata (turn, kind, note, time, files, +/-) lives in the commit message, so listing is one `for-each-ref`.
- **Excludes:** `.git/`, `.puppet-master/`, the work tree's own `.gitignore` files (git reads them natively, even with
  a separate git dir), the project's `.git/info/exclude` (copied), plus defaults for folders without any `.gitignore`:
  `node_modules/ .venv/ venv/ __pycache__/ .next/ .cache/ .DS_Store Thumbs.db`.
- **Size guard:** files over 20 MB are left out (never deleted by restore); a snapshot whose new/changed files exceed
  1 GiB is refused with a plain message.
- **No git installed:** the feature is disabled with "Checkpoints need Git, and Git isn't installed on this computer."
  (the History tab shows it; nothing else breaks).
- **`gix` for reads?** No. Reads here are `for-each-ref` and `diff-tree --numstat`, one process each, and a snapshot
  needs the git CLI anyway for `add -A` with ignore rules and an index. Adding `gix` would add a large dependency tree
  for no gain at this size. Revisit if listing latency matters (it is one ~10 ms process on a 50-ref repo).

### Turn boundaries (`checkpoints/runtime.rs`)
| Trigger | Where it is hooked | Snapshot |
|---|---|---|
| prompt dispatched to a pane | `operations/dispatch.rs::run_operation_dispatch` (when the operation already has a pane) | "Before turn n" (+ the task text as note) |
| pane status -> `running` | `bridge/sse.rs::push_pane_status_sse` | "Before turn n" (covers turns the user types, and runs that create their pane during dispatch) |
| `running` -> `idle` / `waiting_input` / `error` | same | "After turn n" |
| prompt answered | top of `agent_runs/routes/answer.rs::route_answer` (bare pane id) | "Before answering a prompt (turn n)" |
| restore | `engine::restore` | "Before undo" (safety copy) |

- Not the PTY reader. The status hook only decides on the caller's thread, and the snapshot runs on a short worker
  thread (`pm-checkpoint`). The dispatch and answer hooks are synchronous on purpose: the copy must exist first.
- **Debounce:** at most one automatic snapshot per pane per 10 s. A turn-end snapshot that falls inside the window is
  retried once when the window closes (if the pane is still at rest), so a short turn's end state is not lost.
  **Unchanged trees are skipped** (compared with the pane's newest checkpoint).
- Extra beyond the brief: the `-> running` trigger. Without it, turns typed by the user in the terminal (no operation)
  would have no "before" state.
- **Attribution is per folder, not per agent.** Two panes in the same folder share one shadow repo and each has its own
  refs and index, but both snapshot the whole folder: pane A's checkpoint also contains files pane B changed meanwhile.
  Restoring pane A's checkpoint therefore also rewinds B's edits made after it. Worktree isolation (separate folders)
  makes attribution exact. The restore sheet does not warn about this yet (follow-up).

### Restore
1. Refused when the pane is `running`: "The agent is working. Interrupt it first, then undo." (Rust: `Running`;
   HTTP 409 `PANE_RUNNING`; the UI also disables the button).
2. Safety snapshot "Before undo" (so undo is undoable; skipped only when the tree already equals the newest checkpoint).
3. Compute the delta between *what is on disk now* (the safety tree) and the target tree; delete files that exist now
   but not in the target, rewrite the rest with `read-tree` + `checkout-index`. Only paths that exist in the shadow
   trees are touched, so ignored, oversized and untracked-by-exclusion files are never deleted or overwritten. Folders
   emptied by deletions are removed. Paths are validated (no `..`, no absolute paths).
4. Failures (locked files) are reported per file; the rest still restores.

### Retention
Newest 50 checkpoints per pane (setting `checkpoints_keep`, 5-500); older refs are deleted right after each snapshot.
`git gc --auto` runs every 10th snapshot; `gc.pruneExpire=1.day.ago`.

### Surfaces
- Tauri: `checkpoints_list(paneId)`, `checkpoint_diff(paneId, id)`, `checkpoint_restore(paneId, id)`,
  `checkpoint_undo_last(paneId)`.
- Bridge (same engine; MCP catalog untouched): `GET /panes/{id}/checkpoints`,
  `GET /panes/{id}/checkpoints/{cid}/diff`, `POST /panes/{id}/checkpoints/{cid}/restore`. Routes sit under `/panes/`,
  so the existing MCP-session ownership gate applies to the mutating POST.
- Cockpit: pane tab **History** (Terminal . Transcript . Receipt . History): time, label (+ task note), files and
  +/-, expandable file list, Restore with a confirm sheet. Command palette: **Undo last turn**. Settings > General:
  **Checkpoints** toggle and **Keep last N**.
- Settings keys `checkpoints_enabled` (default true) and `checkpoints_keep` (default 50) are public settings, read
  live by the Rust side (`settings_store.rs`, `shared/schemas.ts`, `bridge-settings.ts`).
- Phone: not in this branch.

### Profile / TOML
`TuiProfile::checkpoints() -> bool`, default `features().kind != Kind::Shell`. Plain shells default to **false**: they
have no agent turns, status flips on every command, and users run builds there that churn files. Declarative agents:
`[features] checkpoints = false` to opt out (default true).

### OSS
No new crate. Runtime dependency: the `git` CLI (GPL-2.0, invoked as an external program, not linked or copied). Test
code uses `sha2` and `uuid`, already dependencies.

## 3. Evidence

All commands run in the worktree `D:\pm-checkpoints` on 2026-10-14.

**Rust** (`cd packages/app/src-tauri; CARGO_BUILD_JOBS=3 cargo test --lib`):
```
test result: FAILED. 1024 passed; 1 failed; 2 ignored; 0 measured; 0 filtered out
   (the one failure is the known pre-existing bridge::tests::definitions::approve_requires_app_session)
```
New tests (`cargo test --lib checkpoints`, `16 passed; 0 failed`):
- `non_git_folder_snapshot_modify_restore`: non-git folder; files back, new files and emptied folders removed, the
  ignored file untouched, and redo (restoring the later checkpoint) works.
- `restore_takes_a_before_undo_safety_snapshot_first`
- `users_git_repo_is_byte_identical_after_snapshot_and_restore`: `git rev-parse HEAD`, branches, `for-each-ref`, the
  SHA-256 of `.git/index`, and `git status --porcelain` identical before and after snapshot + restore (the only
  difference allowed is the pre-existing `?? .puppet-master/` line; see Limits).
- `unchanged_tree_is_skipped_and_ids_keep_counting`, `debounce_allows_one_snapshot_per_pane_per_ten_seconds`
- `retention_keeps_only_the_newest_n_per_pane`
- `restore_is_refused_while_the_pane_runs`, `runtime_restore_refuses_a_running_pane_and_undo_last_turn_works`
- `big_files_are_left_out_and_never_deleted`, `total_size_guard_refuses_a_giant_snapshot`
- `status_transitions_map_to_turn_boundaries`, `off_switch_and_profile_opt_out_skip_snapshots`,
  `two_panes_in_one_folder_keep_separate_refs`
- `http_routes_list_diff_and_restore_with_error_codes`: in-process request -> response transcript of the three routes
  (200 list newest-first, 200 diff `a.txt M`, 409 `PANE_RUNNING`, 200 restore with `safetyId`, 404
  `CHECKPOINT_NOT_FOUND`, `state: "off"` when disabled, `None` for non-checkpoint routes).
- `profiles_decide_participation_not_agent_names` and `tui::declarative::tests::profile::checkpoints_default_on_and_follow_the_definition`

**Fails before, passes after.** The tests do not compile on the base commit (module missing). For a behavioural check
the guard in `engine::restore` was disabled (`if false && pane_running`):
```
test checkpoints::tests::restore_is_refused_while_the_pane_runs ... FAILED
test result: FAILED. 0 passed; 1 failed
```
and restored: `... ok`.

**Frontend** (`npm test --workspace=@puppet-master/app`): `Test Files 48 passed (48)`, `Tests 382 passed (382)`.
New: `src/components/cockpit/History.test.tsx` (9 tests): newest-first timeline, expandable files, empty/off/no-git/
error states, confirm sheet text ("This restores your files. It doesn't change what the agent remembers."), restore
only after confirm, Cancel restores nothing, Restore disabled while running, a refused restore is reported.

**Types:** `npx tsc --noEmit -p packages/app` -> no output (clean).

**Screenshots** (`D:\pm-night\evidence\checkpoints\`, harness scenarios `cockpit-history`,
`cockpit-history-restore`, `settings-checkpoints`; opened and checked): `cockpit-history.png` (timeline with one row
expanded), `cockpit-history-restore.png` (confirm sheet), `settings-checkpoints.png` (toggle and Keep last N).

## 4. Limits / follow-ups
- **`.puppet-master/` shows as untracked in the user's `git status`.** The folder already held the operations store; a
  checkpoint repo makes it bigger. Not fixed here because adding it to the user's `.git/info/exclude` would modify
  their repo. Suggest an opt-in "Hide .puppet-master from git" button.
- **Per-folder attribution** (see above): two agents in one folder cannot be separated; no warning in the sheet yet.
- **Nested git repos** inside the project are recorded as a single entry by `git add` (their contents are not
  snapshotted and not restored).
- **Headless runs without a pane** get no History surface (the dispatch hook only fires for operations with a pane).
- "Before turn" at the `-> running` transition is taken a moment after the agent started: an agent that edits within
  the first milliseconds could be captured mid-edit. The dispatch hook covers runs; manual typing relies on this.
- **Windows file locks:** `add -A` retries once after 400 ms; restore reports locked files individually.
- No file *content* diff viewer yet (counts and file list only); no restore of a single file; no phone surface;
  no warning when the pane's cwd is a huge folder beyond the 1 GiB guard.
- `git gc --auto` runs inline every 10th snapshot on the worker thread.

## 5. Overlaps with other branches (file level)
Append-only or one-line changes in shared files; expected merge hot-spots:
- `packages/app/src-tauri/src/lib.rs` (mod line + 4 command names), `commands/mod.rs`, `bridge/routes/mod.rs`,
  `bridge/mod.rs`, `bridge/observe.rs`: any branch adding commands, routes or bridge helpers.
- `bridge/sse.rs::push_pane_status_sse` (1 line): `feat/loop-guard`, `feat/ambient`, `feat/agent-doctor` if they also hook status.
- `operations/dispatch.rs::run_operation_dispatch` (3 lines): `feat/worktree-isolation`, `feat/loop-guard`.
- `agent_runs/routes/answer.rs::route_answer` (3 lines): `feat/approval-cards`, `feat/ask-user`.
- `settings_store.rs` (`PATCHABLE_KEYS`, defaults), `shared/src/schemas.ts`, `lib/bridge-settings.ts`, `lib/settings.ts`,
  `SettingsPanel.tsx`, `GeneralTab.tsx`: `feat/account-profiles` and any settings work.
- `tui/profile.rs`, `tui/declarative/schema.rs` + `profile.rs` (a new method and a new `[features]` key): other branches
  touching `TuiProfile`/the TOML schema (`feat/account-profiles`, `feat/agent-doctor`).
- Cockpit: `PaneView.tsx` (a tab), `Palette.tsx` (an action), `cockpit-api.ts`, `styles/cockpit.css` (appended block),
  `dev/fixtures.ts`, `dev/mock-tauri.ts`, `dev/scenarios/` (new files only).
- Worktree isolation interplay: with isolated worktrees each pane has its own folder and shadow repo, so attribution
  becomes exact with no change here.
