# Turn summaries and "While you were away"

Branch `feat/away-digest` (cut from `feat/checkpoints` at 87b6823). Ideas 23 and 39.

## 1. What and why

- **Personas:** all. A (developers) want "what did it just do?" without scrolling a terminal; B and C come back to the
  desk or the phone after a coffee and want one calm answer, not a wall of agent output.
- **Stories:**
  - "Claude Code edited 4 files (+86 −12), you approved 1 command, now waiting: allow `npm test`?" sits on the Receipt
    tab ("Last turn") and in the tooltip of the agent's sidebar row.
  - After 5 idle minutes I come back; the cockpit shows a counter of how many agents worked, one line each, what needs
    me, and a "Got it". On the phone, a compact card sits at the top of the All view.
- **Agent-agnostic:** nothing here knows an agent. Turns come from pane status transitions (the same ones checkpoints
  use), files from the checkpoint diff, approvals from the answer path every agent shares, the end prompt from the
  generic prompt detector, receipt facts from the run store. The agent's display name comes from
  `TuiProfile::context().label` (TOML agents declare `label`); a profile with no label shows its id. Plain shells
  (`Features::kind == Shell`) have no turns, so they get no summaries (by kind, not by name).

## 2. Design

### Feature 1: `TurnSummary` (`src-tauri/src/turn_summary/`)
| File | Role |
|---|---|
| `model.rs` | `TurnSummary { paneId, agent, turn, startedAtMs, endedAtMs, durationMs, files, approvals, end, receipt, sentence }`, `assemble`, `render_sentence` (pure) |
| `tracker.rs` | turn state machine over pane status; keeps answered prompts; builds and stores summaries; `Env` trait (fake in tests) |
| `store.rs` | last 100 per pane in memory + ring file `<project>/.puppet-master/turn-summaries.json` (atomic write; damaged file = empty ring) |
| `mod.rs` | glue: live `Env`, worker thread, `turn://summary` Tauri event, SSE `event: turn-summary` |
| `commands/turn_summary.rs` | Tauri `turn_summaries(paneId)` (oldest first) |

- **Turn model.** A turn opens when the pane starts `running` and stays open across `waiting_input` (an approval resumes
  the same turn); it closes at `idle`/`error`. Every rest point yields a summary of the turn so far that replaces the
  earlier one of the same turn (one entry per turn), so "now waiting: ..." appears the moment the agent stops.
- **Files +/-.** `checkpoints::engine::turn_changes(project, pane, started_at)`: the newest checkpoint at or within 2 s of
  the turn start ("before turn n"; if that snapshot was debounced or unchanged, the previous "after" is the same tree)
  compared with the newest checkpoint at turn end ("after turn n"). `diff` was refactored to share `changes_between`.
  No baseline (checkpoints off, git missing, shell) means `files: null` and the sentence says "worked for 3m" instead of
  guessing. The summary waits (max 25 s) for the checkpoint worker to settle: one line in
  `checkpoints/runtime.rs` calls `turn_summary::checkpoint_settled(pane)` after an "after" capture.
- **Approvals.** `agent_runs/routes/answer.rs::route_answer` is wrapped: it reads the prompt on screen before the answer
  is sent (kind `command` when the detector found a command, else its kind; short text), and records a delivered answer
  on the open turn (`allow` / `deny` / `answer` for questions). Replays are de-duplicated by prompt id. This covers the
  cockpit (`answer_pane_prompt`), the phone and MCP `answer_prompt` (pane id given or returned).
- **Sentence** (`render_sentence`): `<agent> edited N files (+a −d)` | `made no file changes` | `worked for 3m` (unknown),
  then `you approved N commands|requests`, `you declined N ...`, the receipt (`all checks passed (3/3)`,
  `checks failed (1/3 passed)`, `no checks ran`, `the run failed|was cancelled`), then `now idle` |
  `now waiting: <prompt>` | `now waiting for you` | `ended with an error`. Prompts are shortened to 80 characters.
- **Surfaces.** Cockpit Receipt tab: "Last turn" block (`LastTurn.tsx`) above the run receipt (it shows even for panes with
  no run). Sidebar row: native `title` tooltip with the sentence. State comes from `hooks/useAway.ts` through
  `away-context.ts` (a context, so the shared `useCockpit`/`CockpitApi` did not change).

### Feature 2: "While you were away"
- **Presence** (`presence.rs`, pure state machine + Windows FFI). `GetLastInputInfo` + `GetTickCount` through a minimal
  `extern "system"` block (the `windows` crate is not a dependency; no new dependency was added). Away = idle for
  `away_minutes` (setting, default 5, 0 = off; read live from the public settings). A 5 s poller thread
  (`start_presence`, one line in `lib.rs`) feeds `tick(now, idle)`. The cockpit reports its focus with
  `presence_cockpit_focus(bool)` (also `true` when it opens). The machine fires once when the user is active again AND the
  cockpit is focused/open, with `since` = when input stopped (kept if the user leaves again before looking).
  Other OSes have no idle reading: never "away", so no false digests.
