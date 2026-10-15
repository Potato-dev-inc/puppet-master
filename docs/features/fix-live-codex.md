# fix-live-codex: Codex live bugs #3, #4, #7

Branch `fix/live-codex` (cut from `integration/night` 93c8f2f). Evidence: the first live run of the night build
(`D:\pm-night\NIGHT-LOG.md`, "First LIVE run"), reproduced against the running app and the real Codex CLI 0.159.3
(update to 0.162.1 offered) in `D:\pm-demo`.

## What and why

Visible Codex runs were unusable for every persona: the task sat unsent, a run "completed" on an update menu, and the
doctor's Test failed. All three are fixed in the Codex profile (`tui/codex/*`); the generic code only gained small,
agent-agnostic hooks that any profile (including declarative ones, later) can use.

## Root causes (with evidence)

### #3 Task typed, Enter not submitted
- Reproduced live through the bridge: `POST /panes/:id/input {text, append_newline:true}` (the same path the dispatcher
  uses: write text, sleep `submit_delay` = 50 ms for short text, write `\r`) left the text in Codex's composer, one line
  taller (the Enter became a line break). Pressing Enter by hand then submitted it.
- Timing sweep, Enter after the last character (plus ~5 ms HTTP): 0 ms and 50 ms left the text unsent; 100 ms, 130 ms,
  200 ms, 300 ms and 500 ms submitted. Codex's `paste_burst.rs` has `PASTE_ENTER_SUPPRESS_WINDOW = 120 ms` (read from the
  public source; behaviour matches). Characters arriving in a burst are treated as a paste, and an Enter inside the
  window is part of the paste.
- The same live run also showed Codex collapses a paste over 1000 characters into `[Pasted Content N chars]`, and that
  Ctrl+U clears one line, 16 Ctrl+U in one write clear a 4-line draft and that placeholder, and Ctrl+U on an empty
  composer is harmless.
- A second finding: the emulator marks Codex's full-width rows as wrapped, so `contents()` glues the screen into one long
  line and the composer ends up at the END of a line that starts with an old message (`fixtures/submitted_live.txt`).
  Looking for "the first row starting with `›`" read that old message as the composer; this also made `standby_screen`
  read `NotReady` on any pane with history.

### #4 Run completed on the "Update available" menu
- Reproduced at once: `run_agent` (op 532a1c0f) ended `completed` with the update menu as `assistant_final`
  (`Update available 0.159.3 → 0.162.1 ... 1. Update now ... 3. Skip until next version ... enter continue · esc skip`).
