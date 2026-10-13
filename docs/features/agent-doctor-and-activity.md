# Agent doctor and plain-language activity line

Branch `feat/agent-doctor`. Two features, committed separately.

## 1. Agent doctor ("Set up agents")

### What and why
Personas B (vibecoders) and C (non-technical): "which of my AI agents work, and how do I fix the ones that don't?"
The app starts tray-only with no first-run hint, so a new user sees an empty cockpit and has no idea why an agent will not start.

User story: open Set up agents, see one row per agent (state, version, plain reason), copy the install command for a missing one, open a visible pane to log in, press Test to prove an agent can answer.

Agent-agnostic: the checker never names an agent. Install hint, login hint, version command and headless capability all come from the `TuiProfile` (code profiles) or the TOML definition (declarative agents).

### Design
- `src-tauri/src/agent_health/mod.rs`: `collect_probes` (CLI agents only, dispatchable, so shells and `opencode_native` are excluded), `check_all` (one thread per agent, so one hung CLI cannot delay the rest), `check_probe`, `run_version` (hard timeout, pipes drained on threads), `classify_failure` / `classify_test`, `test_agent` (one bounded read-only `quick_ask("Reply with exactly OK")`, 60 s wait, only on the Test button).
- States: `unchecked` (installed, version works, not tested), `ready` (deep check answered), `not_installed`, `needs_login`, `error` (with a short reason). `needs_login` comes from login words in the failure text (the message `classify_headless_failure` preserved as `AGENT_REPORTED_ERROR`); anything else is `error: <first line>`.
- Trait additions (`tui/profile.rs`, all default `None`): `version_command()`, `install_hint()`, `login_hint()`. `Descriptor` now carries `login_hint`, and `install_hint` is filled for code profiles too (before: only TOML agents).
- TOML: new `binary.login_hint` (<= 200 chars, display only, not in the fingerprint, so editing it does not need re-approval). `binary.install_hint` and `binary.version_args` already existed; the declarative profile now reports them and its `version_args` runs as the version command.
- Hints for code profiles (verified against the vendors' docs on 2026-10-14):

| Agent | Install | Login |
|---|---|---|
| Claude Code | `npm i -g @anthropic-ai/claude-code` | open it, type `/login` |
| Codex | `npm i -g @openai/codex` | `codex login` |
| OpenCode | `npm i -g opencode-ai` (docs also list `curl -fsSL https://opencode.ai/install \| bash`) | open it, type `/connect` |
| Cursor CLI | macOS/Linux `curl https://cursor.com/install -fsS \| bash`; Windows `irm 'https://cursor.com/install?win32=true' \| iex` | `agent login` (not on the install page; from memory, unverified) |
| Kilo (TOML, `tests/agent-definitions/kilo.toml`) | `npm install -g @kilocode/cli` | open it, type `/connect` (`kilo auth` also manages credentials) |

  Note: OpenCode previously inherited the default `is_installed() == true`. It now checks PATH like the others, so a machine without OpenCode now reports it not installed (this is the correct answer and what the doctor needs).
- Entry points: Tauri commands `agents_health` and `agents_health_test` (`commands/agents.rs`); bridge routes `GET /agents/health` and `POST /agents/health/test {"agent": id}` (`bridge/routes/health.rs`, one `.or_else` line in `routes/mod.rs`). The MCP catalog is untouched.
- UI: `components/cockpit/AgentDoctor.tsx` (+ `styles/agent-doctor.css`, `lib/agent-health.ts`). Glyphs reuse `DotSpinner`: idle = not tested or not installed, check (`exited`) = ready, plus (`waiting_input`) = needs login, cross (`error`) = problem, sweep (`running`) while a test runs. Buttons: Copy install command (missing agents), Open to log in (spawns the agent visibly through the existing `new_pane` flow), Test (agents that can run headless read-only: today Claude, Codex, Cursor). Reachable from the Inbox empty state, the command palette ("Set up agents") and the sidebar footer. First run: when no pane exists and none was ever seen (`localStorage pm.cockpit.everSpawned`), the Inbox shows "Welcome to Puppet Master" with a primary "Set up your agents" button; afterwards it is a quiet secondary button.
- Open source: none needed (std `Command`).

## 2. Plain-language activity line

### What and why
"What is my agent doing right now?" in one sentence for every pane: "Running npm test", "Editing src/app.ts", "Waiting: allow `npm test`?", "Idle", "Error: rate limit exceeded".

### Design
- `TuiProfile::activity(&self, visible_tail, status) -> Option<Activity>` (`tui/profile.rs`). Deviation from the brief: it also takes the pane `status`, because the default combines status, tool row and prompt. Default = `tui/activity.rs::default_activity`:
  - Error: `Error: <last line mentioning error/failed/exception>`
  - WaitingInput: the profile's own `detect_prompt` -> `Waiting: allow `<command>`?` / `Waiting: trust this folder?` / `Waiting for you`
  - Running: the newest line that `is_tool_activity_line` accepts, humanised (`Grepped "x" in .` -> `Searching "x" in .`, `$ python -m pytest -q 9.3s` -> `Running python -m pytest -q`); otherwise `Working`
  - Idle: `Idle`
  - Always one line, at most 72 characters.
- Tracker `src-tauri/src/pane_activity.rs`. No work per byte. `note_output(pane_id)` only inserts into a dirty set; it is called from `bridge/sse.rs` (`push_terminal_sse`, `push_pane_status_sse`), not from the PTY reader. One ticker thread (1 s) reads the screen of each dirty pane, skips panes whose tail+status hash is unchanged, recomputes otherwise, and emits only when the phrase changed. So at most one screen read per pane per second, and zero for silent panes.
- Delivery (chosen over growing `PaneInfo`, which the registry serialises on every list): command `pane_activities` (current map), Tauri event `pty://activity {pane_id, activity}`, and SSE `pane-activity` (for the PWA; the PWA does not render it yet).
- UI: `lib/pane-activity.ts` (`usePaneActivity`, `activityText`); `CockpitApi.activities`. Sidebar row second line shows the phrase instead of the project (project moves to the row tooltip); pane header shows it next to the glyph (state text moves to the tooltip). A phrase whose kind contradicts the current status (up to ~1 s stale) is hidden.
- TOML: `[tui] activity_regex = [...]`, matched case-insensitively against visible lines, newest first. Phrase = named group `text`, else group 1, else the whole match; it is humanised the same way. Invalid regexes fail the definition with `AGENT_DEF_BAD_REGEX` at `tui.activity_regex.N`. Existing `[tui.tool_activity] line_regex` rows also feed the default.

### Which agents get rich phrases (honest)
| Agent | Activity |
|---|---|
| Cursor Agent | rich (its `is_tool_activity_line` knows shell rows and grep/read rows; verified on recorded fixtures) |
| Declarative agents | rich if the TOML has `activity_regex` and/or `[tui.tool_activity]`; Kilo's bundled test definition has neither yet |
| Claude Code, Codex, OpenCode, shells | status-level only: Working / Waiting / Idle / Error. Waiting phrases include the command when the profile's prompt detector provides one. No recorded tool-row captures exist to write reliable patterns from, so none are guessed |

How to add patterns: for a code profile implement `is_tool_activity_line` (or override `activity`); for a TOML agent add `activity_regex`, e.g.

```toml
[tui]
activity_regex = ['^\s*[>*]\s*(?P<text>(?:ran|edited|read) .+)$']
```

## Evidence

Commands run in `D:\pm-agent-doctor` (worktree), 2026-10-14.

- `cargo test --lib` (in `packages/app/src-tauri`, `CARGO_BUILD_JOBS=4`): `test result: FAILED. 1035 passed; 1 failed; 2 ignored` where the one failure is the known `bridge::tests::definitions::approve_requires_app_session`. Baseline before this branch was 1013 tests in the lib (11 agent_health + 1 bridge route + 2 declarative + 8 activity + 5 tracker new).
- New Rust tests:
  - `agent_health::tests::*` (11): fake binaries written to a temp dir (batch on Windows, shell on unix) print a version, exit 3 with stderr, hang past the timeout (`hanging_version_command_times_out`), print nothing; plus `classify_failure` / `classify_test` mapping, and `registered_agents_carry_hints_and_exclude_shells`.
  - Hint plumbing from TOML: `tui::declarative::tests::profile::install_and_login_hints_come_from_the_binary_table`, `version_command_uses_the_declared_version_args`.
  - `bridge::tests::agents_health_route_lists_cli_agents_with_hints`: in-process route call `GET /agents/health` -> 200 `{"agents":[{"id":"claude","state":"unchecked"|"not_installed",...,"install_hint":"npm i -g @anthropic-ai/claude-code"}, ...]}`, no shells. The POST test route needs an `AppHandle` and has no in-process harness in this repo; it is covered by the classification tests and was not exercised live (a live Test would spend tokens and is a GUI-less environment).
  - `tui::activity::tests::*` (8): phrases from the recorded Cursor fixtures (`reply_grep_turn.txt`, `reply_pytest_final_screen.txt`, `command_approval_visible.txt`, `workspace_trust_visible_pending.txt`), generic status-only phrases, error quoting, humaniser, length cap, declarative `activity_regex`, bad regex rejection.
  - Throttle: `pane_activity::tests::a_burst_of_output_costs_one_recompute_per_tick` (5,000 `note_output` calls -> exactly 1 screen read and 1 emit; next tick 0 reads; unchanged screen -> read, no recompute, no emit), `only_a_changed_phrase_is_emitted`, `a_status_change_alone_updates_the_phrase`, `closed_panes_are_forgotten`.
- `npm test --workspace=@puppet-master/app`: `Test Files 49 passed (49)`, `Tests 386 passed (386)`. New: `AgentDoctor.test.tsx` (9: states and glyph mapping, install command only when missing, copy to clipboard, Test never automatic and shows progress then result, login hint + open agent, failed test -> problem row, failed check; first-run Inbox hint), `ActivityLine.test.tsx` (4: stale phrase hidden, sidebar line with project tooltip, fallback to project).
- `npx tsc --noEmit -p packages/app`: no output (clean).
- Fails before, passes after:
  - Frontend: with the tracked-file changes stashed, `npx vitest run packages/app/src/components/cockpit/ActivityLine` -> `Tests 1 failed | 3 passed` (`sidebar activity line > replaces the project with the activity...`); after: all 4 pass.
  - Rust: with the login words removed from `LOGIN_WORDS` in `agent_health/mod.rs`, `agent_health::tests::login_failures_map_to_needs_login` -> `FAILED` (`tests.rs:117`); restored, it passes.
- Screenshots: the `?harness=` scenario tooling lives on `feat/ui-monochrome` and has not been merged here, so there are no PNGs yet.

## Limits and follow-ups
- Not run live: no real agent was invoked (no tokens spent, no GUI). Version output of real CLIs on Windows (`.cmd` shims) is covered only by the fake-binary tests.
- `needs_login` is detected from error text keywords; an agent that fails with unfamiliar wording shows `error` with its first line. The deep check cannot tell "logged out" from "network down" unless the CLI says so.
- A not-logged-in agent shows `unchecked` until Test is pressed (the quick check cannot know without a token).
- Cursor's login hint (`agent login`) and the fact the unattended Test works for Cursor are not verified against the vendor docs.
- Claude, Codex and OpenCode only get status-level activity; richer phrases need real screen captures to write patterns from (`activity_regex` or `is_tool_activity_line`).
- The activity line is Tauri-only for now (PWA receives the SSE event but does not render it). `GET /panes` / `PaneInfo` do not carry it.
- Merge hot spots (all append-only, one or two lines): `lib.rs` (module, two commands registered, ticker init), `bridge/routes/mod.rs`, `bridge/sse.rs`, `bridge/mod.rs` export line, `agent_runs.rs` (`quick` made `pub(crate)`), `hooks/useCockpit.ts`, cockpit `Sidebar.tsx` / `PaneView.tsx` / `Palette.tsx` / `Inbox.tsx` / `CockpitApp.tsx`.