- **Digest** (`digest.rs`): `Digest { sinceMs, nowMs, agentsWorked, turns, lines[], needsYou[] }`, one line per agent
  (its latest summary since the away start, newest first, at most 12 lines, the counter is exact), `needsYou` from the
  panes now in `waiting_input`/`error` with the prompt text. On a return with at least one line the app emits
  `digest://ready` and keeps it as pending (`away_digest_pending`, `away_digest_dismiss`) so a cockpit opened later still
  gets it. An empty digest shows nothing.
- **Desktop UI** (`AwayDigest.tsx`): a calm full-pane layer over the main area (above Inbox/pane view): Doto counter,
  "N agents worked while you were away", "Away for 47m", one line per agent with the state glyph, "Needs you" rows that
  open that agent, "Got it" (primary, also Escape) and "Open Inbox". Monochrome per STYLE.md, 200 ms fade, none with
  reduced motion.
- **Phone** (`GET /digest?since=<ms>&seen=1`, `bridge/routes/digest.rs`, logic in `turn_summary/http.rs`). Authorised like
  every bridge route (a remote caller needs a paired bearer token). The device is the paired device behind the token
  (`MobilePairingStore::device_id_for_token`), `local` for loopback tooling. `since` defaults to the device's stored last
  visit (persisted in `<app data>/digest-last-seen.json`); `seen=1` stores "now". The PWA `DigestCard.tsx` sits at the top
  of the All view (`AgentsOverview.tsx`, one line), fetches on mount and on return from background after 30 s, stays quiet
  when the last visit was under 2 minutes ago, and keeps the card until "Got it" (also across tab switches). Terminal and
  input paths are untouched. `BridgeClient.digest?` is optional so existing mocks still type-check.
- **Settings:** `away_minutes` (schema, public settings, Settings > General "Away digest after (minutes)").

### OSS
None added. No new crate. Windows API through FFI (system DLLs).

### File-level overlap with other branches
Shared files touched, all with small additive edits (mount lines, new fields, one new function), except where noted:
- Rust: `lib.rs` (mod, 4 commands, `start_presence`), `commands/mod.rs`, `bridge/sse.rs` (one line in
  `push_pane_status_sse`), `bridge/mod.rs` (re-export line), `bridge/observe.rs` (two accessors appended),
  `bridge/routes/mod.rs` (mod + one `or_else`), `agent_runs/routes/answer.rs` (**wrapper around `route_answer`**: a branch
  that edits answer routing will conflict here), `mobile_pairing/mod.rs` (one method), `checkpoints/engine.rs`
  (**`diff` refactored** into `changes_between`, plus `turn_changes`), `checkpoints/runtime.rs` (one method, one hook line),
  `checkpoints/tests.rs` (two tests appended).
- TS: `CockpitApp.tsx` (provider + digest layer), `cockpit/PaneView.tsx` (Last turn, 3 lines), `cockpit/Sidebar.tsx`
  (tooltip), `styles/cockpit.css` (appended blocks, `position: relative` on `.ck-main`), `lib/cockpit-api.ts`,
  `lib/bridge-types.ts`, `lib/bridge.ts`, `lib/bridge-settings.ts`, `lib/settings.ts`, `shared/src/schemas.ts`,
  `SettingsPanel.tsx`, `settings/tabs/GeneralTab.tsx`, `mobile/AgentsOverview.tsx`, `dev/fixtures.ts`,
  `dev/mock-tauri.ts`, `dev/types.ts`.
- Branches it is likely to overlap: `feat/checkpoints` (base, same files), any branch adding a setting (schemas,
  bridge-settings, settings.ts, SettingsPanel, GeneralTab), any branch touching `answer.rs`, `push_pane_status_sse`,
  `CockpitApp.tsx`/`PaneView.tsx`/`Sidebar.tsx`, `lib.rs` command list, or the phone All view. No MCP catalog change.

## 3. Evidence

All commands run in `D:\pm-away-digest` on 2026-10-14 (`CARGO_BUILD_JOBS=2`).

**Rust** (`cd packages/app/src-tauri; cargo test --lib`):
```
test result: FAILED. 1069 passed; 1 failed; 2 ignored; 0 measured; 0 filtered out
   (the one failure is the known pre-existing bridge::tests::definitions::approve_requires_app_session)
```
New: `cargo test --lib turn_summary` -> `43 passed; 0 failed`; `cargo test --lib checkpoints` -> `18 passed; 0 failed`
(two new: `turn_changes_diffs_before_turn_against_after_turn` with a real git folder, a baseline-less pane and a skipped
"before"; `turn_changes_is_empty_when_nothing_changed`).
- Summary assembly from a synthetic checkpoint diff (4 files +86 −12) + approvals + end state:
  `assembly_from_a_synthetic_checkpoint_diff_approvals_and_end_state`; the documented sentence is asserted verbatim.
- Sentence rendering, 18 cases in `sentence_tests.rs`: no files, unknown files (duration), only approvals, approved and
  declined, error end, waiting with and without prompt, long prompt shortened, receipt passed/failed/unchecked/run failed/
  cancelled, questions not counted, durations, whitespace, file-stat caps.
