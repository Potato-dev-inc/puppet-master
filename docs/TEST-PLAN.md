# Puppet Master test plan

A practical checklist a human or agent can run. Features under test: [`FEATURES.md`](FEATURES.md). Map: [`REPO_MAP.md`](REPO_MAP.md).
Counts are the last recorded values (T37 and later work on `refactor/tui-profiles`); treat them as floors, not exact pins.

Rules for live tests: the dev app runs in the "puppet-master dev app" terminal tab; do not start/stop `npm run dev`. Windows must stay on the
secondary (left) monitor: after any visible-pane test run
`powershell -NoProfile -ExecutionPolicy Bypass -File scripts/check-window-monitor.ps1` and expect `OK: no Puppet Master window on the primary monitor`.
Editing files under `packages/app/src-tauri` restarts the dev app (~1 min; poll `http://127.0.0.1:17321/health`).

## 1. Automated suites

| Suite | Command (from repo root unless noted) | Expected |
|---|---|---|
| Rust lib | `cd packages/app/src-tauri; $env:CARGO_TARGET_DIR='C:\Users\you\cargo-target-shared'; cargo test --lib` | 927 passed, 0 failed, 2 ignored (PF3) |
| Rust: TUI only | `cargo test --lib tui::` (same dir/env) | about 252+ passed, 1 ignored |
| Rust: tool registry | `cargo test --lib tool_registry` | about 22 passed (includes the pinned `catalog_version`) |
| Rust: standby / preload | `cargo test --lib standby` (+ `app_windows`, `tray`) | 50 passed (PreloadConfig, migration, default-project warming, count 2, replacement, project change, back-off incl. a worker that reaches ready and then dies) |
| MCP shim bin | `cargo test --bin puppet-master-mcp` (same dir/env) | 30 passed |
| App unit tests | `cd packages/app; npx vitest run` | 2054 passed, 1 skipped (211 files). Grid flicker guards: `TerminalGrid.reflow.test.tsx`, `TerminalGrid.resize.test.tsx` (whole stack, spy on the bridge resize), `TerminalViewport.stable.test.tsx`, `terminal/terminal-session.reflow.test.ts`, `styles/pane-chrome.css.test.ts` |
| Typecheck | `npm run typecheck` (workspaces) | clean for shared, app, mcp-server |
| CLI tests | `npm test --workspace=@puppet-master/cli` | pass |
| MCP vs mock bridge | `npm run test:mcp-operations` / `npm run test:mcp-package` | pass (not run at T37) |
| Live regression | `node scripts/pm-regression.mjs` (needs the running app; spawns real agents) | `28/28 scenario runs passed; 0 failed assertion(s)`; known flaky: `cursor_agent timeout` can fail `WORKER_STOP_FAILED` (PF3 run was 27/28, rerun passed) |
| Read-only parity | `node scripts/pm-regression/readonly-check.mjs` | claude and kilo PASS; the control (a write) is detected |
| Window monitor | `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/check-window-monitor.ps1` | exit 0, `OK: no Puppet Master window on the primary monitor` |
| Perf (optional) | `node scripts/perf-latency.mjs` then `perf-summarize.mjs` | see `docs/perf/latency-baseline.md` |
| Rust: Point and Fix | `cd packages/app/src-tauri; $env:CARGO_TARGET_DIR='C:\Users\you\cargo-target-shared'; cargo test --lib point::` | all pass (new; the count is recorded when the feature lands). Suites under `point/tests/`: `origin` (every refusal: LAN, public, `file:`, `javascript:`, userinfo `http://evil@localhost`, `localhost.evil.com`, IDN, `0x7f.1`, trailing dot; own ports 17321..17399, 1420, the dev port, the CDP port; navigation rules), `pick` (caps, fragment dropped, token in query redacted, foreign origin refused, NaN and negative rects clamped, `how` unknown rejected), `source` (each accepted form; `..` escape and outside-root files dropped), `message` (golden text for 1, 2 and 3 picks, no-shot reasons, length under 8000), `shot` (crop size at dpr 1.5 and 2, outline pixels, prune keeps the newest N), `target` (opener first, shells dropped, protocol and run-controlled panes greyed with the outbox sentence), `session` (max 3 picks, TTL, discard, drafts), `capabilities` (no capability matches `preview-*`, no `remote` key, `pointbox` present) |
| Rust: send path | `cargo test --lib point::` (the `point_send` case) and `cargo test --lib pane_outbox::` | `point_send` makes exactly one `Outbox::send` call through a fake host and writes nothing else; outbox tests unchanged |
| Rust: full (when `Cargo.toml` or `lib.rs` changed) | `cargo test --lib` (same dir/env) | all pass, 0 failed |
| App: Point and Fix UI | `cd packages/app; npx vitest run src/point-picker.test.ts src/lib/point-api.test.ts src/components/point src/components/chat/ChatComposer.draft.test.tsx src/main.pointbox.test.ts src/components/cockpit/PaneView.point.test.tsx src/components/cockpit/rail/background.point.test.tsx src/components/cockpit/Palette.point.test.tsx` | all pass. `point-picker` loads the real `picker.js` in jsdom: origin guard (frame or other origin defines nothing), selector unique and short, hashed classes dropped, input values never read, password gives no text, React / Svelte / Vue / Angular stubs, ArrowUp/ArrowDown, Esc disarms, click does not reach the page's own listener, `take()` returns once. `PointBox`: picks render, missing thumb tile, Enter sends with the chosen target, Shift+Enter is a new line, Esc hides, greyed target cannot send, a refused send keeps the text, 3 picks disable "Point another", blocked-link notice |
| Type check | `npx tsc -p tsconfig.json --noEmit` (in `packages/app`) | no errors |
| Personal paths | `node scripts/check-no-personal-paths.mjs` (repo root) | no findings |

