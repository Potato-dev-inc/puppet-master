# Keep going: move a task when an agent runs out of usage

Code: `packages/app/src-tauri/src/handoff/keep_going/` (the move itself), `commands/keep_going.rs` (desktop commands),
and the UI in `packages/app/src/components/cockpit/` (`LimitCard`, `ContinueOnSheet`, `BriefSheet`),
`components/chat/KeepGoingBanner.tsx`, `components/settings/sections/KeepGoingRows.tsx`, `lib/keep-going.ts`.
Detection of the limit is in `docs/features/handoff-detect.md`.

Status: built on this branch. Not yet run live on a real usage limit (see "Not done yet").

## What and why

When an agent such as Claude Code runs out of usage, it stops. Keep going moves the task to another agent with one
tap. The new agent works in the same folder and gets a short brief of what was done.

Puppet Master never moves a task on its own. Every move needs a tap from you.

## What you see

1. The limit card in the Inbox says "<Agent> ran out of usage." When another agent can take over, the first button is
   **Continue on <agent>**.
2. **Other agent…** opens a list. Each row says **Ready**, or why it is not. Pick one, then **Continue on <agent>**.
   The button only shows for a pane that has an agent run. A pane without one still gets the card's Continue choice.
3. After the tap, the old pane shows the status **Handed to <agent>**. The new pane opens under it in the sidebar.
   A banner above each chat says what happened:
   - old pane: `Continued on <agent> · 17:02 · Open`
   - new pane: `Continued from <agent> · 14 turns · Show brief`
   - failed start: `<agent> couldn't start · Try again from the card`
4. **Show brief** opens the brief the new agent got. **Copy** copies it. This works on the desktop only.
5. If the setting is on, and the first agent's reset time is known, a card asks **Move it back to <agent>?** later.
   **Stay on <agent>** keeps the task where it is.

## How a move works

The order matters. One folder must never have two writers.

1. **Checks.** The pane must still be at its limit. The target must be ready. No other move may be running for this pane.
2. **Brief.** Puppet Master builds the brief and saves it (see below).
3. **Old run ends first.** Its run is cancelled with error `HANDED_OFF`. So the old run cannot fail later with "agent exited".
4. **Old agent stops.** Puppet Master stops the agent process in that pane by its own process ID, with its child
   processes. The pane, its scrollback and its shell stay open. The agent must be gone within 5 seconds. If it is not,
   nothing new starts, and a card says the old agent did not stop. Puppet Master never types into the old pane.
5. **New agent starts.** It is a normal visible run in the same folder, or in the same separate copy if the old run had one.
   It does not trust the folder for you, it gets no shell, and it gets no extra allowed commands. If it shows a trust,
   permission or sign-in screen, the run waits and the usual card appears. Puppet Master never answers those screens.
6. **Record.** The move is saved. The "Your plans" limit log gets its end time. The audit log gets one entry
   (`keep_going.handed_over`) with the record, pane and agent ids, the hop number and the brief size. No brief text.

If step 5 fails, a card offers **Try <agent> again**. The old agent stays stopped in that case.

## Who can be offered

- Not the same agent, with any login.
- Not the same company. Each agent has a billing company (`limits.vendor`). An agent from the same company is listed
  but disabled: "Same company as <agent>".
- Not an agent that is out of usage right now. That is its own limit, or a limit it hit in an earlier move.
  The row says "Out of usage until <time>".
- Only agents that can run in this folder.
- Order: the order you set in Settings, then the other agents in their usual order.
- Companion panes and mission rounds are never offered Keep going.

## The brief

Puppet Master writes the brief. Every agent or tool string sits inside a fence, and the text says it is notes, not
instructions. The new agent is told to look at the changes with git and carry on.

Parts, in order:

