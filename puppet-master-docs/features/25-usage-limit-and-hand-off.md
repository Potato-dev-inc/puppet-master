# Usage limit and hand-off

**Status:** Partly built. Detecting a usage limit works today (tested only by automated tests, with made-up screens). Handing the work to another agent is **not built**.

## What it is
Every agent plan has a usage limit. When an agent runs out, it prints a message such as "you can continue at 14:05". Puppet Master now reads this message. It stops the run in a safe "parked" state, and shows you one card in plain words. It types nothing into the agent. "Hand-off" is the plan for the next step: give the unfinished work to a different agent. That step is designed but does not exist yet.

## Why it matters
Without this, a pane that hit its limit just looks stuck. You may not know why, or when it can go on. With the card you see the reason and the reset time without reading the terminal. An agent that is waiting for you through MCP also wakes up and can read why it stopped.

## How to use it
There is nothing to turn on. Detection is automatic for Claude Code and Codex.
1. Work as usual. If an agent hits its limit, a card appears in the **Inbox**. It also shows on the Receipt and on your phone.
2. The card says, for example, "Claude Code ran out of usage. It can continue at 14:05."
3. Choose one button:
   - **Remind me at 14:05**: at that time you get a reminder card. If the agent already carried on, no reminder is shown. If no time is known, the button says "Remind me in 1 hour".
   - **Stop here**: the run ends as cancelled. The pane stays open. The receipt says "Stopped at a usage limit (resets 14:05)". For a pane that has no run, the button is **Dismiss**.
4. The card says that handing the work to another agent is coming in a later update. There is no hand-off button today.

**Related setting.** In **Settings**, then **Keys**, then **Key automation**, there is an OpenCode option to switch to another of your API keys when one hits its rate limit. It is **off by default**. Check your provider's terms before you turn it on.

## How it works (simple)
- Puppet Master reads the last rows of the agent screen and looks for the exact limit message of each agent. The rules are data files, not code.
- The message must hold still for 2 seconds before Puppet Master acts. When a run ends its turn (the pane goes idle), the screen is final, so it acts at once.
- It ignores matching words inside the agent's own answer or in your own prompt.
- It reads the reset time in your computer's time zone. If the message uses another time zone, it does not guess.
- A parked run has the stage `usage_limit`. Its deadline is paused, so the run's timeout does not end it while it waits.
- It **never types** into the pane at a limit. It only reads Claude's own "continue automatically" wait. It does not answer the agent's limit menu.
- A short "busy, retrying" message from an agent is not a usage limit. It gets no card. The status line says the agent is retrying.
- The card stays until the limit goes away or you answer it.
- Reminders are saved in a file (`handoff-reminders.json`) and survive a restart. The card itself does not. It is shown again when the screen is seen again.

## Limits and what is not done yet
- This page says "usage limit" for your plan running out. A short busy message from the provider is a different thing and is only a status line.
- **Hand-off is not built.** The design says: start the next agent from a written brief, in the same copy, never two agents at the same time, at most three hand-offs per task, no automatic hand-back. Cursor is left out until its terms are read. Nothing of this runs today.
- Only Claude Code and Codex have detection rules. Other agents have none, unless a custom agent definition adds a `[limits]` table.
- **No rule was checked against a real limit screen.** All test screens are made up from the known message texts. The exact time format of Claude's message, and where it appears in a long turn, are not confirmed. The guard is careful on purpose. It can miss a limit. It should not invent one.
- The hand-off needs Missions v1. Missions today hands over to a backup agent only when an agent is stuck or exits, not at a usage limit. See [Missions](30-missions.md).

## For developers
- Module: `packages/app/src-tauri/src/handoff/` (`mod.rs`, `card.rs`). Rules: `tui/claude/limits.toml`, `tui/codex/limits.toml`, or an inline `[limits]` table in a TOML agent definition.
- Generic code calls `TuiProfile::limits()` and `detect_limit()`. It never matches agent names.
- Run state: `waiting_input`, stage `usage_limit`, `required_action.kind = "usage_limit"`. `answer_prompt` on it fails with `USAGE_LIMIT_NOT_ANSWERABLE`. "Stop here" ends with `USAGE_LIMIT_STOPPED`.
- Final-screen rule: `Engine::observe_final`, called from `Supervisor::observe_pane`.
- Validation codes: `AGENT_DEF_LIMITS_NO_VENDOR`, `AGENT_DEF_LIMITS_REGEX`.
- Reminders file: `handoff-reminders.json` in the app data folder.

## Related
- [Budgets](24-budgets.md)
- [Loop guard and panic stop](27-loop-guard-and-panic-stop.md)
- [Missions](30-missions.md)