`cargo test` needs `packages/app/dist` (run `npx vite build` in `packages/app` once). After any change to `tool_registry/`, the shim or bridge code it
depends on, rebuild the dist MCP exe with a separate `CARGO_TARGET_DIR` and rename-swap (REPO_MAP section 6).

## 2. Manual / live cases

Setup: dev app running (tray-only: no main window ever appears), bridge healthy (`GET /health` gives `build_id`), `kilo` installed and its definition approved. "Bridge" = `http://127.0.0.1:17321`.

### 2.1 Agent definitions (Settings > Agents)
1. **Import to active (Kiro).** Import `kiro.toml` (Import button). Expect: row appears at once, state "Needs approval", no restart. Open Review: argv shown (`kiro-cli chat --output-format stream-json ...`), binary path, warnings, diff. Approve. Expect: state Active; toast says the registry updated, no restart needed. File > New session menu lists Kiro greyed (headless-only) with a reason; the Overview Agents card lists it. Both update without restart.
2. **Check with fixtures (Kilo).** Press Check on kilo. Expect `9/9 PASS`.
3. **No fixtures folder.** Check a definition that has no fixtures folder. Expect a neutral "No captures yet for <name>. Put captured screens or JSON output in <dir> ..." message (not a red FAIL), the exact folder path, and a "Create folder" button; after Create folder it reads "exists but has no captures yet". A typed path that does not exist says so plainly (no OS error text). Generic agents add "Captures are optional for experimental (generic) agents." (fixed in c7bb13a)
4. **Revoke.** Revoke kilo. Expect state Needs approval and `run_agent(agent_type=kilo)` refused with an `AGENT_DEF_*` message. Re-approve restores Active.
5. **Add as generic agent.** Add a new generic agent for an installed CLI. Expect the experimental badge and a "needs approval" flow.
6. **Rec button / drafts.** Open a pane, press Rec in its header, interact, stop. Expect a draft under Drafts (file in `<app data>/agents/drafts/`), never auto-loaded or approved. Validate the draft (Validate/Review) and copy it to the agents folder to approve.
7. **Export / import round trip.** Export kilo (zip), import it under a different folder or after deleting a copy. Expect it lands Needs approval (never auto-approved).
8. **Hot reload.** Edit a label in `<app data>/agents/kilo.toml` while the app runs. Expect the row refreshes within about a second, no restart (label-only edits keep the fingerprint). Edit argv: state turns "Approval stale".
9. **Invalid edit.** Break the TOML of an Active file. Expect the previous registry kept (agent still listed), the row reported invalid when it settles; fixing it recovers.
10. **Sources and consent.** Settings > Agents > Sources: project and extra dirs are OFF by default. Put a definition in `<project>/.puppet-master/agents/`; expect a consent banner for that project; nothing loads until consent is given, and even then it needs approval. Extra dir: add a dir, expect it needs its own consent.

