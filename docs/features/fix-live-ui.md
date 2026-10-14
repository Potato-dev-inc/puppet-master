# Fix live UI bugs (branch `fix/live-ui`, from integration/night 93c8f2f)

Bugs from the first live run (`D:\pm-night\NIGHT-LOG.md`, "First LIVE run"). Backup branch: `backup/fix-live-ui-start`.

## Root causes and fixes

| # | Root cause | Fix | Test (fails before) |
|---|---|---|---|
| 5 | Two bugs. (a) `turn_summary::tracker` counted every `running` blip as a turn. After the real 166s edit turn (turn 3) two 0.8s flickers became turns 4 and 5, and the newest became "Last turn" (0s, 0 files). Summaries were also appended in finish order (the ring held 1,2,3,5,4). (b) Checkpoints numbered turns only when a snapshot was created (quiet turns leave none), so the edit turn 3 was "turn 1" in History. | Sub-second turns with no approval are not turns and give their number back (`BLIP_MS`). Checkpoints take their turn number from the tracker (`turn_for_checkpoint`, `Env::turn_hint`, `engine::snapshot_numbered`). `store::upsert` keeps the ring ordered by turn. | `turn_summary::tests::flickers_are_not_turns_and_do_not_replace_the_last_turn` (the real sequence from `turn-summaries.json`), `checkpoint_turn_numbers_follow_the_trackers_turns`, `summaries_that_finish_out_of_order_are_listed_by_turn`, `checkpoints::tests::a_caller_supplied_turn_number_labels_the_checkpoints` |
| 6 | Claude draws a diff panel right of the chat. `vt100::contents()` glues a full-width row to the next, so chat and panel rows came out as one interleaved text. | `tui/side_panel.rs`: the panel divider is found by structure (a box-drawing bar in one column on most rows with text to its right). `TuiProfile::side_panel()` (Claude opts in) makes `read_snapshot` and `read_full_text` read one entry per row cropped at the divider. Claude's `tidy_reply` also strips the column from reply text. Fixture `tui/claude/fixtures/side_diff_panel_screen.txt` is rebuilt from `03b-claude-done.png` (164x50, divider at col 90). | `tui::side_panel::tests::*`, `pty::registry::pane_io::full_text_tests::a_side_panel_is_cropped_and_rows_are_not_glued`, `agent_runs::side_panel_reply_tests::*` |
| 8 | `CockpitApp` kept `doctorOpen` and `memoryOpen` booleans; the render chain checked the doctor first. Re-selecting the selected pane changed no `selectedId`, so the memory-clearing effect never ran. | One `view: 'pane' \| 'doctor' \| 'memory'` state; the last click wins; `nav.select` always sets `pane`. | `src/CockpitApp.test.tsx` (3 of 5 fail on the old file) |
| 9 | `classify_agent_observation` can only say `running` for a non-prompt screen; idle comes only from the idle watcher. Selecting a pane remounts the terminal, the TUI reprints after the resize, and the reader flipped the idle pane to `running` until the watcher caught up. The same flip made bug 5's phantom turns. | `pty::status::settle_status`: an idle pane stays idle when the profile's `busy_marker` proves the screen shows no work. Real work (spinner, stop hint) still flips it. The remount nudge is untouched. | `pty::status::tests::a_redraw_of_an_idle_claude_pane_does_not_flip_it_to_running`, `real_work_still_flips_an_idle_pane_to_running` |
| 10 | `one_brain::has_user_content` only excluded the managed block, so the front matter Puppet Master writes as the file header counted as user text. | The target's declared header is not user text; anything else outside the block is. | `one_brain::tests::front_matter_puppet_master_wrote_is_not_user_text` |
| 11 | Not a code bug. The handler is a real `<button onClick>`; click, Enter and Space all open Settings. Settings opens a separate OS window, and the live harness probed the main page only. The only silent failure path was `.catch(() => {})` in the palette. | `open_settings` failures are logged and shown. | `Sidebar.settings.test.tsx` (6 tests; the surfacing test fails before) |

Agent-agnostic: generic code asks `profile.side_panel()` and `busy_marker()`, never an agent name.

## Evidence
- `cargo test --lib`: 1514 passed, 0 failed, 2 ignored. `cargo test --bins`: 33 passed.
- `npm test --workspace=@puppet-master/app`: 113 files, 980 tests passed. `npx tsc --noEmit -p packages/app`: clean.
- Fail-before: with `BLIP_MS = 0`, `flickers_are_not_turns...` panics; with Claude's `tidy_reply` reverted to identity, `reply_text_that_still_carries_the_panel_is_tidied_by_the_profile` fails; `CockpitApp.test.tsx` fails 3 of 5 against the old `CockpitApp.tsx` (`expected 'DOCTOR VIEW' to be 'MEMORY VIEW'`); the mdc test failed with `("in_sync", true)` vs `("in_sync", false)`.
- Screenshots: `D:\pm-night\evidence\fix-live-ui\` (`cockpit-view-doctor-then-memory.png` shows Project memory after Set up agents).

## Limits
- Turn blips under 1s with no approval are dropped: a real agent turn that fast would not appear as "Last turn".
- `settle_status` only helps profiles that return `Some(false)` from `busy_marker` (Claude, Cursor, declarative profiles with idle patterns).
- Other agents with a side panel can opt in with `side_panel()`; there is no TOML key yet.
- #11 was verified in jsdom only; a real click in the live app was not repeated (the app is in use).
- Merge hot spots: `pty/registry/spawn.rs` (2 lines in the reader), `CockpitApp.tsx`.
