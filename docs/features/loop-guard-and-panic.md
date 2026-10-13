# Loop guard ("this agent seems stuck") and the panic button

Branch `feat/loop-guard`, cut from `feat/agent-doctor`. Two features. The MCP catalog is untouched.

## 1. Loop and burn detector

### What and why
Personas B (vibecoders) and C: an agent loops on the same error for an hour and burns credits while the user is away or does not notice. No vanilla agent app watches for this.

User story: the pane header (and the Inbox) shows a quiet card, "Claude Code seems stuck: the same error 4 times.", with the lines that show it and four ways out: **Give it a hint** (selects the pane and focuses the terminal), **Interrupt** (`interrupt_pane`), **Ask another agent** (opens the sidebar "+" menu), **Dismiss** (hides it for this episode). Monochrome, no red.

Agent-agnostic: the detector works on screen text and status only. Error-likeness comes from `TuiProfile::is_error_line` (a generic default; declarative agents extend it with TOML `[tui] error_regex`). Generic code never matches agent names.

### Design
- `src-tauri/src/loop_guard.rs` (pure `Guard`, fake-clock testable; process-wide instance + sink for production). It is fed by the existing activity ticker: `pane_activity::tick_with` calls `loop_guard::observe_pane` for each pane whose screen or status changed (nothing in the PTY reader, nothing for a silent pane), and `loop_guard::tick` runs once per ticker pass for the time-based signal.
- Per pane it keeps turn records (a turn = `running` -> `idle`/`error`). Only lines that are NEW since the previous observation count (multiset difference of normalised lines), so an old error that merely stays on screen never repeats. Normalisation drops numbers, durations, hashes, spinner glyphs, punctuation and (for errors) paths; lowercased words remain.
- Signals (defaults conservative):
  - `repeated_error`: the same error line (equal after normalisation or `strsim::normalized_levenshtein >= 0.85`) in each of the last N turns (N = 4).
  - `repeated_action`: the same tool rows in the same order for N turns, none with an error or a progress marker.
  - `no_progress`: 2N busy turns (tool rows or errors) inside ten minutes, none with a progress marker (passed, built, created, updated, committed, a check mark, ...).
  - `stalled`: `running` continuously for X minutes (default 20) with no new distinct line (a changing spinner timer is not new).
- Tool rows: `TuiProfile::is_tool_activity_line` or a generic bullet + verb pattern (Read/Edit/Bash/Update/Grep ...; verbs, not agent names). Agents that print neither (status-level agents) can still trigger `repeated_error` and `stalled`.
- Output `LoopSignal { pane_id, kind, evidence_lines (max 3), turns, since, summary }`. Raised ONCE per episode: it re-arms only after the detection clears (a different outcome, a pane that leaves `running` for `stalled`). Dismiss hides the current episode; the next one alerts again. Delivery: Tauri events `loop://signal` / `loop://clear`, SSE `loop-signal` (`push_loop_signal_sse`, `{pane_id, signal|null}`), commands `loop_status(paneId)`, `loop_signals`, `loop_dismiss`.
- Settings (`loop_guard.json` in the app data dir; commands `loop_settings_get/set`; UI in Settings > General, `LoopGuardSettings.tsx`): on/off, sensitivity (3, 4, 6 or 8 repeats), minutes for `stalled` (10 to 60). Turning it off clears open cards.
- TOML: `[tui] error_regex = ['BOOM-\d+']`, matched per line on top of the generic set; a bad regex fails the definition with `AGENT_DEF_BAD_REGEX` at `tui.error_regex.N`.
- Frontend: `lib/loop-guard.ts` (types, `useLoopSignals`, context), `components/cockpit/StuckCard.tsx` (`StuckCard`, `PaneStuckCard`), mounted in `PaneView` (under the header) and `Inbox` (all stuck panes), `styles/loop-guard.css`.
- OSS: `strsim` 0.11 (MIT), already in the lockfile as a transitive dependency.

## 2. Panic button

### What and why
One action interrupts every agent that is working, then a sheet says what was stopped ("Stopped 2 agents"). A big emotional safety net for B and C.