### 2.2 Runs and receipts
1. **Headless run.** `run_agent(agent_type=kilo, project_path, task="reply ack")` (default headless). Expect result text, receipt with `agent_definition` {id, fingerprint, binary}, `read_only` status, no window opens.
2. **read_only.** Same with `read_only=true`; a task that writes a file must not write. Run `node scripts/pm-regression/readonly-check.mjs`.
3. **followup_task / resume.** `followup_task(handle, "what was the word?")`: same provider session id, `continuity: resume`.
4. **Visible run is the default.** `run_agent(agent_type, project_path, task)` with no `headless`/`placement` through MCP (for example from Codex, 2.5). Expect one pop-out pane window (secondary monitor in dev, unfocused), the result, receipt `fallback: null`, and the window gone after the run (`keep_pane=false`). Then run the window-monitor script.
5. **Kiro fallback.** The same call with `agent_type=kiro` (`modes.visible=false`) returns a headless result with `fallback.reason = "no_visible_mode"` and opens no window. Also: `placement="grid"` or `auto` is accepted and ignored (still a pop-out window; there is no grid).
6. **answer_prompt.** In a visible kilo run, make it write outside the project. Expect a `command_approval` prompt surfaced via `wait_agents` (`needs_input`); `answer_prompt(allow_once)` lets the turn finish.
7. **Mid-run edit.** Start a long kilo run, edit `kilo.toml` argv during the run. Expect the in-flight run completes with its result; the next `run_agent` is refused with `AGENT_DEF_APPROVAL_STALE`; restoring the file (or re-approving) makes it Active again.
8. **UNKNOWN_AGENT_TYPE.** `run_agent(agent_type="nonesuch")`: HTTP 400 `UNKNOWN_AGENT_TYPE` with `context.valid` listing registered ids (not `AGENT_NOT_DISPATCHABLE`).
9. **Keep pane.** Without `keep_pane`, after completion `take_over` fails with `PANE_CLOSED_AFTER_RUN`; `agent_transcript` still works.
10. **Slow-start TUI.** A visible run on a TUI that takes seconds to draw: the prompt is delivered once drawn (up to 20 s), no lost first turn.

### 2.3 Panes-first windows and lifecycle (replaces the grid/main-window cases)
1. **Tray start.** Start the app (dev: the "puppet-master dev app" tab). Expect no window at all, the log line "tray icon created", `GET /panes` = `[]`, and no Puppet Master window rows from the window-monitor script. The tray menu (show hidden icons) has New pane / Panes / Settings... / Quit; left-click opens Settings. **Human step, unverified by agents:** clicking the real tray icon and menu entries.
2. **Settings via second launch.** A second launch of the exe (by hand, or a hidden `Start-Process` in dev) opens the Settings window on the Agents tab (focus allowed, user-opened) and the second process exits. No main window appears.
3. **New pane.** `puppet-master.exe --new-pane kilo` (or tray New pane > kilo): one pane window, unfocused, on the anchor monitor; a second one is cascaded (offset) from the first.
4. **Close pane window kills pane.** Close a pane window (X / WM_CLOSE): `GET /panes` no longer lists it and its process is gone.
5. **Window anchor on the secondary monitor (dev).** With `packages/app/src-tauri/pm-dev.local.json` present the app logs "window anchor <x>,<y> (pm-dev.local.json)"; Settings and pane windows open there and `scripts/check-window-monitor.ps1` exits 0. Priority is `PM_WINDOW_ANCHOR` > `settings.json` `pane_window_position` > primary monitor.
6. **Pane exit closes its window.** `kill_pane_process` or a finished run closes the window within about 3 s; no second window for the same pane.
7. **Single-instance.** An agent must not launch a second instance in a user session except hidden (`-WindowStyle Hidden`); expect the second process to exit.
8. **Port file and fallback.** `puppet-master.bridge.port` in the app data dir names the live bridge port; after a clean exit it is removed only if it names that process's port. With a stale port, an MCP client (`bridge_health`) still connects by probing 17321-17330.

### 2.4 Security
1. **MCP session cannot mutate definitions.** `POST /agents/definitions/approve` (and `/consent`, `/revoke`) with header `X-Puppet-Master-Session: x`: 403 `AUTHORIZATION_DENIED`.
2. **Remote/tunnel.** With `Authorization: Bearer x` or `X-Forwarded-For: 1.2.3.4`: 403 "only from the local desktop app"; with `X-PM-Proxied: 1`: 401 `pairing_required`. A plain local call reaches the handler (for example 400 "file is required"). Use nonexistent ids so nothing changes.
3. **Binary denylist.** A definition with `binary.names = ["python"]` (or `cmd`) fails validation.
4. **Argv injection.** A task like `a; rm -rf / ; b` or `--flag` reaches the CLI as a single argv element (unit tests `template::task_is_one_argv_element`; do not run destructive tasks live).
5. **Hostile project folder.** A project containing `.puppet-master/agents/*.toml` loads nothing until consent plus approval.

