# fix/live-hunt: four bugs from the live bug hunt

Source: `D:\pm-night\HUNT-REPORT.md` (live run of `integration/night` 93c8f2f). Branch `fix/live-hunt`,
backup `backup/fix-live-hunt-0000`. Personas: A developers, B vibecoders (undo and review must just work),
C non-technical people. Nothing here is agent specific.

## 1. HIGH: checkpoint restore always said PANE_RUNNING

- **Root cause.** `bridge::checkpoint_pane_facts` read `pane.info.status`. `PaneState.info` is the spawn-time
  snapshot: it is created with `status: "running"` (`pty/registry/spawn.rs`) and nothing ever writes it again.
  The live status is behind `PaneState.status` (what `PaneState::info()` and `GET /panes` report, which is why
  `/panes` said `idle` while restore said `PANE_RUNNING`). So `Runtime::restore` saw EVERY registered pane as
  running. The same stale read also made `capture_with_retry` skip the post-turn retry ("pane running again").
  It was not specific to `/agents/run`.
- **Fix.** `bridge::observe::pane_facts` reads the live status lock. The same stale read was fixed in
  `opencode/quota.rs` and `opencode/status.rs` (`pending_permission_status`).
- **Precise error.** New `CheckpointError::RunOpen` (`409 RUN_OPEN`, "A run is still open on this pane. Interrupt
  it first, then undo."). It is returned only when the pane really is running AND an operation is open on it
  (`Env::run_open`, `bridge::pane_has_open_run`). A lingering operation record never blocks an idle pane.
- **Tests (`checkpoints/tests.rs`).**
  - `restore_succeeds_on_an_idle_pane_whose_spawn_snapshot_says_running`: real `PaneRegistry` + the same
    `pane_facts` the app uses, a stub pane with `info.status == "running"` and the status lock `Idle`.
    FAILS before (`left: "running" right: "idle"`, reproduced by putting the old line back), passes after.
    It also asserts a pane whose lock is really `Running` is still refused.
  - `restore_names_an_open_run_precisely`.
  - Not done: a test that drives a full `/agents/run` dispatch into a visible pane (needs a live PTY agent). The
    defect is in the registry read that every pane goes through, so the registry-backed test covers it.

## 2. MEDIUM: `POST /project-path` returned ok but did not persist

- **Root cause.** The bridge route only called `registry_set_project_path` and `event_log::set_active_project_path`
  (the runtime folder). `settings.json`'s `project_path` is written by the JS side (`useProjectPath` calls the
  `set_project_path` command and then `patchSettings`), so a caller that used only the route changed nothing
  that Settings, the "+" menu or the next launch read.
- **Fix.** `project_path::set_active_and_persist` normalizes, sets the runtime folder, then persists through
  `settings_patch::apply` (the single atomic writer: store plugin + atomic file write + public mirror +
  `settings://changed`), so the JS `LazyStore` cache is updated in the same step. The route maps a bad path to
  400 and a failed save to 500 with a plain message. The Tauri command is unchanged (its caller patches settings).
- **Tests (`project_path.rs::persist_tests`).** The file changes, other settings are untouched, the registry
  holds the folder, exactly one `settings://changed` is emitted with `project_path`; a bad path is a 400 and
  never persists; a failed save is a 500. These cannot fail "before" in the strict sense (the function is new).
  The old route body never invoked any persist step, which is what the tests pin down. No in-process app handle
  exists in tests, so the route itself is covered through the closure seam.

## 3. MEDIUM: worktree runs "had no Changes/Discard UI"

- **Root cause.** The backend was fine and so was the lookup: re-checked live with a real
  `run_agent(isolation:"worktree", headless:false)`: the pane's cwd is the worktree, `pane_worktree` returns the
  item, and the Changes block (file list, Second opinion, Merge, Discard) renders. But it lived only INSIDE the
  Receipt tab, below the receipt rows, and nothing in the pane header or tab strip hinted at it. The hunter read
  the four tab names and concluded there was no UI.
- **Fix.** `components/cockpit/usePaneWorktree.ts` + `PaneView.tsx`: a pane in a worktree gets a "Changes" tab
  (with the file count, "Changes · 3") right after Terminal. It holds the diff, Second opinion, Merge and
  Discard. A pane that is not in a worktree is unchanged (four tabs). Second opinion moves out of Receipt for
  isolated panes (it is in Changes). The harness scenarios `cockpit-changes`, `cockpit-changes-merge`,
  `second-opinion-changes` now click the Changes tab.
- **Tests.** `PaneView.worktree.test.tsx` (FAILS without the PaneView change: tabs stay four), and
  `worktrees::tests::a_pane_working_in_a_worktree_finds_its_changes` (the `for_pane` lookup had no test).
- **Screenshot.** `D:\pm-night\evidence\fix-live-hunt\cockpit-changes.png` (also `cockpit-changes-merge.png`,
  `second-opinion-changes.png`, `cockpit-receipt.png`).
- **Seen but not fixed.** On that live isolated run the Receipt's "Last turn" said "0 files" while the worktree
  diff had 1 file (+2): the same family as NIGHT-LOG bug 5 (turn summary vs. what changed), owned by `fix/live-ui`.

## 4. LOW: ghost sidebar rows ("Claude Code 3", "PowerShell 3")

- **Cause (found by reading, not reproduced live).** `usePaneRegistry.refresh()` runs on mount, on every
  `pty://panes-changed` and 200 ms after every exit. Overlapping calls each did `setPanes(await listPanes())`,
  so an OLDER, slower list (taken while the pane still existed) could land after a newer one and put a closed
  pane back until the next refresh. That matches rows that vanish by themselves a few actions later and are not
  in `GET /panes`.
- **Fix.** A sequence counter: only the newest refresh may write (and set `initialReady`).
- **Test.** `hooks/usePaneRegistry.test.tsx` resolves two refreshes out of order. FAILS before
  (`expected ['a','ghost'] to deeply equal ['a']`), passes after.

## Evidence (exact commands)

- `cargo test --lib` (in `packages/app/src-tauri`, `CARGO_BUILD_JOBS=3`): `1501 passed; 0 failed; 2 ignored`
- `cargo test --bins`: `33 passed; 0 failed`
- `npm test --workspace=@puppet-master/app`: `113 files, 973 tests passed`
- `npx tsc --noEmit -p packages/app`: no output
- Live repro cleanup: the isolated run, its worktree and pane were discarded; `GET /panes` is `[]`, the demo
  repo has one worktree. The running app was not restarted or modified.

## Overlap with other branches

`checkpoints/{engine,http,runtime,tests}.rs`, `bridge/{observe,mod}.rs` (checkpoints/turn summary readers),
`opencode/{quota,status}.rs`, `project_path.rs`, `bridge/routes/workspace.rs`, `settings_patch.rs` (one
visibility change), `cockpit/PaneView.tsx` (shared file: tab list and Receipt), `hooks/usePaneRegistry.ts`,
dev scenarios for Changes. `fix/live-ui` also touches `PaneView.tsx` (Receipt tab): expect a small merge there.

## Limits

- The ghost-row cause is inferred from the code and a deterministic test, not captured live.
- `RUN_OPEN` only fires while the pane is running; a stale open operation on an idle pane is deliberately ignored.