- Tracker with a fake env: `a_turn_across_an_approval_is_one_turn_and_the_last_summary_wins`,
  `the_next_turn_after_idle_gets_the_next_number_and_no_old_approvals`, `plain_shells_have_no_turns`,
  `answers_are_counted_once_and_only_inside_a_turn`, `end_states_and_receipt_facts_reach_the_sentence`,
  `a_pane_that_is_gone_has_no_summary`.
- Ring file: `ring_keeps_the_last_100_per_pane_in_memory_and_on_disk` (105 in, 100 kept, oldest dropped, reload equal),
  `the_ring_file_keeps_only_the_most_recent_panes` (25 panes -> 20), `the_same_turn_replaces_its_entry`,
  `a_damaged_ring_file_is_an_empty_ring_and_is_rewritten`, `summaries_survive_a_restart_through_the_ring_file`.
- Presence state machine: `presence_goes_away_after_the_threshold_and_returns_when_the_cockpit_is_focused` (active ->
  away at 5 min -> back, waits for focus -> digest once), `..._when_the_cockpit_already_has_focus`,
  `..._keeps_the_earliest_start_...`, `..._threshold_is_configurable_and_zero_turns_it_off`,
  `a_return_shows_a_digest_only_when_something_happened`, `the_idle_reading_is_sane_on_this_machine` (calls the real
  `GetLastInputInfo` on Windows).
- `GET /digest?since=` request -> response (`get_digest_returns_what_happened_since_and_remembers_the_device`):
  `since=50000` -> 200 `{agentsWorked:1, lines:[{paneId:"pb", sentence:"Claude Code worked for 0s, now idle."}],
  needsYou:[{prompt:"allow \`npm test\`?"}], empty:false, previousSeenMs:null}`; `seen=1` stores the device's visit; the next
  call without `since` starts at it; another device has its own (empty). `since=yesterday` -> 400 `INVALID_ARGUMENT`;
  POST and other paths -> not this route.
- Auth: `digest_route_needs_a_paired_device_when_called_remotely`: remote without token -> 401 `pairing_required`; the PWA
  proxy header on loopback -> 401; plain loopback -> allowed; a freshly paired device's token maps to its device id, a
  stranger's token to none.
- **Fails before, passes after:** `git checkout 87b6823 -- src/checkpoints/engine.rs` (the base's engine) then
  `cargo test --lib turn_changes` -> `error[E0425]: cannot find function turn_changes in module engine` (the tests and
  `runtime.rs` need it); restoring HEAD's `engine.rs` -> `test result: ok. 2 passed`.

**Frontend** (`npm test --workspace=@puppet-master/app`): `Test Files 51 passed (51), Tests 411 passed (411)`.
New: `AwayDigest.test.tsx` (11: counter, titles, lines, needs-you rows and fallbacks, Got it, Escape, open pane, Open
Inbox), `LastTurn.test.tsx` (7: Last turn block, empty and unknown-files states, context read, `useAway` loading the
newest summary, event ordering, digest delivery, pending digest on open, dismiss), `DigestCard.test.tsx` (11: no
`digest`, empty, short absence, content, singular, cap, tap, dismiss, remount, resume after 31 s vs 5 s).
`npx tsc --noEmit -p packages/app`: no output (clean).

**Screenshots** (headless Edge, `node scripts/evidence/screenshot.mjs --out D:\pm-night\evidence\turn-summary-and-away-digest`):
- `cockpit-digest.png`: the digest over the cockpit (scenario `cockpit-digest`).
- `cockpit-last-turn.png`: Receipt tab with "Last turn" (scenario `cockpit-last-turn`).
The older `cockpit-receipt` scenario clicks and screenshots too early and sometimes captures the Terminal tab (it did so
without my change as well); `cockpit-last-turn` waits for the block.

## 4. Limits / follow-ups
- Idle detection is Windows only; other systems never show the digest. (macOS `CGEventSource`, X11 idle would slot into
  `presence::system_idle_ms`.)
- Files per turn follow checkpoints: with checkpoints off, git missing or an agent that opts out, the sentence reports
  duration instead of files. Two panes in one folder share the folder, so each turn's file list also contains the other
  pane's edits (same attribution limit as checkpoints).
- The "before" baseline is picked by time (a checkpoint within 2 s of the turn start). A turn whose first edit lands in
  those 2 s while the "before" snapshot is still running could undercount by that edit.
- The receipt is read when the turn settles; a run receipt written a moment later is picked up on the next summary of
  that pane, not retroactively.
- An answer sent by run handle without a pane id is recorded only when the reply carries `pane_id` (the bare-pane and
  cockpit paths always do).
- The sidebar tooltip is the native `title` (no component test, covered by the shared `useAway`/context tests); the
  phone card appears when the All view is open (mount or resume), not on other tabs.
- The ring file lives in the project's `.puppet-master/` like the checkpoints repo; a project that commits everything
  will see that folder as untracked (pre-existing from checkpoints).
- "Auto-handled by policy" lines (idea 39) are not separate yet: auto-approvals go through the same answer path and so
  already count as "you approved" in the sentence (it says "you" even when a policy answered).