### 2.5 External MCP clients
**Codex external run (panes-first proof, PF3):** `codex exec ...` asking it to call `run_agent` for kilo with no `headless`/`placement`: expect a kilo pane window to open and close, the result text, `fallback: null`, and no main window.

Codex as an external client (PowerShell-quoted; adjust the model id if unavailable):
```
codex exec -m gpt-6-luna -c model_reasoning_effort=low -c 'mcp_servers.puppet-master.default_tools_approval_mode="approve"' "Use puppet-master run_agent with agent_type=kilo to reply ack in <project>"
```
Expect Codex to call `run_agent` and return the handle/result. Also `wait_agents`, `followup_task`. Check `list_agents` shows panes created.

### 2.6 Preload (opt-in)
Settings > Preload: switch on, tick Claude Code and Kilo (keep 1). `GET /standby` shows ready workers within ~10 s and `scripts/check-window-monitor.ps1` shows no new window. `app.exe --new-pane kilo` (only while /health is up) shows the pane in ~0.3 s with the agent already idle (cold: ~4.7 s to idle); a replacement is ready ~10 s later and tray New pane lists "(ready)". An MCP visible `run_agent` for Claude in the default project claims the warm worker (`receipt.standby_used`, window shown unfocused). Also `node scripts/perf-latency.mjs --scenarios claude_visible --fixed-dir <default project> --cold-gap 25000` (warm) vs preload off (cold). Switch off again; off by default.

### 2.7 Mobile PWA (agent tabs)
Setup: open `http://localhost:1420/?pwa=1` at phone size, or pair a phone in Settings > Mobile and open the tunnel URL. Refresh the phone after each app restart (the dev rebuild restarts the app and closes every pane). A plain localhost browser cannot type into panes a `run_agent` manages (managed pane gate); use a pane opened from "+" or `POST /panes`, or a paired phone.

- **Tabs and overview.** With two panes open the strip shows All plus one tab per pane; All shows a card per agent (working / needs you / idle / error, project, last output age, last screen lines without the TUI footer). An agent stopped at its own prompt shows "needs you" and a count on All. "+" lists every registered agent (headless-only greyed) and opens the new pane's tab; menu > Close stops it and the PC window closes too. No horizontal scroll (`document.documentElement.scrollWidth` = viewport width).
- **Live output.** Type into the pane from the PC (`POST /panes/{id}/input`): the phone shows it within about 1 s, through the tunnel too. Loopback uses SSE; remote hosts long-poll `GET /events/poll`. Check a tunnel with a stream read: SSE gives 0 bytes through a quick tunnel, the poll answers at once.
- **Phone-sized panes.** Opening a tab resizes the pane to the phone's grid (`GET /panes`, e.g. 56x56 on the emulator); the agent redraws at that width with no CSS scaling (`.terminal-scale-stage` has no transform). Back to All restores the desktop size (121x36). One `POST /panes/{id}/viewport` and nothing more restores after about 15 s (`phone size lease expired` in the log).
- **Keyboard.** Tap: the keyboard comes up and stays up; the tab strip stays at the top and the pane gets fewer rows (emulator 56x56 -> 56x32), drawn full size with the agent's input line above the keyboard. Close it with the system key, tap once: it opens again. The log shows one resize per keyboard change, never a back-and-forth.
- **Typing.** Type `123456` then erase it: the phone and the PC show the same prompt at every step (no stray characters; the phone draws only what the agent echoes).
- **Two phones on one pane.** Leases from two viewers (`viewer` in the body), e.g. 56x56 and 48x21, give 48x21 and stay there through renewals; the smaller one leaving gives 56x56, the last one leaving restores the desktop size.
- **PC window.** While a phone has the tab open the pop-out shows "Sized for your phone" (`viewer_held` in `GET /panes`); it shows the agent normally again when the phone leaves the tab.
- **Legacy.** `?pwa=legacy` still loads the old orchestrator UI.
- **Headless iPhone check.** Chrome headless over CDP with an iPhone user agent, touch and a fake `visualViewport` whose height drops by the keyboard height: run the steps above and read pane sizes from `/panes`, prompt lines from `.xterm-rows`, and whether the focused input element survives the resize.

