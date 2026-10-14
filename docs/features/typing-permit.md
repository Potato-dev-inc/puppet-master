# Typing permit and host-shell sentinel (K1 + K2, roadmap P0-3 + P0-4)

Branch `core/typing-permit`, base `integration/r8` (feeb29d).

## What and why

Live bugs VERIFY #4 (a Codex update menu typed over, the run silently fell back to headless) and
VERIFY #2b (a declined Claude trust dialog, the task typed into PowerShell) had one root: Puppet
Master typed whenever "no gate was found", which is not the same as "the agent takes a task now".
Round 7/8 patched the two cases. This branch makes it one invariant:

> Puppet Master writes a run's input into a pane only while holding a permit. A typing permit exists
> only for a `Ready` verdict computed from a screen that is still the latest (nothing printed since)
> with the agent process not gone. A gate's keys need that gate's permit. Anything else keeps
> looking, then parks the run with the screen and a plain sentence. It never types and never falls
> back to a background run on its own.

Personas: A (developers) get runs that never type into the wrong thing; B and C get a plain
explanation ("bring the agent to its input box, the run continues by itself", "the agent's command
`claude` was not found; install it with ...") instead of a run that "completed" with a PowerShell error.

Agent-agnostic: generic code (`agent_runs/readiness.rs`, `delivery.rs`, `bridge/dispatch.rs`) never
names an agent. What an input box looks like is per-profile: `TuiProfile::ready_recogniser`.

## Design

| Piece | Where |
|---|---|
| `Readiness` (Ready / Busy / Gate / HostShell / Unknown), `Verdict`, `assess`, `TypingPermit`, `GatePermit`, `wait_while_parked`, error builders | `agent_runs/readiness.rs` (+ `readiness_tests.rs`) |
| `WriteOrigin` (User, Task(permit), Gate(permit), Answer, Nudge, Api, Structured) on every `registry_write_input` | `pty/write_origin.rs`, `pty/registry/pane_io.rs::write_input`, every caller |
| Screen + stream position under one lock | `pty/registry/pane_io.rs::read_snapshot_full_at` |
| Agent process under the host shell | `pty/agent_proc.rs` (Toolhelp32 FFI, no new crate) |
| "Not recognized" detector | `tui/shell/prompt.rs::command_not_found` |
| Permit-driven delivery: a look right before every keystroke, gate permits, `AgentGone`, `NotReady` | `agent_runs/delivery.rs` (`await_ready`, `deliver_prompt_traced`, `type_unverified`, `observe_pane`) |
| Gate dismissal with gate permits | `bridge/startup_gate.rs` |
| Dispatch: outcome mapping, park on an unknown screen, resume | `bridge/dispatch.rs` (`deliver_task`, `park_on_unknown_screen`) |
| Recognisers | `tui/claude/screen.rs::input_box`, `tui/cursor/screen.rs::input_box`, `tui/codex/screen.rs::still_loading` + `composer_state` |
| Trait method | `tui/profile.rs::ready_recogniser` (default: `composer_state` when `reads_composer`, else `None` = legacy) |
| Ratchets | conformance rule `blank_screen_is_unknown` (`tui/conformance.rs`); `readiness_tests::ready_and_gate_are_never_the_same_screen` (every profile x every `tui/*/fixtures/*.txt`); `fallback.rs::readiness_refusals_never_fall_back_headless` |

### The verdict (`readiness::assess`), in order; each step only moves away from Ready

1. the host shell says the agent's command was not found (its prompt is the last line) = `HostShell(NotInstalled)`;
2. the agent process is known to have exited = `HostShell(AgentExited)` (the process vetoes the screen);
3. blank = `Unknown`;
4. a bare host-shell prompt where the agent was = `HostShell(AgentExited)`;
5. `startup_gate` = `Gate(gate)`; a detected dialog (`detect_prompt`) = `Gate(prompt)`;
6. the host shell has not started the agent yet = `Unknown`;
7. the profile's recogniser: `Empty` = `Ready`; text in the box = `Ready` only if the profile can clear it (else `Unknown`); `Working` = `Busy`; else `Unknown`;
8. no recogniser (legacy): busy marker says working = `Busy`, else `Ready` with basis `Inferred`
   (the run transcript gets a `READINESS_INFERRED` line).

### Freshness

`Observation.output_pos` is `stream_pos.processed` read under the emulator lock with the screen. The
write path compares it with `stream_pos.emitted` on the permit's first write: any byte printed in
between = `PERMIT_STALE`, and delivery looks again. This is what stops "the menu drew 120 ms after
the check" for good (test `a_menu_that_draws_between_the_look_and_the_keystroke_is_never_typed_into`).
Every write (also the Enter that rides on the text's permit) re-reads the agent process.

### Unknown parks

Delivery keeps looking (and closing dismissible gates) for about 10 s. Then dispatch marks the run
`waiting_input` with `required_action {kind: "screen_not_ready", detail, screen}`, releases the
pane-assignment lock, and keeps looking for 5 minutes: `Ready` (or a gate delivery can close) resumes
the dispatch; the agent gone = `AGENT_EXITED` / `AGENT_NOT_INSTALLED`; a declined dialog =
`WORKSPACE_TRUST_DECLINED`; cancelled = `OPERATION_CANCELLED`; time out = `SCREEN_NOT_READY` with the
screen. None of these is a pre-turn failure, so none falls back headless (ratchet test). A blank
pane, which used to end `PROMPT_NOT_SUBMITTED` and fall back, now parks.

### Host shell: process tracking, not dropping `-NoExit` (decision)

Two options from the roadmap: drop `-NoExit` for run-spawned panes, or track the agent's PID under
the host. Chosen: **track, keep `-NoExit` everywhere**. Reasons:
- User-opened panes keep their shell after the agent exits (useful, and the founder rule: no behaviour
  change without a yes). The spawn path does not know whether a pane is "run-spawned": a warm
  standby is spawned before any run claims it, and a run's pane becomes the user's pane for follow-ups.
- Without `-NoExit` the pane dies with the agent and takes the screen with it: the "not recognized"
  text (the not-installed explanation) and the declined-dialog screen that round 7/8 read are gone,
  and the pane-exit path (window close, cleanup) runs instead of a typed refusal.
- Tracking is read-only: no behaviour changes until a write is about to happen.
- Live evidence that the screen alone is not enough: Claude killed at its input box leaves its last
  frame on screen (`tui/claude/fixtures/killed_host_shell_2_1_299_live.txt`); the screen says Ready,
  the process says Exited, and 0 bytes are typed (`live_permit_claude_killed_before_delivery_types_zero_bytes_into_the_shell`).

How: the pane's root process is read from a Toolhelp32 snapshot (raw kernel32 FFI like
`spawn.rs::process_alive`, no new dependency). A shell root (`powershell.exe`, `pwsh.exe`,
`cmd.exe`, by image name, never an agent name) hosts the agent as its child: child seen = `Alive`, seen
then gone = `Exited`, never seen = `Starting`. Any other root is the agent itself.

### Profiles and readiness

| Profile | Readiness | Recogniser |
|---|---|---|
| `claude` | recognised (new) | `❯` row starting a row, rule above (own row or glued by a soft wrap), rule below, near the bottom; works without the banner (follow-ups). No clear key, so a draft is not typed into. |
| `cursor_agent` | recognised (new) | arrow row framed by `▄` / `▀` rows; placeholders `Plan, search, build anything` / `Add a follow-up`; Ctrl+U clears a draft. Cursor is not installed on this PC: fixtures only (`reply_*_screen.txt`, `workspace_trust_visible_resolved.txt`), no live check. |
| `codex` | recognised | `composer_state`, plus not while 0.162 is still `loading` (new, live capture) |
| `opencode` (TUI) | **legacy** (`Inferred`) | none: no captured screens to write one from, and it does not verify delivery |
| declarative (kilo, kiro, user TOML) | **legacy** | `[tui.ready]` describes the HOME screen for a standby; kilo's reply screen has no "Ask anything", so using it would park every follow-up. A future `[tui.input]` table could add one. |
| `opencode_native` | n/a | prompts go over its HTTP API (`WriteOrigin::Structured`, refused on the terminal path) |
| shells, `cursor_ide`, generic | n/a | not dispatchable |

## Compile-fail proof

`cargo test --doc readiness`: three `compile_fail,E0451` examples (struct literals of `TypingPermit`,
`GatePermit`, `Verdict` outside the module) and one example that compiles (the types are public, so
the failures are the private fields). Shown to bite: with `TypingPermit`'s fields made `pub`, the
first example fails with "Test compiled successfully, but it's marked `compile_fail`"
(`D:\pm-night\p0\typing-permit\doctest-forgeable.txt`). trybuild was not used: it would build the
whole Tauri crate again in its own target directory.

## Overlap with other branches (file level)

- **fix/live-r9** (supervisor watchdog, mid-run AGENT_EXITED): same code and shape. `AGENT_EXITED` stays
  `OperationError { code: "AGENT_EXITED", recoverable: false, message: <the dispatch_blocker sentence> }`;
  this branch adds `context {pane_id, typed_bytes: 0}`. For mid-run detection use
  `pty::agent_proc::state(pane_id, pane.info.pid)` (or `agent_runs::delivery::observe_pane`) and
  `agent_runs::readiness::host_shell_error(&HostShell::AgentExited, profile, pane_id)`.
  Files both may touch: `agent_runs/headless/supervise.rs` (here: two `WriteOrigin::Nudge` args only),
  `agent_runs/headless/pane_turn.rs` (here: `WriteOrigin::Answer`, two new `Delivery` variants).
- **flight-recorder**: `WriteOrigin::tag()` is the writer tag for recorded input.
- **Everyone calling `registry_write_input`**: it has an 8th argument now (`WriteOrigin`). Merge rule:
  person = `User`; a run's text = only through `delivery`; control keys = `Nudge`; answers = `Answer`.
- `DeliveryIo` / `GateIo` traits changed (write takes the origin / the gate permit; `composer_state`,
  `startup_gate`, `reads_composer` replaced by `profile()`; new `observe()`).

## Evidence

See `D:\pm-night\p0\typing-permit\REPORT.md` for the command outputs.

## Limits / follow-ups

- Mid-turn agent death is detected at the next write (every permit write re-reads the process) and by
  dispatch/redelivery; ending a running turn on agent death belongs to fix/live-r9's supervisor.
- Live: Claude 2.1.299 showed a follow-up's typed text about 2 s late once (just inside the 2 s
  visibility window). Pre-existing timing, now visible in the trace; a slower machine could retype.
- `PROMPT_NOT_SUBMITTED` (typed, never visible) still falls back headless as before; it is recorded on
  the receipt. It is no longer reachable from a blank or unknown screen.
- A PID reused by an unrelated process whose parent id happens to be the host shell's is not ruled out
  (no creation-time check).
- Cursor's recogniser has no live check (not installed here).
