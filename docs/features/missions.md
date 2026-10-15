# Missions v0 (Preview)

A mission is a goal plus checks plus a budget. Puppet Master makes one separate copy of the project and asks one
agent at a time to work towards the goal. After every turn it runs the checks in that copy.

- If they are not green yet, the agent gets the failures and continues.
- If the agent makes no progress, the next backup agent takes over from a brief.
- If a turn makes things worse, the copy's files go back to the best turn so far.
- When the checks pass, the test-change guard compares the tests with how they were at the start. If an agent
  skipped, removed or loosened tests, the mission stops and asks the person.

Nothing merges, pushes or opens a pull request on its own. A finished mission is a separate copy waiting for review.

Design: `D:\pm-night\DESIGN-MISSIONS.md`. Founder decisions (2026-10-15):

- Missions ship as a Preview without waiting for the Hook Lane.
- The MCP shape is the `run_agent` arguments.
- Orchestrators can stop a mission but not pause it.
- Missions use visible panes only, and the budget and the tamper guard are required.
- Missions are off by default behind a Labs switch.

## Who it is for

- **Developers (A)** hand over a red test suite and come back to a green copy that is ready to review.
- **Vibecoders (B)** get "keep going until it works", with a budget and a guard against fake green.
- **Orchestrator agents** call `run_agent(until_checks_pass: true)` and receive a `mission-<id>` handle.

Missions are agent-agnostic: a round is an ordinary `run_agent` or `followup_task` turn on any agent that has a
visible pane. No code reads agent names.

## Where it lives

| Part | Files |
|---|---|
| Engine | `src-tauri/src/missions/`: `model`, `store` (mission.json, rounds.jsonl, baseline.json, app-wide index), `ledger` (MISSION.md), `detect`, `verdict` (checks + counts + test changes), `decide` (the pure decision table), `prompt` (fenced), `engine` (loop, `RoundIo`, registry), `io` (live), `cards` (attention), `view`, `routes` (bridge + MCP handles), `tests` |
| Seams | `run_agent` `join_worktree` (crate-internal) in `agent_runs/headless/entry.rs` and `runtime/request.rs`; a joined run never discards the copy (`pane_close.rs`); `checkpoints::mission_snapshot/restore/changes`; budgets: a stopped helper refunds its unused share; `context_pack::fence`; `agent_runs/checks/counts.rs` and `checks::run_command_capture` |
| MCP | `run_agent` gains `until_checks_pass` and `backup_agents` (`tool_registry/catalog/agents.rs`, catalog re-pinned to `68-312b457ce7361adc`); `mission-` handles on `/agents/wait`, `GET /agents/<h>`, `/agents/cancel`, `GET /agents/<h>/transcript`; `followup`/`send`/`answer` give `MISSION_NOT_STEERABLE` |
| Bridge / Tauri | `/missions`, `/missions/detect`, `/missions/<id>`, `POST /missions/start`, `/missions/<id>/{stop,pause,resume,decide}` (all person-only, an MCP session is refused); the `mission_*` commands in `commands/missions.rs` |
| UI | `components/cockpit/mission/` (StartMissionSheet, MissionProgress, MissionCards, MissionGlyph), `lib/missions.ts`, `styles/missions.css`; hooks in `Inbox.tsx`, `PaneView.tsx` (Mission tab), `Sidebar.tsx` + `NewAgentMenu.tsx` (Start a mission…), Settings › General › Labs |

## Tests

- **Unit, scripted fake agents:** 33 tests in `missions/tests.rs`. A `FakeIo` plays a script, and the checks, the
  checkpoints and the tamper engine run for real on a temporary git repo. The tests cover:
  - red to green, rollback, double rollback leading to a hand-off, no progress, a loop signal, exit then resume
    then hand-off;
  - two timeouts, stuck (the list wraps once), the budget pause, pause and resume, stop mid-turn;
  - checks already green, trust declined, a person editing between rounds;
  - tamper block with undo, accept, the nudge, "agents may change tests";
  - crash and resume: found by key, interrupted, paused after a restart, a damaged log, a corrupt record;
  - validation, the MCP status mapping, the grep test that missions never merge, the fences.
- **End to end, `pm-fake-tui`:** `tests/fake_tui_missions.rs`, included from `fake_tui_e2e.rs`. It runs real ConPTY
  panes, real `run_agent`/`followup_task`, real checks in the real copy. It covers going green over 3 turns, a
  regression and rollback, tamper then Undo then green, stuck, and an agent crash followed by a resume with a brief.
- **vitest:** `lib/missions.test.ts` and `components/cockpit/mission/Mission.test.tsx`.

## Limits (v0)

- There is no Sidebar mission row. A mission shows in the Inbox (cards, then "Missions", then "Ready for you") and in
  the Mission tab of its agent's pane.
- The phone has no mission view yet. The attention cards (tamper, stuck, setup, review) reach it through
  `/attention`, but their answers go through `/missions/<id>/decide`.
- Checks run once per turn, by the engine, in the copy. Round operations carry no checks, so a turn's own receipt does
  not list them; the mission timeline does.
- The person can stop a mission, but pausing is desktop-only. A hand-off at the usage limit (T1) and
  `waiting{until}` arrive in v1.
- `main moved on` warnings and the "Show diff" filter on protected files are not built.
