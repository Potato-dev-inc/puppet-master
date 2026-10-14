# Fix: Claude Code approval cards and the folder-trust gate (live bugs 1 and 2)

Branch `fix/live-claude` (cut from `integration/night` 93c8f2f; backup `backup/fix-live-claude-start`).
Found in the first LIVE run of the night build (Claude Code 2.1.299, `D:\pm-night\NIGHT-LOG.md`, "First LIVE run").

## What and why

Personas A, B, C. The agent stops to ask permission; the Inbox card, the desktop buttons and the phone push
"Allow" are how a person answers without hunting for the terminal. Two Claude dialogs had no card.

1. **No approval card for Claude's Bash dialog.** The pane correctly read `waiting_input`, but
   `pane_prompt::detect` returned null, so the Inbox only offered "Open terminal": no risk card, no buttons, no
   phone Allow.
2. **A visible run in an untrusted folder typed the task into the folder-trust dialog.** The task text landed in
   "Quick safety check ... Yes, I trust this folder", delivery reported `NotVisible`, the run fell back to
   headless, and the headless `npm test` was denied.

Agent-agnostic: nothing generic names Claude. Detection lives in the profile (`tui/claude/prompt.rs`). A "startup
gate" is any prompt whose `kind` is in `pane_prompt::STARTUP_GATE_KINDS` (today `workspace_trust`, which Cursor
already reports); the Codex lead (or a TOML `[[tui.prompt]]` definition) plugs its trust dialog in by returning that
kind from `detect_prompt`. See `tui/README.md`, "Startup gates".

## Root causes (with evidence)

- **Bug 1.** `tui/claude` had no `detect_prompt`, so Claude used `generic::detect_prompt`, which only looks for
  `allow once`, `approve this command`, `allow all`, `always allow`. Claude 2.1.299's dialog says none of those:
  it is `Do you want to proceed?` + numbered rows. Real capture (`fixtures/bash_multi_cd_2_1_299.txt`): Claude pads
  every dialog row with spaces to the pane width, so `vt100::Screen::contents()` glues rows together
  (`... 1. Yes<114 spaces>2. Yes, and switch to auto mode ...<47 spaces>3. No`). A line-based parser would never
  see the options. The wording also varies: the multi-cd Bash dialog has 3 rows, `npm test` has 4 (`2. Yes, and
  don't ask again for: npm test *` pushes `No` to row 4), the edit dialog's second row is `switch to accept edits`.
- **Bug 2.** The trust dialog (`fixtures/trust_dialog_2_1_299.txt`) has no `y/n`, no `1. Yes`, no `allow once`: the
  generic status heuristic read it as `running`, `startup_manual_approval_required` (which needs a profile-detected
  prompt or a `waiting_input` plus certain phrases) said no, nothing parked the run, and `send_task_prompt` typed
  the task. Claude's `detect_prompt` returned null, so `dispatch_blocker` could not refuse either. (The existing
  parking code from the Cursor trust work was already correct; it just never fired for Claude.)

## Design

