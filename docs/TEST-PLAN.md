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
| Bring All My Chats: Rust | `cd packages/app/src-tauri; $env:CARGO_TARGET_DIR='C:\Users\you\cargo-target-shared'; cargo test --lib session_chat::history` (also `session_chat::declared::history`, `chat_index`, `commands::chat_import`, `tui::claude::history`, `tui::codex::history`, `tui::opencode::history`, `tui::conformance`) | Static count on `feat/bc-docs`: 162 Bring All My Chats test functions (+1 ignored `regenerate_fixture_db`), counted from source, not run in the docs pass. The filters `chat_index` and `tui::conformance` also run older tests. The count includes the guard tests in `session_chat/history/guard.rs` |
| Bring All My Chats: UI | `cd packages/app; npx vitest run src/components/history src/lib/chatImport src/components/settings/blocks/BringChatsSettings src/lib/chatTab` | Static count: 45 test cases in those files, not run in the docs pass. `CockpitApp.test.tsx` (31 cases, 3 for the old-chat host) runs with `npx vitest run src/CockpitApp` |
| Bring All My Chats: fixture builder | `node --test scripts/make-fixture-history-home.test.mjs` | Static count: 15 cases (layout, refusals, extra files, exit codes), not run in the docs pass |
| Agent Store: Rust | `cd packages/app/src-tauri; $env:CARGO_TARGET_DIR='C:\Users\you\cargo-target-shared'; cargo test --lib agent_store` (also `agent_install`, `tui::install`, `tui::declarative`) | Static count: about 110 test functions in `agent_store`, not run in the docs pass. Includes `the_shipped_catalog_has_no_problems` and `the_embedded_list_matches_the_catalog_folder_on_disk` |
| Agent Store: catalog check | `cargo test --lib agent_store::check` (same dir/env) | Expected: all pass. A failure names the entry and the rule (see `docs/features/agent-store.md`, the 11 rules) |
| Agent Store: UI | `cd packages/app; npx vitest run src/components/store src/lib/agent-store src/lib/agentStoreCheck` | Not run in the docs pass |
| Agent Store: scripts | `node --test scripts/gen-store-catalog.test.mjs scripts/store-verify-publishers.test.mjs`; `node scripts/gen-store-catalog.mjs --check`; `node scripts/store-verify-publishers.mjs --offline` | Expected: tests pass; `--check` exits 0 when `embedded.rs` is current; `--offline` lists the packages with no network |
| Agent Store: publishers (network) | `node scripts/store-verify-publishers.mjs` | Maintainer, before a release, output kept. Exit 0 when every package's repository matches its entry |
| Perf (optional) | `node scripts/perf-latency.mjs` then `perf-summarize.mjs` | see `docs/perf/latency-baseline.md` |
| Rust: Point and Fix | `cd packages/app/src-tauri; $env:CARGO_TARGET_DIR='C:\Users\you\cargo-target-shared'; cargo test --lib point::` | all pass (new; the count is recorded when the feature lands). Suites under `point/tests/`: `origin` (every refusal: LAN, public, `file:`, `javascript:`, userinfo `http://evil@localhost`, `localhost.evil.com`, IDN, `0x7f.1`, trailing dot; own ports 17321..17399, 1420, the dev port, the CDP port; navigation rules), `pick` (caps, fragment dropped, token in query redacted, foreign origin refused, NaN and negative rects clamped, `how` unknown rejected), `source` (each accepted form; `..` escape and outside-root files dropped), `message` (golden text for 1, 2 and 3 picks, no-shot reasons, length under 8000), `shot` (crop size at dpr 1.5 and 2, outline pixels, prune keeps the newest N), `target` (opener first, shells dropped, protocol and run-controlled panes greyed with the outbox sentence), `session` (max 3 picks, TTL, discard, drafts), `capabilities` (no capability matches `preview-*`, no `remote` key, `pointbox` present) |
| Rust: send path | `cargo test --lib point::` (the `point_send` case) and `cargo test --lib pane_outbox::` | `point_send` makes exactly one `Outbox::send` call through a fake host and writes nothing else; outbox tests unchanged |
| Rust: full (when `Cargo.toml` or `lib.rs` changed) | `cargo test --lib` (same dir/env) | all pass, 0 failed |
| App: Point and Fix UI | `cd packages/app; npx vitest run src/point-picker.test.ts src/lib/point-api.test.ts src/components/point src/components/chat/ChatComposer.draft.test.tsx src/main.pointbox.test.ts src/components/cockpit/PaneView.point.test.tsx src/components/cockpit/rail/background.point.test.tsx src/components/cockpit/Palette.point.test.tsx` | all pass. `point-picker` loads the real `picker.js` in jsdom: origin guard (frame or other origin defines nothing), selector unique and short, hashed classes dropped, input values never read, password gives no text, React / Svelte / Vue / Angular stubs, ArrowUp/ArrowDown, Esc disarms, click does not reach the page's own listener, `take()` returns once. `PointBox`: picks render, missing thumb tile, Enter sends with the chosen target, Shift+Enter is a new line, Esc hides, greyed target cannot send, a refused send keeps the text, 3 picks disable "Point another", blocked-link notice |
| Type check | `npx tsc -p tsconfig.json --noEmit` (in `packages/app`) | no errors |
| Rust: Text your agents | `cd packages/app/src-tauri; $env:CARGO_TARGET_DIR='C:\Users\you\cargo-target-shared'; cargo test --lib channels` | all pass. Covers the policy table (every attention kind), command parsing, the 1,500-character render cap and the redaction, the one-time code (40 bits, 10 minutes, 5 wrong tries), button tokens, rate buckets, the engine rules with a fake host and bot, the poller (401, 403, 409 stop it; offset saved). No test talks to Telegram |
| Rust: Text your agents, chat-answer guard | `cargo test --lib attention::answer` (same dir/env) | all pass (the `Via::Telegram` refusals for trust, consent, budget, limit and non-low approvals) |
| App: Text your agents UI | `cd packages/app; npx vitest run src/lib/telegram-api src/lib/telegram-bridge src/components/settings/blocks/TelegramSetup src/components/mobile/TelegramOffRow` | all pass (the components are not mounted in the app yet) |
| Rust: Clip It | `cd packages/app/src-tauri; $env:CARGO_TARGET_DIR='C:\Users\you\cargo-target-shared'; cargo test --lib clip` | all pass. Covers the plan model, the redaction of planted keys (split over rows, wrapped, colour-split, env values), the window and refusal sentences, the store (tokens, magic bytes, one-time save), and the guard tests (no bridge or MCP mention, no pane writes) |
| App: Clip It UI | `cd packages/app; npx vitest run src/lib/clip src/lib/mp4Mux src/components/chat/ClipSheet src/components/chat/clipSheet src/components/chat/ChatExportMenu.clip src/components/chat/SessionChatPanel.clip src/components/chat/SessionChatView.clip` | all pass. The MP4 has no user-data atoms; the clip files make no network calls |
| Rust: Scores on your code | `cd packages/app/src-tauri; $env:CARGO_TARGET_DIR='C:\Users\you\cargo-target-shared'; cargo test --lib scores` | expected all pass (not run for the docs change). Covers the sample tiers and Wilson interval, the `ahead` rule, the join and roll-up, the undo log, the guard that no shipped file outside the module names it, and that a planted prompt, path, pane id or e-mail never reaches the report |
| App: Scores on your code UI | `cd packages/app; npx vitest run src/lib/agent-scores.test.ts src/components/cockpit/AgentScores.test.tsx src/styles/cockpit-scores.css.test.ts` | expected all pass (not run for the docs change). The page is not checked in the running app yet (no live run) |
| Rust: Pick for me | `cd packages/app/src-tauri; $env:CARGO_TARGET_DIR='C:\Users\you\cargo-target-shared'; cargo test --lib scores::pick` | expected all pass (not run for the docs change). Covers the skips (not ready, own login out of usage, shared plan), the usual saved login, the 5-run minimum, the clear gap, too early and close call fallbacks, the sentences and "Why this one" lines, and the limit of one login only (no switch to another login) |
| App: Pick for me UI | `cd packages/app; npx vitest run src/lib/pick-for-me.test.ts src/components/cockpit/NewAgentMenu.pick.test.tsx src/styles/cockpit-pick.css.test.ts` | expected all pass (not run for the docs change). The row and panel are not checked in the running app yet (no live run) |
| Rust: Package check, offline | `cd packages/app/src-tauri; $env:CARGO_TARGET_DIR='C:\Users\you\cargo-target-shared'; cargo test --lib prompt_risk::packages` | Static count: about 53 test functions in `prompt_risk/packages/` (the parser cases are table rows inside a few of them), not run in the docs pass |
| Rust: Package check, lookups | `cargo test --lib package_check` (same dir/env) | Static count: about 143 test functions in `package_check/` (registries, HTTP against a local test server, cache, private servers, the card view), not run in the docs pass |
| Rust: Package check, prompt and policy | `cargo test --lib pane_prompt` and `cargo test --lib approval_policy` (same dir/env) | Static count, not run in the docs pass: the `packages` cases in `pane_prompt/tests.rs` and `approval_policy/tests.rs` (look-alike and link installs never auto-approved) |
| Package check: popular list script | `node --test scripts/package-check/popular.test.mjs` (repo root) | Static count: 7 cases, no network, not run in the docs pass |
| App: Package check UI | `cd packages/app; npx vitest run src/components/cockpit/PackageFacts.test.tsx src/lib/package-check.test.ts src/components/settings/sections/PackageCheckRow.test.tsx src/components/cockpit/ApprovalPrompt.test.tsx` | Static count: 19, 9, 10 and 28 cases (ApprovalPrompt's 28 include older cases), not run in the docs pass |
| Package check: end to end | `cd packages/app/src-tauri; cargo test --features dev-fake-tui --test fake_tui_e2e package_check -- --test-threads=1` | 8 passed (3 runs in a row). A fake registry on 127.0.0.1 (`fake_tui/package_rig.rs`): `npm install lodahs` facts and the requests it sees, a closed server, a slow server, a workspace `.npmrc`, settings off, a stale id, a secret as a name, two ecosystems on one line |
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

### 2.9 Bring All My Chats (fixture home only)
Never run against the real home. Build the fake home: `node scripts/make-fixture-history-home.mjs D:\pm-night\bc-fixture-home` (add `--extra-claude <file>` and `--extra-codex <file>` for the two real chats from the check's own test pane). Start the app with `PUPPET_MASTER_PROFILE=bc-live` and `PUPPET_MASTER_HISTORY_HOME=D:\pm-night\bc-fixture-home` on its own CDP port, then run `node scripts/verify-bring-chats.mjs D:\pm-night\bc-live --cdp <port> --expect claude=2,codex=3,opencode=2` (fixture counts; add the real counts when the extra files are in). The script checks the card counts, the Codex toggle, "Add N chats", the Ctrl+Shift+F hit for "stripe webhook" with the "Old chat" tag, and the read-only viewer. Still by hand: Remove imported chats, index off, Continue in a pane (Claude and OpenCode), the planted-secret byte check on `chat-index.sqlite` and `-wal`, and `scripts/check-window-monitor.ps1`. The card steps need the cockpit wiring of the card (BC-23).

#### 2.9 steps (this branch)
Use a new empty folder for the fake home, outside your real home and outside any agent folder.

1. Build the fake home: `node scripts/make-fixture-history-home.mjs <fixture-home>`. It refuses the real home and its agent folders. Add `--extra-claude <file>` or `--extra-codex <file>` only for session files you made in a test pane of your own (the live check's own panes).
2. Start the app on its own profile and CDP port, with `PUPPET_MASTER_PROFILE=bc-live` and `PUPPET_MASTER_HISTORY_HOME=<fixture-home>`. Use a fresh profile: step 3 expects every chat to be new. The script refuses port 9333.
3. Open **Bring my old chats** from the palette. On this branch no card opens it at start (the first-start card is `feat/bc-card`, not merged here).
4. Run `node scripts/verify-bring-chats.mjs <evidence-folder> --cdp <port> --expect claude=2,codex=3,opencode=2`. The counts are the fixture counts; add the extra files to them. The script waits up to 20 s for the panel. It checks the agent counts, the Codex switch, "Add N chats", the Ctrl+Shift+F hit for "stripe webhook" with the "Old chat" tag, and the read-only viewer, with a screenshot per step.

Still by hand: Remove imported chats (the Settings row shows no chats after it), index off ("Chat search is off"), Continue in a pane for Claude and OpenCode (the resumed chat answers), the planted-secret check on `chat-index.sqlite` and its `-wal` (the fake key and e-mail are not in the bytes), and `scripts/check-window-monitor.ps1`.

Phone check: with a paired phone that has the `watch` cap, search "stripe webhook" in the phone's search. No old chat hit may appear there (`GET /chats/search` skips imported rows).

### 2.10 Text your agents (Telegram) (live check, not run yet)
Setup: a dev app on its own profile (never the founder's app). A bot you made in BotFather for this test (its token only in
the credential store, never in a file or a screenshot). A second Telegram account for the stranger step. The Settings
block must be on screen first (see `docs/features/telegram.md`, Status). Use one small agent turn at most, in a test folder.

1. Paste the token and press **Check**. Expect `@your_bot` and "kept in Windows Credential Manager". The token is not shown again.
2. **Make a code.** From the test account, send `/start CODE` to `@your_bot`. Expect the bot to reply that it is connected.
   On the PC, the name shown is the test account's. Press **Yes, that's me**.
3. Turn Telegram on. Send `/status` and `/panes`. Expect the list and no error.
4. From the second account, send `/start`. Expect **no reply**. Expect the stranger count to go up by one. The audit entry is written at most once an hour, so a second stranger in the same hour adds none.
5. Ask an agent a question that has choices. Expect the question with its buttons in Telegram, and an answer by tap to reach the pane.
6. A low-risk approval: expect **Allow once** (then a confirm tap) and **Deny**. A medium or high-risk approval, or a trust
   question: expect no buttons, only "Open Puppet Master".
7. Check the sent text: no path that starts with a user folder, no `sk-` or `ghp_` text, no e-mail, no terminal dump.
8. Send `/stop`. Expect "Telegram is off. Turn it on again in Puppet Master on your PC." and no further messages. The phone row (if the phone is paired) says "Turn Telegram off" and then "Off. Turn it on again on your PC."

Expected: each step matches. Record screenshots (no token, no real names) and the audit lines for steps 2, 4 and 8.

### 2.11 Agent Store (live plan, not run yet)

Feature page: `docs/features/agent-store.md`. Set up an isolated app profile, with its own bridge and CDP ports, and a window
on the left monitor (the same recipe as 2.9). The live run must not install anything on your own PC. Use a spare PC or a
throwaway VM for a real install. Keep every profile folder and list it in the report.

1. **Grid.** Open "+" > More agents. Expect 19 cards. Search `qw` (Qwen Code only). Check each filter chip and the
   Tested and license chips. Expect "found on this PC" on any agent whose program is on PATH. Take screenshots in light
   and dark, at wide and narrow widths.
2. **Install sheet, not run.** Qwen Code: the npm command, the "This comes from" line, and a Node.js step if Node is missing.
   Press Cancel. Kimi Code: the script step, the trust tick, and Run off until ticked. Press Cancel.
3. **Add.** On an agent whose program is installed: Add. Expect a row in Settings > Agents reading "Added from the Agent
   Store", and a pane that opens. Send one short task. Expect the Chat tab to show the reply read from the screen. If the
   agent shows a trust question, take a screenshot and do not answer it.
4. **Check it works.** On the added agent (Ready): the cost sentence, Start, and the result line "N of M checks passed on
   version X". Confirm no file was written to the agents folder and nothing was approved by the wizard.
5. **Remove.** Remove from Puppet Master, then put the file back from Settings > Agents. For an agent the Store did not
   install, expect no Uninstall button (the sheet shows no reason yet; a known gap).
6. **Install and Uninstall, optional.** Only on a spare PC or throwaway VM: Install, then Add, then Open (sign in) and
   Uninstall of Qwen Code. If no such machine is available, write "not run" for this step. Never fake it.
7. **Negative checks.** A file of your own with the same id as a shipped agent: the card says "You have your own <id>
   definition", and Add does nothing. Edit a Store-added file: the card says "Needs your review", and the agent does not start.
8. **Window monitor.** After the visible-pane steps, run `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/check-window-monitor.ps1`
   and expect `OK: no Puppet Master window on the primary monitor`.

### 2.12 Package check (live check, not run yet)

Blocked until the desktop lookup is wired (task PC-13: `package_check_lookup` registered in `lib.rs`). Without it the card
shows "Could not check" under the offline rows, and the live check cannot pass. Spec section 6.

Setup: isolated profile only: `PUPPET_MASTER_PROFILE=pc-live` with its own bridge and CDP ports. Never ports 17321, 17322
or 9333, and never the founder's app data. Window on the left monitor (bounds left -1852, top 121). Recipe: the pane-window steps in section 2.3 and the
`scripts/check-window-monitor.ps1` check after the run.

1. Open a pane in a test folder with a Claude Code agent (or a wizard-added agent). Ask it for `npm install lodahs` in that folder.
   Expect the approval card, with a Package check block at once: `Looks like lodash (2 letters swapped)` and the card at High risk.
2. Within 8 s expect one registry fact or a "Not found on npm" line for `lodahs` (the real registry decides). Record which row text appears.
3. Open Settings > General > Safety, turn "Check packages before installs" off, ask again. Expect `Not checked: package lookups are off`
   on the row, the look-alike line still there, and no request to the registry.
4. **The checker never answers the prompt.** No Allow, no Deny. Screenshot, read the card, then close the pane this run
   opened (by its own PID or the app's close). Confirm the agent's log shows no answer.

Expected: each row matches what it says; the card's buttons, their order and the focus do not move when the facts land;
the only request is one name to the public npm registry.

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
- **Bring All My Chats: phone search** skips old chats (`search_live`); fixed in this batch, guard test `all_chats_search`. The paired-phone check in section 2.9 is still by hand.
- **Bring All My Chats: first-start card** is on `feat/bc-card` (BC-13) and not merged into this branch. Section 2.9 step 3 depends on it.
- **Bring All My Chats: OpenCode layout** is taken from the installed OpenCode 1.19.0 program, not from a live database. A live session is needed to confirm it.
- **Bring All My Chats: no live run** is recorded for this branch. Section 2.9 is the plan.
- **Agent Store: no live run.** The grid, Install, Add, Check, Remove and Uninstall have unit tests only. Section 2.11 is the plan.
- **Agent Store: no end-to-end test** with a made-up agent on this branch. `npm run test:fake-tui` runs only the existing fake-TUI tests.
- **Agent Store: built-in cards show the vendor name as the title** (Anthropic, OpenAI, Anysphere, SST), and the four built-in
  agents show the Tested chip while their entries say `windows_tested = false`. See `docs/features/agent-store.md`, Known gaps.
- **Agent Store: Basic support entries are not tested on Windows.** Most install commands and sign-in sentences are unchecked.
