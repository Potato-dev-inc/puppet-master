# Settings

**Status:** Works today. Autosave, search and the inline view were seen live. Some rows were tested only by automated tests.

## What it is

Settings is where you change how Puppet Master behaves. In Orchestration mode it opens **inside the cockpit**, next to the sidebar. In Individual mode it opens in its own window. It has six sections, a search box, and a "Labs" area for early features. Every change saves by itself. There is no Save button.

## Why it matters

- Developers get every knob and a search box.
- People who build with AI get sensible defaults and plain words.
- People who don't code never have to think about "saving", and dangerous actions sit at the bottom behind a confirm sheet.

## How to use it

1. Open Settings: click **Settings** at the bottom of the sidebar, or press Ctrl+K and choose **Open settings**. Press **Back** (or Esc) to return to the cockpit.
2. Pick a section on the left:
   - **General**: Project, Appearance (Theme, Animated tray icon, Welcome), Safety, Budgets, Undo and memory, When you are away, Labs, Storage, About, Uninstall.
   - **Agents**: Logins (Accounts), Keep agents ready, Agent versions, Agent definitions (your own agents).
   - **Orchestrator & Models**: which AI coordinates your other agents, and which models it may use.
   - **Keys**: API keys, OpenCode keys, and "Remove all keys".
   - **Bridge & MCP**: "Add Puppet Master to your agents", Toolbelt, bridge status, install, and set-up-by-hand snippets.
   - **Phone**: Pair a phone, Paired phones, phone options, and the Public address (your tunnel).
3. **Search**: type in the Search box. Every word you type must appear in a row's name, its help text or its hidden words. Results are grouped by section.
4. **Safety** (General): "Ask me before an agent controls a pane it didn't start", "Let agents merge separate copies", "Ask before closing a working agent", "Warn when an agent seems stuck", and "Recent approvals and control changes" (a list nobody can edit).
5. **Labs** (General › Labs): a list of early features that are off until you turn them on. Press one to jump to its switch. Today: Team, Ask bar, Simple mode, Live updates, Key switching (OpenCode) and Missions (Preview).
6. **Deep links**: other places open the exact row for you. For example Ctrl+K › "Budgets…" opens General › Budgets. The page scrolls to the row and marks it.
7. **Coming later** (bottom of the list) is a read-only list of ideas that are not built.
8. Rows that cannot be undone sit last in their section and ask first: Uninstall Puppet Master, Remove all keys, Uninstall MCP, Revoke all devices.

**When a change saves.** A switch or a menu saves at once. Text saves after you stop typing for a moment, or when you leave the field. A number saves when you leave the field and is kept inside its limits. A secret (such as a key) saves when you leave the field and is then cleared from the screen. A small "Saved" tick shows. The Public address has its own **Save and restart** button, because saving restarts a network tunnel.

## How it works (simple)

A control sends only the keys it changed. The Rust part of the app merges them into the settings file (a "JSON merge patch") and writes the file safely, with a temp file first. So an old, stale window can not wipe newer settings. Every open window reloads when the settings change, when a window gets focus, or when it is shown again.

The mode (Orchestration or Individual) is saved by its own command and a settings patch can not change it. API keys go to the Windows Credential Manager when it is available, not to the settings file.

## Limits and what is not done yet

- **Two agent-control defaults are not decided.** Today "Ask me before an agent controls a pane it didn't start" is off and "Let agents merge separate copies" is on. The recommendation is the opposite (ask on, merge off). It is a founder decision.
- The audit asked for a new top-level layout (General, Agents, Safety, Phone, Team, Labs, Advanced). In 0.2.1 there are still six sections. Safety and Labs are groups inside General.
- The "What's new" row in About is still called "What's new in 0.2.0" and does not list the 0.2.1 changes.
- Keys in the Credential Manager were tested only by automated tests. Set a key once and look under `com.puppetmaster.app` in Windows Credential Manager to check. The settings file should hold no key.
- Search lands on a whole block for big blocks (Toolbelt, Accounts), not on the single control inside.
- Per-row error text only: there is no global error toast.

## For developers

- UI: `packages/app/src/components/SettingsPanel.tsx`, `components/cockpit/SettingsView.tsx`, `components/settings/` (`catalog.ts`, `arrange.tsx` for the layout and Labs, `sections/*Section.tsx`, `controls.tsx`).
- A row is data: `{ id, label, hint, keywords, group, labs?, danger?, render }`. A new row that is not named in `arrange.tsx` lands after About, and a test catches that.
- Deep links: `lib/settings-target.ts` (a section id, a row id, `labs`, or an old tab id; `?tab=` and `?row=`).
- Rust: `src-tauri/src/settings_patch.rs` (command `settings_patch`), `settings_store.rs`, `app_windows.rs`; events `settings://changed` and `settings://show`.
- Keys used by 0.2.1 features: `theme`, `simple_mode`, `away_minutes`, `checkpoints_enabled`, `checkpoints_keep`, `budget_default_turns`, `budget_default_minutes`, `missions_enabled`, `onboarding_*`.

## Related

- [The cockpit](01-cockpit.md)
- [Command palette and ask bar](06-command-palette-and-ask-bar.md)
- [Simple mode](07-simple-mode.md)
- [Undo and checkpoints](05-undo-checkpoints.md)
- [Budgets](24-budgets.md)
- [Security, bridge and devices](29-security-bridge-and-devices.md)
