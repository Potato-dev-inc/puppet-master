# Loop guard and Stop all (panic stop)

**Status:** Works today. Both parts are tested only by automated tests. No real loop was made live, and the thresholds have not been tuned on real long sessions.

## What it is
Two safety nets. The **loop guard** watches your agents and warns you when one seems stuck. It shows a card such as "Claude Code seems stuck: the same error 4 times." **Stop all** (the panic stop) is one action that interrupts every agent that is working. A sheet then says what was stopped, for example "Stopped 2 agents".

## Why it matters
An agent can repeat the same failing step for an hour and use up your plan while you are away. The guard tells you early. And when something goes wrong, you do not want to find and stop each agent one by one. One action is a big comfort.

## How to use it
**Loop guard** (on by default):
1. When an agent seems stuck, a card appears under the pane header and in the **Inbox**. It shows up to three lines as proof. On your phone, the agent shows "seems stuck" in the Agents list and counts in "Needs you".
2. Choose one of the buttons:
   - **Give it a hint**: opens that pane so you can type to the agent.
   - **Interrupt**: sends Ctrl+C to the agent.
   - **Ask another agent**: opens the **+** menu so you can start a helper.
   - **Dismiss**: hides this warning. A later, different problem warns again.
3. To change it, open **Settings**, then **General**. You will find:
   - **Warn when an agent seems stuck** (on or off).
   - **Stuck warning sensitivity**: Early (3 repeats), Balanced (4 repeats, the default), Careful (6) or Very careful (8).
   - **Quiet for too long**: 10, 20 (default), 30 or 60 minutes.

**Stop all agents** (four places):
1. Press **Ctrl+K** and type "Stop all agents".
2. Or use the **tray** menu of Windows: **Stop all agents**.
3. Or on your phone: open the menu, tap **Stop all agents**, and confirm.
4. Or use the global shortcut **Ctrl+Alt+Shift+P**. It is off by default. Turn it on in Settings › General, with **Global shortcut: stop all agents**.

After you use it, a sheet lists each stopped agent. If one could not be stopped, it is listed with the reason.

## How it works (simple)
- The guard looks at the text on each agent's screen and at its status. It does not read the agent's mind and uses no AI.
- It looks for four patterns:
  - **Same error again**: the same error line in each of the last 4 turns.
  - **Same steps again**: the same tool steps in the same order, with no progress.
  - **No progress**: many busy turns in ten minutes, and none shows progress words such as "passed" or "built".
  - **Quiet too long**: the agent has been working for 20 minutes with nothing new on screen.
- Only new lines count. An old error that stays on screen is not counted again.
- A warning shows once for each episode. It comes back only after the problem clears.
- **Stop all** sends the same Ctrl+C as Interrupt, only to panes whose status is "running". **Plain shells are skipped on purpose.** A dev server in a shell is not an agent that burns credits, and you would lose it.
- The route for phones needs a paired phone with the "Start tasks" ability. An agent cannot call it, so one agent cannot stop the others.

## Limits and what is not done yet
- The thresholds are careful guesses. A real, long silent build can trigger "quiet too long". The wording is neutral and Dismiss is one click.
- Agents that print very little may only trigger "same error" and "quiet too long".
- The guard samples about once a second. A turn shorter than that can be missed.
- The shortcut keys are fixed. You cannot change them yet.
- Stop all does not take a checkpoint first. It only interrupts. An interrupt asks the agent to stop. It does not undo anything. For undo, see History in the app.

## For developers
- Loop guard: `packages/app/src-tauri/src/loop_guard.rs` and `loop_guard/`. Settings file: `loop_guard.json` in the app data folder (`loop_settings_get`, `loop_settings_set`).
- Events: Tauri `loop://signal`, `loop://clear`; SSE `loop-signal`. Commands: `loop_status`, `loop_signals`, `loop_dismiss`.
- Agent teaching: `TuiProfile::is_error_line`, `is_tool_activity_line`; TOML key `[tui] error_regex`.
- Panic: `panic.rs` (`stop_all`, `stop_running`). Command: `stop_all_agents`. Tray item and global shortcut: `tray.rs`, `tauri-plugin-global-shortcut`.
- HTTP: `POST /panes/stop-all` (`bridge/routes/panes.rs`). Not an MCP tool.
- UI: `components/cockpit/StuckCard.tsx`, `settings/blocks/LoopGuardSettings.tsx`, `mobile/MobileSheet.tsx`.

## Related
- [Budgets](24-budgets.md)
- [Security, bridge and devices](29-security-bridge-and-devices.md)
- [Flight recorder and diagnostics](28-flight-recorder-and-diagnostics.md)
- [Missions](30-missions.md)
