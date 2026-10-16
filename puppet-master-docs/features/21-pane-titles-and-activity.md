# Pane titles, activity line and pane status

**Status:** Works today · Titles were checked live. The activity line, tray icon and taskbar badge are tested only by automated tests and still need a look on a real desktop. Rich activity phrases exist for Cursor Agent only.

## What it is
Three small signals that tell you what each agent is doing without opening it:
- **Title.** A short name made from the task, such as "Create a new file r10-note.txt". You can rename it.
- **Status.** One of four states: Working, Needs you, Idle, Error.
- **Activity line.** One sentence under the title, such as "Running npm test", "Editing src/app.ts" or "Waiting: allow `npm test`?".

The Windows tray icon and the taskbar badge show the same states for all agents together. PM makes these without any AI call.

## Why it matters
Names like `claude · 3f9a02bc` mean nothing when you run five agents. A short title and one line of activity let you scan the sidebar and find the one that needs you. The tray icon lets you check from another window.

## How to use it
1. Start an agent and send it a task. The sidebar row gets a title from the first task line.
2. Read the sidebar groups: **Needs you** (waiting for you, an error, or a pending question), **Working**, **Idle**. Moving dots show who is working.
3. Read the second line of each row for the activity. The project folder moves to the tooltip.
4. To rename a pane, double-click its title in the pane header, or open the **⋯** menu and choose **Rename**. **Enter** saves. **Esc** cancels. An empty name removes your name, and the automatic title returns.
5. Look at the tray icon: a plus with a bright centre means someone needs you. An X means an error. A moving sweep means agents are working. Four dim corners mean idle. The tooltip reads, for example, "Puppet Master — 2 need you · 3 working".
6. To stop the movement, open **Settings > General > Appearance** and turn off **Animated tray icon**. The icon then shows a still ring while agents work.
7. The taskbar button of the cockpit shows a number badge. It counts panes that need you or have an error.
8. Titles also show in the Inbox, Ctrl+K, pop-out window titles, the tray's Panes menu and the phone tabs.

## How it works (simple)
- **Titles.** PM watches the text you type, or the task another agent sends, into an agent pane. At the first submitted line it makes a title: it removes control codes and polite starts like "please" or "can you", keeps the first sentence, and cuts at 40 characters. It ignores empty lines, slash commands, `!commands`, one-word answers like "yes", and a single long word with digits (it looks like a secret). A plain shell never gets an automatic title, because lines typed there can be passwords or commands. You can still rename a shell.
- **Status.** The status comes from reading the screen. Each agent's profile says what its screen means. There are four values: running, waiting for input, idle, error. Your choice of group is built from them.
- **Activity phrase.** Waiting phrases include the command when the agent's prompt reader can find it. For "Running", PM takes the newest line that looks like a tool row and makes it plain: `Grepped "x" in .` becomes `Searching "x" in .`. Every phrase is one line, at most 72 characters. A phrase that does not fit the current status is hidden.
- **Cost.** PM does no work per byte. Once per second it reads the screen of panes that changed, and only sends an update when the phrase changed. A silent pane costs nothing.
- **Tray.** The icon is a 3 by 3 dot picture. The sweep is 8 frames, 4 per second, and stops when nothing is running. It turns black on a light taskbar. Warm standby panes are not counted.

## Limits and what is not done yet
- **Activity detail.** Claude Code, Codex and OpenCode only get Working, Waiting, Idle or Error (with the command in waiting phrases). Rich phrases need real screen captures to write patterns. Cursor Agent has them. A TOML agent can add `activity_regex`. The Kilo sample has none yet.
- The activity line shows in the desktop app. The phone gets the event but does not show it yet. Titles do show on the phone.
- Titles are kept in memory. They reset when the app restarts.
- A pane started with the prompt on the command line, not typed, gets no automatic title until you rename it. A first line that is a code or a one-word answer waits for the next real line.
- A short plain password typed into a hidden prompt of an agent could become a title. The secret check covers long tokens, not short words.
- Status is a reading of the screen, so it can be wrong after an agent update. See [Tested versions and canary](18-tested-versions-canary.md).
- Not checked on a real desktop: the tray animation and the taskbar badge.

## For developers
- `src-tauri/src/ambient/`: `glyph.rs`, `raster.rs`, `tray_icon.rs`, `title.rs`, `store.rs`, `prefs.rs` (`ambient.json`). Commands: `pane_set_title`, `ambient_get_prefs`, `ambient_set_animated_tray`.
- `src-tauri/src/pane_activity.rs` (tracker, 1 s ticker), `tui/activity.rs` (`default_activity`), `TuiProfile::activity`, `is_tool_activity_line`. TOML: `[tui] activity_regex`.
- Delivery: command `pane_activities`, Tauri event `pty://activity`, SSE event `pane-activity`. `PaneInfo.title` is in the shared schema.
- Status: `pty/status.rs` (`PaneStatus`), profile `classify_screen`. UI: `components/cockpit/model.ts` (`groupPanes`), `Sidebar.tsx`, `RenameTitle.tsx`, `lib/pane-activity.ts`.
- Developer docs: `docs/features/ambient-and-titles.md`, `agent-doctor-and-activity.md`.

## Related
- [Cockpit](01-cockpit.md)
- [Inbox: Needs you](02-inbox-needs-you.md)
- [Phone app](36-phone-app.md)
- [Set up agents and Doctor](13-set-up-agents-and-doctor.md)
- [Supported agents](12-supported-agents.md)