- Nothing recognised the menu: the Codex profile only used it for the standby pool; `startup_manual_approval_required`,
  `dispatch_blocker` and `observe_tui_turn` saw no prompt. The task was typed into the menu, the pane read
  running then idle, and the idle menu text counted as new screen content = the "reply". After Esc the buffered task text
  appeared in the composer, still unsent (see #3).

### #7 Doctor Test failed for Codex
- `puppet-master-doctor` runs a read-only `quick_ask` in `%TEMP%\puppet-master-doctor` (not a git repo).
  `codex --ask-for-approval never exec --json --sandbox read-only "..."` in a non-git dir prints
  `Not inside a trusted directory and --skip-git-repo-check was not specified.`; with `--skip-git-repo-check` it answers.

## Design

Agent-agnostic structure: per-agent knowledge is declared by the profile, generic code only asks.

New `TuiProfile` methods (all with generic defaults, `tui/profile.rs`, types in `tui/types.rs`):

| method | meaning | Codex |
|---|---|---|
| `submit_sequence() -> SubmitSequence{settle, keys}` | pause between the typed text and the submit keys, and the keys | 400 ms, `\r` |
| `composer_state(screen) -> ComposerState` (`Unknown/Empty/HasText/Working`) | what the input box holds | reads the last `›`, `esc to interrupt` |
| `startup_gate(screen) -> Option<StartupGate{id, summary, dismiss}>` | a startup screen (update notice, trust, login, onboarding) | update menu (dismiss = Esc), trust, login, onboarding (no dismiss) |
| `clear_input_sequence`, `verifies_prompt_delivery` (existing) | clear the box; opt in to verified delivery | 16x Ctrl+U, true |

Files:
- `tui/codex/screen.rs`: `composer_state`, `startup_gate`, `CLEAR_COMPOSER`, glue-proof `composer_row` (also used by `standby_screen`).
- `tui/codex/headless.rs`: read-only runs (and read-only resumes) add `--skip-git-repo-check`. Writing runs keep the check.
  A read-only sandbox cannot edit, so the check, which exists to stop edits in an unversioned folder, has nothing to protect.
  This also fixes any read-only `quick_ask`/`run_agent` in a non-git project, not only the doctor.
- `pty/registry/pane_io.rs`: `write_input` uses the profile's settle and keys (`submit_pause`, `write_submit_keys`).
- `agent_runs/delivery.rs`: `DeliveryIo::{enter_settle, composer_state}` (defaults keep other agents unchanged); after the
  settle and Enter, `submit_confirmed()`: `Working` marker or an emptied box = started; text still in the box = not started
  whatever the pane's busy flag says (typing flips an idle pane to "running"), and Enter is sent once more at ~0.4 s
  instead of the full wait. Paste placeholders (`pasted text` / `pasted content`) count as the typed prompt.
- `bridge/startup_gate.rs` (new, generic): `dismiss_startup_gates()` closes gates that declare `dismiss`, one at a time,
  waiting for the screen to change; stops after 4 rounds; never writes for a gate without keys. Wired into
  `bridge/dispatch.rs` (`await_pane_ready` and again before the baseline in `send_task_prompt`).
- A gate that is still up (trust, login, onboarding, or a notice that ignored its keys) is treated like a startup
  prompt: `startup_manual_approval_required` / `wait_for_startup_permission_resolution` park the run in `waiting_input`
  with `{"kind":"manual_approval_required","startup_gate":"<id>","detail":"<summary>. Resolve it in the pane; ..."}`.
  Puppet Master never answers a trust/login screen for the user.
- `pane_prompt::dispatch_blocker`: refuses to type over a gate (`PANE_BLOCKED_BY_PROMPT`, context `startup_gate`).
- `agent_runs/completion.rs::observe_tui_turn`: while the screen is a gate it returns `WaitingInput` and resets its
  evidence, so a startup screen can never be a run's answer (the safety net if a gate shows up after dispatch).
- `tui/conformance.rs`: new rule `submit_and_startup_declarations_are_sane` for every registered and declarative profile.

### Dismissing the update menu: why Esc
The menu footer reads `enter continue · esc skip`. Enter would run **Update now** (a PowerShell installer); option 3
("Skip until next version") writes a setting. Esc skips this launch only. Conformance forbids nothing here by pattern, but
the Codex test pins `dismiss == Some("\u{1b}")` and the generic code never presses Enter on a gate.

### For the coordinator: reconciling with the Claude lead's `fix/live-claude` startup gate
- My hook is `TuiProfile::startup_gate(&self, screen) -> Option<StartupGate>` (+ `StartupGate`, `SubmitSequence`,
  `ComposerState` in `tui/types.rs`, the new `bridge/startup_gate.rs`). If the Claude branch defines a similar trait method
  or struct, keep ONE definition (mine is minimal: `id`, `summary`, `dismiss: Option<&'static str>`), and keep ONE
  call site per place: `bridge/dispatch.rs` (`await_pane_ready`, `send_task_prompt`), `bridge/approvals.rs`
  (`startup_manual_approval_required`, `wait_for_startup_permission_resolution`), `pane_prompt::dispatch_blocker`,
  `agent_runs/completion.rs::observe_tui_turn`.
- Merge hot spots: `tui/types.rs`, `tui/profile.rs`, `bridge/dispatch.rs`, `bridge/approvals.rs`, `pane_prompt/mod.rs`,
  `agent_runs/completion.rs`, `bridge/mod.rs` (one `pub(crate) mod startup_gate;` line), `tui/conformance.rs`.
- A Claude folder-trust dialog is a gate with `dismiss: None`: it parks the run with the trust screen shown, which is the
  rule in `BUGFIX-RULES.md` (surface, never auto-accept).

## Evidence

Commands (in `packages/app/src-tauri` unless noted, `CARGO_BUILD_JOBS=3`):

- `cargo test --lib`: `test result: ok. 1519 passed; 0 failed; 4 ignored` (baseline before the change: 1496).
- `cargo test --bins`: `33 passed; 0 failed`.
- `npx tsc --noEmit -p packages/app`: exit 0. `npm test --workspace=@puppet-master/app`: `111 files, 969 tests passed` (no frontend change).
- Live, real Codex 0.159.3 on a ConPTY, ignored by default:
  `cargo test --lib codex_live -- --ignored --nocapture --test-threads=1` -> `2 passed`.
  - `codex_live_an_enter_50ms_after_the_text_leaves_the_task_unsent`: the bug on the real CLI (the canary: if Codex ever
    stops doing this, the settle can be dropped).
  - `codex_live_closes_the_update_menu_and_the_task_is_submitted_by_one_enter`: gate report
    `GateReport { dismissed: ["update_available"], blocked: None }`, `delivery: Started` after one Enter
    (`enter poll 0: started=true ... composer=Empty`), the reply `PINEAPPLE` and `Worked for 2s` on screen.

New tests (real captures under `tui/codex/fixtures/`: `update_prompt_live`, `composer_unsent_live`,
`composer_unsent_glued_live`, `submitted_live`, `working_live`, `composer_pasted_live`):
- `tui::codex::headless::tests::a_read_only_run_skips_the_trusted_folder_check_and_a_writing_run_keeps_it` (#7)
- `tui::codex::screen::tests::*` (composer states on real screens, gate ids and keys, trust/login never dismissed, banner is not a gate)
- `agent_runs::codex_delivery_tests::*`: a model of Codex's burst window driven by the real captures and the real profile:
  one Enter after the settle submits; an Enter swallowed as a line break is noticed and repeated once; text still in a
  busy-looking pane is never "started"; placeholder is the prompt; **`the_update_menu_never_completes_a_run`** (the
  completion gate test, 81 ticks of running+idle on the real menu, never `Complete`)
- `bridge::startup_gate::tests::*`: dismiss sequence incl. trust never answered, ignored keys, loop cap
- `pty::registry::pane_io::submit_delay_tests::a_profile_can_lengthen_the_pause_before_enter_but_never_shorten_it`
- `tui::conformance` rule `submit_and_startup_declarations_are_sane` over every profile

Fail-before / pass-after: with the Codex wiring reverted (no `--skip-git-repo-check`, settle 0, `composer_state` Unknown,
`startup_gate` None) `cargo test --lib codex` gives
`a_read_only_run_skips_the_trusted_folder_check_and_a_writing_run_keeps_it ... FAILED`,
`the_captures_are_what_the_model_says_they_are ... FAILED`, `the_profile_waits_out_the_paste_burst_so_one_enter_submits ... FAILED`,
`the_update_menu_never_completes_a_run ... FAILED` (4 failed); restored, all pass. The pre-change behaviour on the live CLI is
the canary test above plus the bridge reproduction (op 532a1c0f completed with the menu).

## Limits / follow-ups
- **Codex reply capture is still poor** (not in scope, found while testing): a visible Codex turn's "result" is read from
  the whole screen. Run 29eef236 recorded `⚠ 3 warnings · f2 to view` as its answer, and on a pane with history the
  glued screen line is returned whole. Codex needs `is_chrome_line`/reply extraction from its `• reply` + `Worked for Ns`
  markers; `Worked for` + an empty composer is a precise turn-end signal.
- The row-glue is specific to Codex's full-width rows; other consumers of `read_snapshot` for Codex (`contents()`) still see
  glued lines. A profile flag to read rows individually would fix them at the source.
- Not covered because no capture exists: a "release notes" / "what's new" screen. Add its text to `startup_gate` with a
  real fixture when one is seen. Trust/login/onboarding phrases come from the existing hand-written lists, not live captures.
- Warm standby panes still treat the update menu as `NeedsAttention` (bug #12, standby churn); they could use
  `startup_gate().dismiss` too.
- Declarative (TOML) agents get the defaults; `submit_sequence`/`startup_gate` have no TOML fields yet.
- The first gate pass costs ~1 s on a Codex start with the menu (screen-change wait plus a 300 ms settle).
