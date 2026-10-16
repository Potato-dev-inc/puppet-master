# Tested versions and canary check

**Status:** Works today · No false "Untested version" chip on the current versions (live). The chip itself is tested only by automated tests.

## What it is
Agents update often. An update can change the screens PM reads, and then PM can guess wrong. PM keeps a "tested range" of versions for Claude Code and Codex. If an agent is outside the range, PM shows an **Untested version** chip and becomes more careful ("strict mode"). The "canary check" is a free test: it starts the agent in an empty folder, looks at the screen, and closes it. A canary is a small early warning, like the bird in a mine.

## Why it matters
Codex 0.159.3 added an update menu that broke runs: a task was typed over the menu, and Enter chose "Update now". Without a version check, you lose a run and do not know why. With it, PM notices a change early and stops guessing.

## How to use it
1. Do nothing for normal use. PM reads each agent's version at start-up (after about 20 seconds) and then every 15 minutes.
2. If an agent is outside its range, a chip **Untested version** shows in the pane header. An Inbox card may say "Codex 0.171.0 changed its screens. Puppet Master will ask you before acting." (This card does not count as "Needs you".)
3. To look at it yourself, open **Settings > Agents** and find the **Versions** group, **Agent versions**.
4. Read each installed agent: its version, the chip if any, a plain sentence, and the time of the last check.
5. Press **Check this agent** to run the check now. It can take up to a minute.
6. The switch **Check agents after they update** (on by default) makes PM run the check by itself when a version changes. Off: only when you press the button.
7. **Screens I didn't recognise** counts screens that PM could not read. Press **Open folder** to see them. Secrets are removed. Nothing is sent anywhere.

## Tested ranges

| Agent | Range | Seen live with |
|---|---|---|
| Claude Code | 2.1.299 up to, not including, 2.2 | 2.1.299, 2.1.300 |
| Codex | 0.159 up to, not including, 0.170 | 0.162.1 (and 0.159.3 for older fixes) |
| Cursor Agent, OpenCode, Kilo Code | no range set | never shows the chip |

Hook Lane has its own, narrower ranges. See [Hook Lane](19-hook-lane.md).

## How it works (simple)
- **Strict mode** starts when an agent is outside its range, or when the last check on this version found screens it could not read. Then:
  - PM does not press any key for a start-up notice. It parks the run and asks you.
  - PM stops warming this agent and closes its warm workers. See [Standby workers](14-standby-workers.md).
  - Settings says so in plain words: "Puppet Master has not been tested with Codex 0.171.0 yet. It will ask you before closing a start-up notice, and it will not keep one ready in advance."
  - Strict mode never blocks you. You can still use the agent yourself.
  - For an untested version, strict mode ends when the agent is back in range. For "changed screens", it ends when a new check on that version passes, or when the version changes.
- **The check.** PM starts the agent on a hidden terminal in its own scratch folder. It waits for the screen to settle, and needs the ready screen to hold still for 3 seconds. It closes a notice only with the profile's own safe key. It never answers trust or sign-in, and it never types a prompt. So it costs no tokens.
- **Results.** Ready, Asks you (a screen only you can pass), Unrecognised, Never drew, Exited, Start failed, Inconclusive. Only "Unrecognised" means "the screens changed". Only it raises strict mode and the Inbox card.
- **Unknown screens.** If an agent stays still on a screen PM cannot read for 6 seconds, PM saves it (secrets removed) for later. At most 25 per version.

## Limits and what is not done yet
- Claude Code asks about trust in the scratch folder, so the check ends with "Asks you". It reaches "Ready" only in a folder you already trusted. An "open the check folder once to trust it" step is a decision still open.
- The check never types, so it cannot find a problem that only shows when typing.
- Not built: replaying a saved set of screens, an optional test turn, and "one click to make a test from a saved screen".
- The ranges are the versions the recorded screens came from. They are not a promise that other versions fail.
- The check runs in PM's own environment.
- Windows is the only system tested.

## For developers
- `src-tauri/src/canary/`: `version.rs`, `state.rs` (pure rules and plain text), `strict.rs` (`strict::startup_gate` strips `dismiss`), `run.rs` (`drive()`), `engine.rs`, `live.rs`, `unknown.rs`, `captures.rs`.
- Ranges: `TuiProfile::tested_versions()`, constants `TESTED_VERSIONS` in `tui/claude/mod.rs` and `tui/codex/mod.rs`. TOML agents: `[profile] tested`. The `semver` crate judges the range.
- Tauri commands: `canary_status`, `canary_check`, `canary_set_auto`, `canary_dismiss_notice`, `canary_open_captures`. State file: `<app data>/canary.json`. Captures: `<app data>/captures/<agent>/<version>/`.
- UI: `components/cockpit/UntestedChip.tsx`, `CanaryNotices.tsx`, `components/settings/blocks/AgentVersions.tsx`.
- Developer doc: `docs/features/canary-strict.md`.

## Related
- [Trust and start-up screens](15-trust-and-startup-dialogs.md)
- [Standby workers](14-standby-workers.md)
- [Hook Lane](19-hook-lane.md)
- [Supported agents](12-supported-agents.md)
- [Set up agents and Doctor](13-set-up-agents-and-doctor.md)
