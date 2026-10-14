# CLI-update safety: tested versions, strict mode, the zero-token canary (P0-10)

Branch `core/canary-strict`. Roadmap K9 + K10 + K11.

## What and why
Every CLI update can change the screens Puppet Master reads. Codex 0.159.3's update menu broke runs
(a task typed over the menu, Enter chose "Update now"). Personas A, B and C all lose a run silently when this
happens; the fix is to notice a version change, check it for free, and stop guessing until it is known good.

Agent-agnostic: per-agent knowledge is `TuiProfile::tested_versions()` (or `[profile] tested` in a TOML
definition). "Ready" and "a gate" are the profile's own `standby_screen` / `composer_state` / `startup_gate`.
Nothing in `src/canary/` names an agent.

## Design (all under `packages/app/src-tauri/src/canary/`)
| File | Job |
|---|---|
| `version.rs` | parse `--version` output, judge it against the range (`semver`, MIT/Apache) |
| `state.rs` | pure rules: strict reason, when to check, notice text, plain copy, the UI view |
| `strict.rs` | the strict set and `strict::startup_gate`, the wrapper both live dismissal paths call |
| `run.rs` | the check: `drive()` over a `CanaryIo` trait, virtual-time testable, sends only profile gate keys |
| `engine.rs` | records, persistence (`<app data>/canary.json`), version trigger, listeners |
| `live.rs` | real agents, real `--version`, hidden PTY (`pty/registry/probe.rs`), Tauri events |
| `unknown.rs`, `captures.rs` | unknown-screen tracker, redacted capture folder |
| `commands/canary.rs` | `canary_status`, `canary_check`, `canary_set_auto`, `canary_dismiss_notice`, `canary_open_captures` |

- **Ranges:** Codex `>=0.159, <0.170`, Claude Code `>=2.1.299, <2.2` (the versions the fixtures were captured on).
  Agents with no range are never strict.
- **Strict mode** (outside the range, or the last check on this version found `Unrecognised` screens):
  `strict::startup_gate` strips `dismiss` from a recognised gate, so `dismiss_startup_gates` and `close_startup_gates`
  park the run on it instead of pressing keys; the pane header shows "Untested version"; the standby pool
  (`set_strict_agents`) shuts the agent's warm workers down, warms nothing, and a claim misses. A run of the agent does
  not lift it. Strict mode never blocks the person.
- **Canary:** trigger = version differs from the last probe (probe at start +20 s, then every 15 min), or first
  sighting outside the range, or "Check this agent". The CLI starts in `<app data>/canary/scratch/<agent>` on a hidden
  ConPTY, `drive()` waits for a settled screen, closes gates only with the profile's dismiss keys, never answers
  trust/login, and needs the ready screen to hold still 3 s (a menu can draw late). It writes nothing else and records
  `keys_sent` and `prompts_sent = 0`. Outcomes: `Ready`, `AsksYou` (recognised user-only gate), `Unrecognised`
  (only this one means "changed screens" and raises strict + the Inbox card), `NeverDrew`, `Exited`, `StartFailed`,
  `Inconclusive` (profile has no readable ready screen).
- **Unknown screens:** called from the pane-activity ticker (one line each in `observe`/`tick`). A still, unreadable
  screen for 6 s is saved redacted (`agent_definitions_recorder::redact`) to `<app data>/captures/<agent>/<version>/`
  (cap 25 per version, deduped). Settings row "Screens I didn't recognise" shows the count and opens the folder.
- **UI:** `UntestedChip` (pane header), `CanaryNotices` (Inbox, not counted as "needs you"), `AgentVersions`
  (Settings > Agents > Updates). Copy follows UX-PHASE1 section 0.

## Evidence
See `D:\pm-night\p0\canary\REPORT.md` (commands, counts, live transcripts, screenshots).

## Overlaps
`agent_runs/delivery.rs` (1 line, `PaneIo::startup_gate`), `bridge/startup_gate.rs` (1 line), `pane_activity.rs`
(2 lines), `standby/pool.rs` (strict set), `lib.rs`, `commands/mod.rs`, `Cargo.toml` (`semver`), Inbox.tsx,
PaneView.tsx, AgentsSection.tsx, PreloadSettings.tsx (one text branch).

## Limits / follow-ups
- Claude Code asks about folder trust in an untrusted scratch folder, so the canary reports `AsksYou` there; it reaches
  `Ready` only in a folder the person already trusted (live-proven in `D:\pm-demo`). Founder decision: offer
  "open the check folder once to trust it".
- No composer round-trip (typing text without Enter) in the check: kept out so nothing is ever typed.
- The corpus replay and the opt-in PONG turn from IDEAS-R3 section 1.7 are not built.
- "One click makes a fixture + draft rule" for captures is not built; the folder is openable.
- The check runs in the app's own environment: Claude Code printed "Transcript saving is off - inherited
  CLAUDE_CODE_CHILD_SESSION marker" because the test shell was inside a Claude session; irrelevant in the app.
