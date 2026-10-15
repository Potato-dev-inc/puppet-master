# Hand-off: live captures still to take (TODO)

From `D:\pm-night\DESIGN-RELAY.md` section 7.1. **Every fixture shipped so far is synthetic**
(`tui/<agent>/fixtures/limits/<version>/*.txt`, header `#! source: synthetic ... NOT a live capture`). Only `live`
fixtures count toward "tested". When a capture lands: put it next to the synthetic one with `#! source: live (...)`,
add the expectation header, and replace or add the replay cast under `tests/corpus/<agent>/<version>/`
(`tests/corpus/claude/2.1.300/usage_limit_mid_turn.cast` is synthetic too).

How: the unknown-screen auto-capture (`canary/unknown.rs`, 6 s) and the flight recorder on a machine where a limit
happens naturally; `GET /panes/{id}/buffer`; or a probe (dev only, never shipped).

| # | Agent | Screen | Status |
|---|---|---|---|
| C1 | Claude 2.1.300 | `You've hit your session limit - resets ...` with its own wait armed (`continuing automatically at ... - esc to cancel`) | TODO |
| C2 | Claude | the same with its own wait off, plus the `/rate-limit-options` dialog | TODO |
| C3 | Claude | `Automatic continue cancelled ...` after Esc (read only: Puppet Master never presses it) | TODO |
| C4 | Claude | `Your usage limit has reset` / `Press enter to continue` | TODO |
| C5 | Claude | weekly limit; a model limit (Opus or Fable) | TODO |
| C6 | Claude | transient `Server is temporarily limiting requests ...` + `Retrying in ... attempt n/m` | TODO (probe P-L2) |
| C7 | Claude | warning `You've used NN% of your ...` | TODO |
| C8 | Claude | headless stream-json `rate_limit_event` `rejected` | TODO |
| C9 | Claude | Hook Lane `StopFailure` payload, `error = rate_limit` | TODO (with the Hook Lane fixtures) |
| C10 | Claude | each of the above at 52 and 200 columns | TODO |
| X1 | Codex 0.162.x | `■ You've hit your usage limit. ... try again at 9:45 PM.` (same day) | TODO (probe P-L3) |
| X2 | Codex | `... try again at Oct 16th, 2026 9:45 PM.` (other day) | TODO |
| X3 | Codex | `... Try again later.` (no reset) | TODO |
| X4 | Codex | per-model `... for {limit_name}. Switch to another model now, ...` | TODO |
| X5 | Codex | `Heads up, you have less than 25% ...` wrapped at 52 columns | TODO |
| X6 | Codex | `Reconnecting... 2/5`, `exceeded retry limit, last status: 429 ...` | TODO |
| X7 | Codex | the "Approaching rate limits" switch popup | TODO |
| X8 | Codex | `exec --json` error event for X1 | TODO |
| N1 | both | negative: the agent's reply quoting the limit sentence | TODO live (synthetic negative exists) |
| N2 | both | negative: a test file in the repo containing the sentence, shown in a diff | TODO live (synthetic negative exists) |

Open questions a capture answers: the exact time case and spacing Claude prints; whether a mid-turn limit notice
sits under the last `●`/`⏺` block or on its own (the detector accepts a `⎿` row under a block and refuses a plain
quoted row); whether Claude's busy marker (`esc to interrupt`) is up while its own wait counts down (the detector
refuses to read a limit while the turn is running).