### Design
- `src-tauri/src/panic.rs`: `stop_running(candidates, interrupt)` is pure; `stop_all` collects the registry's visible panes and sends the same Ctrl+C `interrupt_pane` sends, only to panes whose status is `running`. Deliberate deviation: plain shell panes are skipped (a dev server in a shell is not an agent burning credits). Result: `StopReport { stopped: [{pane_id, agent_type, cwd}], failed: [[pane_id, reason]] }`.
- Entry points:
  - palette (Ctrl+K) "Stop all agents" -> command `stop_all_agents` -> `PanicSheet`;
  - tray item "Stop all agents" and the global shortcut both call `stop_all_and_show` (stop, open the cockpit, emit `panic://stopped`; the cockpit opens the sheet);
  - phone: `POST /panes/stop-all` (`bridge/routes/panes.rs`), Menu sheet button "Stop all agents" with a confirm (`MobileSheet.tsx`, `BridgeClient.stopAllPanes`). The terminal is untouched.
- Route auth: the route is covered by the bridge's normal gate (`authorize_bridge_request`): a remote peer or a proxied PWA request needs a valid pairing bearer token; the desktop app itself (loopback, no proxy marker) passes. MCP sessions are refused (`route is not exposed to MCP sessions`, the route is not a catalog tool), so one agent cannot stop its siblings.
- Global shortcut: `tauri-plugin-global-shortcut` 2 (MIT/Apache-2.0), registered from Rust only (no JS permission added). OFF by default; Settings > General toggles Ctrl+Alt+Shift+P (hard-coded combination; not yet rebindable).
- Checkpoints: `feat/checkpoints` is not part of this branch, so there is no checkpoint integration. TODO hook: in `panic.rs::stop_running`, take a checkpoint of each stopped pane's project before interrupting.

## Overlaps with other branches (file level)
- `feat/agent-doctor` (base): `pane_activity.rs` (two added lines), `bridge/sse.rs`, `bridge/mod.rs` export line, `lib.rs`.
- `feat/checkpoints`: `panic.rs` (future hook only), `lib.rs`, `Cargo.toml`.
- Any branch adding commands, modules or plugins: `lib.rs` (module lines, command list, plugin line, init lines), `commands/mod.rs`, `Cargo.toml`, `Cargo.lock` (global-shortcut pulled in `global-hotkey`, `keyboard-types`, `x11rb` ...; cargo also re-resolved a few unrelated entries, so expect a lockfile conflict: take either side and re-run cargo).
- Any branch adding `[tui]` TOML keys: `tui/declarative/{schema,validate,screen,profile}.rs`; `tui/profile.rs` (new `is_error_line` default method).
- `tray.rs` (new menu item and its test indices), `bridge/routes/panes.rs`, `bridge/tests.rs` (appended).
- Cockpit UI: `CockpitApp.tsx`, `Palette.tsx`, `PaneView.tsx`, `Inbox.tsx`, `Sidebar.tsx` (a 5-line listener for "open the + menu"), `settings/tabs/GeneralTab.tsx` (one line), `dev/mock-tauri.ts`; phone: `PwaApp.tsx`, `MobileSheet.tsx`, `lib/bridge.ts`, `lib/bridge-types.ts`.

## Evidence
Commands run in `D:\pm-loop-guard`, 2026-10-14.