### 2.8 Point and Fix (live check, spec section 6)
Setup: dev app running on its own profile (never the founder's app). A small test app in a test folder, `index.html` with an `<h1>`, a card grid, a `button[data-testid=buy]`, a password input, a link to `https://example.com`, and a React-like element whose `_debugSource` points at `src/Card.jsx:12` (with that real file). Serve it from a shell pane with `python -m http.server 5199` (or `npx --yes http-server -p 5199`), so the Preview chip shows `http://localhost:5199`. Use one tiny agent turn at most, in the test folder only.

1. Click the Preview chip on the shell pane. Expect a preview window beside the cockpit, on the same monitor.
2. In the preview, `window.__TAURI_INTERNALS__?.invoke('pane_previews')` fails or is missing, and the preview's `localStorage` has no Puppet Master keys.
3. Click the example.com link. Expect no navigation and a notice in the box or window: "Blocked a link to example.com. The preview only shows your app."
4. Press Alt+P, move over the button, click it. Expect an outline and a label `button[data-testid=buy]`, then the Point box next to the element with a thumbnail and `Card · src/Card.jsx:12`. The PNG is in `<test folder>/.puppet-master/shots/` and `git status` in the test folder does not list `.puppet-master/` (if it is a repo).
5. Pick the password input. Expect no typed value anywhere in the box, and the Copy text has no value in it.
6. **One real agent turn.** Open an agent pane in the test folder. Pick the `<h1>`, write `change this heading text to "Hello Point"`, choose that agent under To:, press Enter. Expect the message to show as a pending bubble in the Chat tab, then to send when the agent is free. The message names the element, selector, component, page and the screenshot path. After the turn the file has the new text, and the preview shows it after a reload. If the agent shows a trust, permission or login question, stop, take a screenshot and report it. Do not answer it.
7. **Second agent, no real turn.** A fake or wizard-added agent pane receives the same message through the outbox (state `sent`). This shows the path works for any agent.
8. Click Edit in Chat. Expect the cockpit to select that pane, open its Chat tab, and put the text in the composer. Nothing is sent.
9. Close the preview window. Expect a log line that the pull loop stopped, and no WebView2 process left for the preview data folder after 10 s.

Expected: each step matches what it says, and the message contains no typed password value and no Puppet Master key. Screenshots are Windows only in v1.

## 3. Known flaky tests (collected from the progress log and T37 evidence)
- `tui::descriptor::descriptor_builds_all_nine` - global registry race (fixed by `registry::test_serial()`; tests that swap/count the global registry must take it).
- `operations_contract_tests::recovery::startup_recovery_releases_locks_held_by_interrupted_operations` - "Access denied" on Windows; rerun.
- `operations::tests::recovery::recover_all_visits_nonselected_indexed_projects` - seen once (T26).
- `pm-regression` `cursor_agent timeout` scenario - `WORKER_STOP_FAILED` (taskkill could not stop the headless worker tree): seen earlier and again at PF3 (27/28; rerun repetition 2 passed). Known flaky, unrelated to windows.
- Free kilo model (`kilo/kilo-auto/free`) sometimes refuses tasks or reports `model_mismatch: true`; not a code bug.
- Rebuild windows: any edit under `src-tauri` drops the bridge for about a minute; a live run during that fails spuriously.

## 4. Known gaps / not proven live
- **Tray icon and menu clicking by a human** (menu model unit-tested; tray creation seen in the log; real clicks untested).
- **`visible_failed` fallback live** (pane fails to launch / prompt not submitted): unit tests only. `no_visible_mode` (Kiro) was proven live in PF1.
- Clean-state Kilo approval flow (delete approvals, re-approve) was skipped at T37 (classifier denied); needs_approval -> approve and the stale-edit refusal were proven live at T36/T36b and by unit tests.
- Remote/tunnel behaviour tested with headers on loopback only (no real tunnel).
- Kiro runs and the Kiro import/approve flow were exercised live (demo videos, Codex runs). Rec -> draft was exercised live ("Draft saved" in the pane header); turning a recorded draft into an approved definition was not.
- Visible declarative results are screen-inferred (flagged); free models may refuse.
- Env/stdin launches, `[mcp_host]` writer UI/route, SPEC Q6/Q7, T29 (blocked), `extract_model` parity: not implemented.
- `npm run test:mcp-package` / `test:mcp-operations` were not run at T37.
- Mobile PWA: verified on an Android emulator through the Cloudflare tunnel, in headless Chrome as an iPhone, and from a recording of a real Android phone; not yet on a real iPhone. The emulator stops showing its on-screen keyboard once host-keyboard input is used (`show_ime_with_hard_keyboard = 0`): tap its keys instead.
- The removed legacy UI (grid, Overview, orchestrator sidebar) has no live tests by design; its code is unreachable. The Settings > Agents cases in 2.1 run in the tray Settings window.