- Line 1: a fixed sentence. It says the task is in the fenced notes, and that it was handed over at a usage limit
  (or handed back when the first agent's usage reset).
- Task: the first thing you asked, up to 1,200 characters.
- Turns: turn 1 and the last 7. Each is one line with a time and a changed-file count.
- Changed files: names and line counts from `git diff --numstat` against the base, plus new untracked files.
  No diff text. Up to 20 files.
- Open task-list items of the old agent: up to 12.
- Last message of the old agent: up to 1,500 characters.
- A question the old agent asked that nobody answered: up to 400 characters.

The whole brief is capped at 6 KiB. If it is too big, Puppet Master cuts in this order: files to 10, last message to
600 characters, turns to turn 1 plus the last 3, open items to 6, and then drops the question. The cuts are saved with
the record.

**Never in the brief:** tool output, raw diffs, terminal scrollback, environment variables.

**Redaction:** every string is cleaned first. Known secret shapes are masked, e-mail addresses are masked, and your home
folder is shown as `~`.

**Where it lives:** `<app data>/keep-going/briefs/<id>.md`, a private file. It is never in your project folder.
No bridge route, push message, team message or phone screen reads it.

## Coming back

- Setting: **Ask to move it back when the first agent resets**. It is on by default.
- It only works when the first agent's reset time is known. At reset plus 60 seconds, if the first agent is still
  limited, Puppet Master waits. If it is free, the card asks.
- If the new agent is busy, Puppet Master checks again every 10 minutes, up to 3 times. Then it drops the come-back.
- A come-back is a move like any other: a tap, a new brief, and the current agent stopped first.

## Limits on moves

- At most 3 moves in one chain (the hop cap). A fourth move is refused.
- Nothing moves on its own. Each move needs a tap.
- One move at a time per pane. A pane is moved once. A failed start can be tried again.

## Who can start a move

Only a person's answer on a limit card (on the desktop or on a paired phone), or the desktop's **Continue** button in the
**Other agent…** list. Not an agent, not a companion, not an MCP tool, not a script on loopback.
The phone shows the card's choices only. It has no agent list.

## Settings

Settings > Agents > **Keep going**:

- **Offer to continue on, in order.** Up to 8 agents. Add, remove, move up or down.
- **Ask to move it back when the first agent resets.** On by default.
- Footer: "Puppet Master asks every time. It never moves a task to another login of the same company."

## Error codes

| Code | What it means |
|---|---|
| `KEEP_GOING_NO_TARGET` | That agent cannot take over now. Nothing moved. |
| `KEEP_GOING_SAME_VENDOR` | That agent is from the same company. Nothing moved. |
| `KEEP_GOING_TARGET_LIMITED` | That agent is out of usage too. Nothing moved. |
| `KEEP_GOING_HOP_CAP` | The task already moved 3 times. |
| `KEEP_GOING_ALREADY` | This pane was already moved. |
| `KEEP_GOING_NOT_AT_LIMIT` | The agent is working again. Nothing moved. |
| `KEEP_GOING_NOT_HERE` | This pane cannot move (gone, a companion, or a mission round). |
| `KEEP_GOING_BUSY` | A move for this pane is already running. |
| `OLD_AGENT_DID_NOT_STOP` | The old agent did not exit in 5 seconds. Nothing new started. |
| `KEEP_GOING_START_FAILED` | The new agent could not start. A card offers a retry. |
| `HANDED_OFF` | On the old run. Follow-ups to the old task are refused with this. |

## Files

- `<app data>/keep-going/records.json`: the last 200 moves (ids, agent ids, labels, folder, state, reset time, redacted goal).
  It holds no brief text.
- `<app data>/keep-going/settings.json`: the order and the move-back switch.
- `<app data>/keep-going/briefs/<id>.md`: the brief.

File names come only from a checked record id (`kg-` plus 12 hex characters).

## Tests

- Rust, no app needed: `cargo test --lib handoff::keep_going` (model, store, brief, offers, chain, and the full move
  against fake I/O with a fake clock). `cargo test --lib handoff::card_tests` (the card choices and decisions).
- Rust, stop by process ID: `cargo test --lib agent_stop`.
- Rust, the old run: `cargo test --lib handed_off` (cancel with `HANDED_OFF`, context merge) and `cargo test --lib followup`.
- Rust, desktop commands: `cargo test --lib commands::keep_going`.
- UI: `cd packages/app; npx vitest run src/lib/keep-going src/components/cockpit/LimitCard src/components/cockpit/ContinueOnSheet src/components/cockpit/BriefSheet src/components/chat/KeepGoingBanner src/components/chat/SessionChatPanel.keepgoing src/components/settings/sections/KeepGoingRows`.

A test scans the module for write paths into a pane (`write_input`, `send_keys`, `press_key` and similar) and for
agent names in the generic code.

## Not done yet

- **No live run.** No real usage limit has been run through this flow. The live check plan is in the feature spec.
- **No end-to-end test with the fake agent.** The planned `tests/fake_tui_keep_going.rs` is not in the tree.
- **Limit texts are still synthetic.** The rules are checked against made-up screens, not captured live screens
  (see `handoff-detect.md`).
- **The brief's line 1 does not hold the task.** The task is in the fenced notes instead.
- **The move-back switch in the sheet is read only.** Change it in Settings > Agents > Keep going.
- **No phone agent list.** A phone can answer the card's choices. It cannot pick another agent.
- **Not in `FEATURES.md` or `TEST-PLAN.md` yet.**

Later steps (not built): automatic moves as an opt-in per run, an MCP `continue_from`, following a waiting run to its
new agent, a phone agent list, a push when an agent takes over, and the brief through the hook lane.
