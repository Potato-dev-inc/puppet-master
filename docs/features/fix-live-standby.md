# Fix: warm-standby churn (live bug #12)

## Root cause (evidence)
Live `GET /standby` on the running dev app (no restart) showed, for the default-project keys:
`codex ... reason=needs_attention fails=3 cold`, `cursor_agent ... reason=dialog fails=3 cold`, and the same
agents again under other folders (`c:/users/ren-pc/desktop`, `d:/pm-demo`, `d:/pm-integration/packages/app/src-tauri`),
`stats: spawned 27, discarded 19`.

- The classification is not wrong. Codex 0.159 starts on the update MENU ("Update now / Skip / Skip until next
  version", fixture `tui/codex/fixtures/update_prompt_live.txt`), which blocks the composer; the plain "Update
  available!" banner above a ready composer is already Ready (`standby_ready_live.txt`). Cursor stops at "Workspace
  Trust Required" in an untrusted folder (`tui/cursor/fixtures/workspace_trust_visible_pending.txt`). The pool
  never answers those (rule), so both are correctly unhealthy ~2s after spawn.
- The bug is the retry policy. Backoff and the 3-strike cold state were per `PoolKey` (agent + project + options).
  The Preload default project moves (settings `project_path`, registry project), and every new folder is a fresh
  key with a fresh 15s/30s schedule and three more spawns. A settings save also cleared all backoff, and so did
  any run (`note_run`). Net effect: spawn, kill after 2s, repeat every ~15-30s, indefinitely.

## Fix (`standby/pool.rs`)
- Failure state per AGENT across all keys (`AgentBackoff`): exponential 15s, 30s, 60s... capped at 5 min; after 3
  consecutive unhealthy workers the agent goes cold. A worker reaching Ready clears it.
- `retry_at_ms` survives every reset (a run, a settings change), so the respawn rate can never beat the schedule.
  A run or a real settings change lifts "cold" for exactly one more try; one more failure goes straight back.
- `set_config` with an unchanged config no longer clears backoff.
- `StatusView.agents_blocked` carries the reason in plain words; Settings > Preload shows
  "Codex couldn't stay warm: it stops at a login or update screen (finish that once yourself). Not warming it
  until you open it."
- Generic code still never matches agent names; screen knowledge stays in the TuiProfiles.

## Evidence
- New Rust tests (standby/tests.rs): `an_unhealthy_agent_backs_off_then_stops_and_says_why`,
  `a_new_project_folder_does_not_restart_the_agent_schedule`,
  `a_run_or_changed_settings_allow_one_more_try_but_never_before_the_backoff`,
  `reaching_ready_clears_the_agent_failures`. Fail-before: with the agent check in `wanted()` disabled,
  the folder-churn test spawns 40 workers instead of 3 and the reset test fails (left 1, right 0).
- Component test: "says why an agent couldn't stay warm".
- `cargo test --lib`: 1500 passed, 0 failed. `cargo test --bins`: 33 passed. vitest: 970 passed. `tsc --noEmit`: clean.

## Limits
- I could not capture a fresh warm Codex/Cursor screen: the bridge refuses `spawn_agent` in agent mode. The
  classification was reasoned from the existing live fixtures plus the live `blocked` reasons.
- A user who has not skipped Codex's update menu or trusted the folder in Cursor gets no warm pane for that agent
  until they do; this is intended (never auto-answer dialogs).
