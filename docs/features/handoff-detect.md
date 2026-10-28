# Usage limits: detection and the limit card

Branch `feat/handoff-detect` (base `fix/live-r12` @ 2111483). Design: `D:\pm-night\DESIGN-RELAY.md` ("Hand-off at the
limit"). This branch built the detection step and the limit card. The hand-over that the card can start ("Continue on
<agent>") was built later; it is described in [`keep-going.md`](keep-going.md). The team relay is a different thing
(`team/transport/relay.rs`), so never call this feature "relay".

## What and why

When an agent's plan runs out, Puppet Master now notices, says so in plain words, and does nothing else. Personas:
A (developers) get the reset time without reading the pane; B (vibecoders) get "Claude Code ran out of usage. It can
continue at 14:05." instead of a pane that looks stuck; C never see the word "rate limit" (a transient retry is a
status line: "Claude Code is busy at its end - retrying").

What it does, per agent that has a `[limits]` table:

1. **Detects** a usage limit on the screen (Claude Code 2.1.300, Codex 0.162.x: the exact texts the design cites), with
   a reset time read in this computer's zone (a reset printed in another zone is not guessed).
2. **Waits for the screen to hold still for 2 s** before acting; fires once; stops counting when the agent carries on.
   **Except at a turn's end:** when a run's pane goes idle, its screen is final and still, so a usage limit on it is
   acted on at once (`Engine::observe_final`, called by `Supervisor::observe_pane` for an idle pane). The run is parked
   and its receipt carries `usage_limit` without waiting out the 2 s. Every other guard still applies (tail rows, not
   inside the agent's reply or the prompt echo, not while a turn runs).
3. **Parks the run** bound to the pane: `waiting_input`, stage `usage_limit`, `required_action.kind = "usage_limit"`
   with `window`, `reset_at_ms`, `reset_text`, `self_resume_armed` and no options. An MCP caller waiting on the run
   wakes with `needs_input` and can read why. `answer_prompt` on it is refused (`USAGE_LIMIT_NOT_ANSWERABLE`).
4. **Raises one Inbox card** (`handoff/card.rs`, `limit_card`): "Claude Code ran out of usage. It can continue at 14:05."
   The choices, in order:
   - **Continue on Codex** (first and primary). It is shown only when another agent can take over now. Picking it
     starts the hand-over: the old pane's agent is stopped and the task moves to the new agent. See
     [`keep-going.md`](keep-going.md). When no agent can take over, there is no Continue choice. The card says why
     ("No other agent can take over now: ...") and points to Settings > Agents > Keep going.
   - **Remind me at 14:05** (or "Remind me in 1 hour" when the reset time is unknown).
   - **Stop here** (for a pane with a run), or **Dismiss** (for a pane with no run).
   The context line says what Continue does: the other agent gets a short brief and works in the same folder, and this
   pane's agent is stopped first. When Claude is set to continue on its own, the card says that Continue stops it.
   On the desktop, `LimitCard.tsx` adds **Other agent…** for a pane that has a run. It opens the agent list
   (`ContinueOnSheet.tsx`). On a companion's pane it shows **Wake {name} on…** instead. A paired phone shows the same
   choices as ordinary buttons. It has no agent list.
   - Remind me: a reminder card at the reset ("usage limit should have reset by now"), skipped if the agent
     already carried on. Reminders survive a restart (`handoff-reminders.json` in the app-data folder).
   - Stop here: the run ends as cancelled with error `USAGE_LIMIT_STOPPED`; its pane is **kept**; the receipt says
     "Stopped at a usage limit (resets 14:05)".
5. **Types nothing, ever.** At a usage limit (or the vendor's own limit menu) readiness is a gate with no keys, so no
   task is typed (`TypingPermit` and `GatePermit` are never issued). Claude's own "continue automatically at usage
   limit" wait is only read (`self_resume_armed`, shown on the card); `/rate-limit-options` is never answered.
6. **The run's deadline stands still while it is parked at a limit.** The deadline stops the worker, which presses
   keys in the pane (Esc is how Claude cancels its own wait), so a run waiting out its limit is never ended by its
   `timeout_ms`; the clock runs again once the agent does.
7. **Transient rate limits** raise no card and leave the run alone; the pane's status line says the agent is retrying.

## Design

- Rules are data: `tui/claude/limits.toml`, `tui/codex/limits.toml` (embedded with `include_str!`), or an inline
  `[limits]` table in a TOML agent definition (validated with `AGENT_DEF_LIMITS_NO_VENDOR`, `AGENT_DEF_LIMITS_REGEX`).
  The schema is closed: it has no key that presses anything. `cancel_self_resume` / `reset_gate` (the design's keys
  for the later hand-off step, validated with `AGENT_DEF_LIMITS_GATE_KEYS`) are deliberately NOT in the schema yet.
- Generic code never names an agent: it calls `TuiProfile::limits()` / `detect_limit()` (default: the generic engine).
- False-match guards: tail rows only, not while a turn runs, never inside the agent's own reply or the person's prompt
  echo (only the CLI's own notice row counts), and the match must hold still.
- Files: see `docs/REPO_MAP.md` section 3.6a.
- Reset formats are regexes in the table (`hour`, `ampm`, `minute`, `mon`, `day`, `year` groups) instead of the
  design's chrono strftime strings: no strftime parser, the same data for both vendors. `chrono` and
  `iana-time-zone` (both MIT/Apache-2.0, already in `Cargo.lock`) are used only for the local zone and DST.

OpenCode key switching (same branch, founder decision 2026-10-15): "Switch to another of your keys when one hits its
rate limit (check your provider's terms)" is now an **opt-in** switch, off by default. Store version 3: an old file that
holds the old default is read as the default and turned off; a stored `notify_only` stays; from version 3 on an
opt-in is kept.

## Evidence

See `D:\pm-night\p0\handoff\REPORT.md` (commands, counts, fail-before logs, screenshots).

## Limits and follow-ups

- **No rule has been checked against a live screen.** Every fixture is SYNTHETIC (built from the quoted texts) and
  says so in its `#! source:` header. Live captures are listed in `docs/specs/handoff/live-captures.md`.
- Claude's exact time formatting (`5pm` vs `5 PM`, zone suffix) and where a mid-turn limit notice sits in its
  transcript are unconfirmed (probe P-L1, capture C1). The guard is conservative: it can miss a limit, it should not
  invent one.
- Reminders and the 2 s hold live in memory (reminders also in a file); the card is a question, so it does not survive
  an app restart. A run parked at a limit is parked again when the screen is seen again.
- Built later, see [`keep-going.md`](keep-going.md): the brief, the hand-over, `HANDED_OFF`, the come-back card, and the
  lineage shown in the chat banner. The design's "Backup agents" setting became Settings > Agents > Keep going.
- Not built yet: Puppet Master resuming the first agent at its reset by itself (the come-back is a card the person
  answers), the scheduler, lineage on the receipt, and a phone agent list.