- `tui/claude/prompt.rs` (new, with `prompt_tests.rs`). Detection is by structure, not by option text:
  - anchor `Do you want to ...?`, numbered rows `1.`..`N.` found by marker (so glued rows work), the `❯` pointer on
    one of them, a plain `Yes` and a `No` present, and nothing but the footer (`Esc to cancel ...`) below it
    (an old dialog scrolled up under real output is ignored; a numbered list in the model's reply has no pointer).
  - **Command approval** (`command_approval`): the command is the text between the two dashed rules under a
    `... command` header (a leading `│` bar stripped); `reason` is the row under it. The existing
    `approval_policy::leading_cd_dir`, `command_summary` and `prompt_risk` run on it unchanged.
  - **File edit/create** (`file_edit`): `path`, `summary` ("Edit sum.js", "Create notes.tmp"); the id includes the
    diff so two edits to one file are two prompts.
  - Anything else with the same structure is `tool_approval` (question as the summary).
  - Choice ids: plain `Yes` -> `allow_once`; `No` -> `deny`; one widening row -> `allow_all`. When both exist,
    "don't ask again for X" (the persistent rule for that command) takes the slot over "switch to auto mode"; the
    broader row is never offered. `allow_all` is only listed with `allow_broad` and is refused without the opt-in
    (unchanged SEC-7 rules; conformance test passes).
  - **Answering** presses arrow keys from the highlighted row (`write_input` then adds the Enter). A number key
    selects and submits by itself, and the Enter that follows could answer the *next* dialog; arrows + Enter is one
    atomic selection. Verified live on a real pane (below).
  - The old wording (`Allow once`, `Always allow this command?`) still goes through the generic fallback and keys.
- **Trust gate**: the trust dialog is a `workspace_trust` prompt with `choices [trust, deny]`, `folder`, summary
  "Claude Code asks to trust D:\pm-demo", labels "Trust this folder" / "Don't trust", an id that ignores wrapping.
  The pointer starts on `No, exit`, so `trust` is one Down + Enter and `deny` is Enter alone. `classify_screen` reads
  the dialog (and every permission menu) as `waiting_input`, which is what makes `pty://prompt`, the Inbox and the
  push fire for it.
- Generic changes (small, agent-neutral):
  - `pane_prompt/mod.rs`: `startup_gate()` + `STARTUP_GATE_KINDS`; optional `labels` map on a prompt (custom
    button text, also in `required_action.options`); a prompt with a named `identity` is identified by it alone
    (so listing `allow_all` on request does not change the id); the `do you want to` region anchor; `path` into the
    risk input.
  - `prompt_risk/mod.rs`: kind `file_edit` (inside the project: low; outside it: high; secrets/keys/logins: high;
    unknown: medium).
  - `bridge/approvals.rs`: `wait_for_startup_permission_resolution` remembers the startup gate it saw. If the pane's
    agent exits (or the pane disappears) while a gate was showing, the run fails with `WORKSPACE_TRUST_DECLINED`
    (not recoverable). It is not a pre-turn failure, so there is **no headless fallback** into the folder the user
    just refused (`fallback.rs` test updated).
- Flow after the fix, visible run in an untrusted folder: spawn -> `await_pane_ready` -> the dialog is detected ->
  the run parks as `waiting_input` with `required_action` (kind `workspace_trust`, options Trust / Don't trust) ->
  the user answers in the Inbox, on the phone or with `answer_prompt` -> the wait ends when the dialog is gone ->
  `wait_for_pane_idle` -> the task is typed. Puppet Master never answers the gate itself.
- OSS used: none new.

## Evidence

Commands run in `D:\pm-fix-live-claude` (`CARGO_BUILD_JOBS=3`):

```
cargo test --lib                 -> test result: ok. 1518 passed; 0 failed; 2 ignored
cargo test --bins                -> test result: ok. 0 passed; 0 failed
npx tsc --noEmit -p packages/app -> exit 0
npm test --workspace=@puppet-master/app -> Test Files 111 passed, Tests 969 passed
```

New tests (real v2.1.299 screen text as fixtures under `tui/claude/fixtures/`):

- `tui::claude::prompt::tests` (12): multi-cd Bash dialog (command, reason, choices, menu), 4-option dialog, edit and
  create dialogs, trust dialog (v2.1.299 and the older v2.1.287 capture), arrow-key answers per row, the
  allow_all opt-in, a re-pointed highlight, another wording of the same structure, the old `Allow once` wording,
  negatives (model reply with a list, non yes/no menu, stale dialog, blank/idle screens), status reads
  `waiting_input`, CRLF checkouts.
- `pane_prompt::claude_tests` (6): `detect_in_screen("claude", fixture)` -> prompt id stable across scrollback and
  `allow_broad`, command, risk (`rm -rf /` high, `npm test` not), UI shape and `required_action`, edit risk by
  location, trust gate + labels, `dispatch_blocker` refuses every dialog and allows the ready screen.
- `bridge::startup_gate_tests` (4), on a stub pane fed the real screens and running the same
  `wait_for_startup_permission_resolution` the dispatch thread runs: the trust screen parks the run for every
  pane status; the wait **holds** while the dialog shows, survives a redraw, and **releases after the accept**; a
  declined dialog (pane exits) fails with `WORKSPACE_TRUST_DECLINED`, not recoverable; a pane that dies with no gate
  keeps `PANE_READINESS_FAILED`.
- `agent_runs::headless::fallback::tests::only_pre_turn_failures_are_retried_headless` now also asserts
  `WORKSPACE_TRUST_DECLINED` and `PANE_BLOCKED_BY_PROMPT` never fall back to headless.

**Fails before, passes after.** With `tui/claude/mod.rs` reverted to 93c8f2f (everything else kept) the new tests
give `test result: FAILED. 1 passed; 9 failed` (the one pass is the "pane dies without a gate" control):
`claudes_bash_dialog_becomes_a_card...`, `claudes_edit_dialog...`, `claudes_trust_dialog...`,
`a_task_is_never_typed_over_claudes_dialogs`, `claudes_answers_move_the_pointer...`,
`claudes_dangerous_command...`, and the three `startup_gate_tests` (`the_trust_dialog_parks_the_run...`,
`the_run_holds_while_the_dialog_shows...`, `declining_the_dialog_fails_the_run...`). Restored: all pass.

**Live checks against the running integration app** (no restart, bridge GET/POST only; spawned panes closed,
`D:\pm-demo-untrusted-*` deleted, `D:\pm-demo` left as found):

- Captured the fixtures from real panes (`/panes/<id>/buffer`, the same `contents()` the detector reads).
- Reproduced bug 2 on the unfixed app: `POST /agents/run` visible in a fresh folder -> pane shows the trust dialog,
  the run falls back to headless within seconds.
- Verified the answer keys on a real pane: on the create-file dialog, `ESC[B ESC[B` then Enter selected `3. No`
  ("User rejected write to notes.tmp", no file created). On the trust dialog of a fresh folder, `ESC[B` + Enter
  selected `Yes, I trust this folder` and Claude drew its normal screen (so `READY` in the gate tests is a real
  capture).
- Claude's `auto mode` footer suppressed the Bash dialog until the pane was switched to manual mode with Shift+Tab
  (`ESC[Z`): the Bash dialog only appears in non-auto mode. The captured pane had been switched this way.

## Live repro for the coordinator (after integration)

`node scripts/evidence/live-claude-dialogs.mjs D:/pm-demo` against the rebuilt app (spawns real Claude panes):

1. Trust, then run: a visible run in a fresh temp folder parks as `waiting_input` with a `workspace_trust` card;
   `answer_prompt choice=trust` -> the task runs in the visible pane (result PONG, no `fallback`).
2. Deny: a second fresh folder, answered `deny` -> `failed`, `error.code = WORKSPACE_TRUST_DECLINED`, no fallback.
3. Bash card: the run exposes a `command_approval` card (command, risk, allow_once / deny); `answer_prompt deny`
   clears the dialog. Note Claude must be in manual (non-auto) permission mode for the Bash dialog to appear.

By hand in the UI: Inbox shows the trust card ("Claude Code asks to trust <folder>", buttons Trust this folder /
Don't trust) for a visible run in an untrusted folder; and Allow / Deny on a Bash approval, with the phone push
"Allow" appearing for a low-risk command (`push/triggers.rs` already requires a command, low risk, allow_once+deny).

## Overlap with other branches (file level)

`pane_prompt/mod.rs` (shared by anything touching prompts/approvals), `prompt_risk/mod.rs` (one new field + kind),
`bridge/approvals.rs`, `bridge/mod.rs` (one `mod` line), `tui/claude/*`, `tui/README.md`,
`agent_runs/headless/fallback.rs` (test only). `tui/codex` is untouched.

## Limits / follow-ups

- **Codex** has no trust gate card yet: its profile must return a `workspace_trust` prompt and read the dialog as
  `waiting_input` (the Codex lead). `STARTUP_GATE_KINDS` is the only generic list to extend for a new gate kind.
- Not handled (still "Open terminal"): Claude's plan-mode approval (`Would you like to proceed?`, rows that start
  `Yes, and bypass permissions`) and `AskUserQuestion` menus. They are deliberately not mapped to allow/deny.
- `allow_all` for the 3-row Bash dialog is the `switch to auto mode` row (that is the only widening row there):
  broad, hidden behind the existing opt-in, and labelled "Allow all" by the generic label code. Worth a clearer
  label per prompt via the new `labels` map if product wants it.
- The `cwd` of a command like `cd X 2>/dev/null || cd "Y"; ...` is not guessed (`leading_cd_dir` only reads a plain
  leading `cd X &&`), so risk uses the pane workspace.
- Claude's dialog wording drifts between versions; the structure-based detector tolerates reordered/extra rows but
  an entirely new layout needs a new fixture and a look at `prompt.rs`.
- The live repro script is untested against the rebuilt app (the running app cannot be restarted by this branch).