- `cargo test --lib` (in `packages/app/src-tauri`, `CARGO_BUILD_JOBS=3`): `test result: FAILED. 1059 passed; 1 failed; 2 ignored`; the one failure is the known `bridge::tests::definitions::approve_requires_app_session`. Baseline on the base branch was 1035 passed.
- New Rust tests (`cargo test --lib loop_guard` -> `18 passed; 0 failed`):
  - true loops: `the_same_error_across_turns_is_a_loop_with_evidence` (a), `near_duplicate_errors_with_changing_numbers_and_paths_match` (a), `the_same_steps_over_and_over_without_progress_is_a_loop` (b), `running_for_twenty_minutes_with_no_new_lines_is_stalled` (d), `busy_turns_without_any_progress_in_a_short_window_flag_no_progress` (c).
  - healthy sessions, NOT flagged: `healthy_typescript_session_with_varied_errors_that_get_fixed`, `healthy_rust_session_repeating_one_error_twice_then_passing`, `healthy_long_refactor_and_a_chat_with_no_tools`; also `an_old_error_that_merely_stays_on_screen_never_repeats` and `a_long_but_busy_run_is_not_stalled`.
  - hysteresis and settings: `one_signal_per_episode_and_it_re_arms_after_it_clears`, `dismiss_hides_the_episode_but_not_the_next_one`, `sensitivity_and_off_switch`, `turning_the_guard_off_clears_open_cards`.
  - TOML: `error_regex_in_a_toml_definition_teaches_the_guard` (taught profile loops, the same definition without the table does not), `a_bad_error_regex_is_rejected_at_load`; plus `generic_error_detection_has_sane_edges` and the normaliser test.
  - panic: `panic::tests::interrupts_only_running_agent_panes` (running agents only; idle, waiting, error and a running shell untouched), `a_failing_interrupt_is_reported_and_does_not_stop_the_rest`, `nothing_running_stops_nothing`.
  - route auth (`bridge::tests`): `stop_all_route_is_authorised_like_other_remote_control_routes` (no token from a remote peer -> `401 pairing_required`; proxied PWA request without a token -> 401; bad token -> 401/503; the desktop app on loopback -> allowed), `stop_all_route_is_not_exposed_to_mcp_sessions` (`AUTHORIZATION_DENIED`), `stop_report_wire_shape`. `tray::tests` updated for the new item (`parse_command("stop-all")`).
  - Not covered: the live route with a real `AppHandle` (no in-process harness exists, as for the doctor's POST route) and the real global shortcut registration (no GUI allowed).
- `npm test --workspace=@puppet-master/app`: `Test Files 51 passed (51)`, `Tests 394 passed (394)` (base 386). New: `StuckCard.test.tsx` (7: card sentence, evidence and four buttons, action wiring incl. hint/ask/dismiss, nothing when not stuck, Inbox shows it, `useLoopSignals` load/event/clear/dismiss, panic sheet with stopped and failed rows, empty case, palette "Stop all agents"), `MobileSheet.test.tsx` (1: the phone button asks for confirmation before stopping).
- `npx tsc --noEmit -p packages/app`: no output (clean).
- Fails before, passes after:
  - Rust: with the "new lines only" step defeated in `loop_guard.rs` (`let mut remaining = HashMap::new()` instead of the previous tail), `cargo test --lib loop_guard` -> `5 failed` (`an_old_error_that_merely_stays_on_screen_never_repeats`, both healthy sessions, hysteresis, dismiss); restored: 18 pass.
  - Frontend: with `Inbox.tsx` stashed, `npx vitest run StuckCard` -> `Tests 1 failed | 6 passed` (the Inbox card test); restored: all pass.
- Screenshots (harness; `git merge 7651d54` done first; scenarios `loop-stuck`, `panic-sheet`; mock data in `dev/mock-tauri.ts`): `D:\pm-night\evidence\loop-guard\loop-stuck.png` (card under the pane header with two evidence lines and four actions) and `panic-sheet.png` ("Stopped 1 agent", Codex). Both viewed and correct. The phrases and the signal are fixtures.

## Limits and follow-ups
- Not run against a live agent: no real loop was provoked (no tokens, no GUI). The detector is tested on synthetic but realistic screen-tail sequences; thresholds may need tuning on real sessions (that is why defaults are conservative: 4 repeats, 20 minutes).
- The ticker samples at most once a second, so a turn shorter than about a second can be missed.
- Screens that redraw in place and reprint errors can inflate or hide "new" lines; the multiset diff is tolerant of spinner changes but not perfect when more than the whole tail scrolls between two ticks.
- `repeated_action` and `no_progress` need tool rows; status-level agents (Claude, Codex, OpenCode print no profile-taught rows) rely on the generic bullet + verb pattern, which may not match every UI. `error_regex` / `is_tool_activity_line` teach more.
- `stalled` can fire for a legitimately long silent build; the card wording is neutral ("no new output for N minutes") and Dismiss is one click.
- The loop guard is Tauri-only for display; the phone receives the `loop-signal` SSE event but does not render it yet.
- The panic shortcut combination is fixed (Ctrl+Alt+Shift+P) and off by default; "configurable" is a follow-up (needs a key-capture control).
- Panic skips shell panes by design; no checkpoint is taken (hook documented above).
