# Puppet Master end-to-end tests (with video)

Tests that drive the **real app** (its WebView2 window, over the DevTools protocol) with **fake agents only**: no real agent CLI, no network, no cost. Every test records a video. Run one test, an area, or all of them from the command line or from a small local dashboard.

```
packages/e2e/
  tests/<area>/<name>.test.mjs   one file per test (meta + run)
  tests/_helpers.mjs             shared steps (openChat, sendChat, selectPane, replyScript)
  tests/_template.test.mjs       copy this to start a test
  fixtures/agents/*.toml         the fake agents the app knows (nimbus)
  src/                           launcher, page driver, recorder, runner, CLI, dashboard server
  dashboard/                     the dashboard page (plain HTML + JS)
  scripts/build.mjs              builds the app once
```

## Build once

```bash
npm run e2e:build
```

Builds the web UI, the app (debug, with the dev-only `dev-fake-tui` feature) and `tools/pm-fake-tui` (the fake agent). Output goes to `packages/e2e/.build/cargo` (ignored). Put it somewhere else with `E2E_CARGO_TARGET=D:/pm-cargo-e2e npm run e2e:build` (set the same variable when you run). First build is about 3 minutes, a rebuild after a Rust change about 2 minutes (`--skip-ui` skips the web UI, `--skip-rust` skips cargo). Rebuild after changing the app; the tests themselves need no build.

## Run

```bash
npm run e2e -- --list                       # every test, by area
npm run e2e                                 # all tests, with video
npm run e2e -- --only chat.send-message     # one test
npm run e2e -- --only "chat.*,settings"     # a glob, an area, several (comma separated)
npm run e2e -- --only tag:smoke             # by tag
npm run e2e -- --only panes --no-video      # faster: no recording
npm run e2e -- --only chat.send-message --keep-app   # leave the app running to look at it
```

| Flag | What it does |
|---|---|
| `--no-video` | no recording, tests run about 5x faster (the cursor and waits are not slowed down) |
| `--fresh` | restart the app before every test (default: one app per run, reset between tests) |
| `--keep-app` | leave the app running after the run (prints the pid and how to stop it) |
| `--headless` | put the window off-screen (`-32000,-32000`) |
| `--anchor x,y` | where the window opens (default `PM_WINDOW_ANCHOR`, else the left monitor, `-1852,121`) |
| `--run-id <id>` | name of the results folder |

Exit code: 0 all passed, 1 a test failed, 2 could not run.

Results: `e2e-results/<run-id>/<test-id>/{video.mp4,result.json,failure.png,thumb.jpg}` and `e2e-results/<run-id>/run.json` (ignored by git). `result.json` holds the status, duration, the named steps with their time in the video, the error and a screenshot on failure, the test's own log lines and the page's console errors.

Environment: `E2E_CARGO_TARGET`, `E2E_APP_EXE`, `E2E_FAKE_TUI_EXE` (use other binaries), `E2E_RESULTS_DIR`, `E2E_VIEWPORT` (default `1280x800`, also the video size), `E2E_CRF` (video quality, default 28), `E2E_PROFILE` (default `e2e`), `E2E_HEADLESS=1`, `FFMPEG` (path to ffmpeg if it is not on PATH).

## Dashboard

```bash
npm run e2e:dashboard                 # prints http://127.0.0.1:<port>/
npm run e2e:dashboard -- --port 4747
```

Tests grouped by area with search and tag filters, a Run button per test, per area and for everything shown, live log while it runs, the badge, duration, the video of the last run (click a step to jump to it), the screenshot and error of a failure, and the history of past runs. One run at a time; Stop ends it cleanly. It listens on 127.0.0.1 only, refuses other host names, and needs a header on every POST, so a web page elsewhere cannot start runs. HTTP API (also handy from scripts): `GET /api/state`, `GET /api/runs`, `GET /api/runs/<run>`, `GET /api/result/<run>/<test>`, `POST /api/run {only:[ids], video, fresh, headless}`, `POST /api/stop` (send `x-e2e-dashboard: 1`), live events at `GET /api/events` (Server-Sent Events).

## Write a test

1. Copy `tests/_template.test.mjs` to `tests/<area>/<name>.test.mjs`.
2. Change `meta` (`id` is `area.name`, lowercase with dashes; `area` is the folder; add `tags`, `edge: true` for an edge case).
3. Write `run(t)`. See `tests/chat/agent-asks-menu.test.mjs` for a full example.

```js
export const meta = { id: 'chat.send-message', title: 'A message sent from Chat reaches the agent', area: 'chat', tags: ['smoke'] };

export async function run(t) {
  t.fake.script(replyScript('pong'));          // what the fake agent does (pm-fake-tui script, TOML)
  await t.spawnPane('nimbus');                 // start a fake agent pane through the bridge
  await t.step('Open the Chat tab');           // named step: shows in the video and in result.json
  await openChat(t);
  await sendChat(t, 'hello');
  await t.expect.text('pong');                 // waits up to 10 s, no sleeps needed
  t.expect.equal((await t.fake.waitEvent('prompt')).text, 'hello');
}
```

### The `t` object

| | |
|---|---|
| `t.step(name)` | mark a step (video label, log, `result.json`) |
| `t.click(target, {nth, within})`, `t.type(text, {into})`, `t.fill(target, text)`, `t.press('Enter' / 'Ctrl+K')`, `t.waitFor(target)`, `t.waitGone(target)` | the page driver (also under `t.page`; adds `hover`, `rightClick`, `doubleClick`, `value`, `count`, `text()`, `eval(js)`, `reload()`, `screenshot(file)`) |
| `t.expect.text(strOrRegex)`, `noText`, `visible(target)`, `notVisible`, `count(target, n)`, `value(target, v)`, `eventually(fn)`, `equal`, `ok`, `includes`, `match` | assertions; the page ones poll until they hold |
| `t.spawnPane(agent='nimbus', {cwd, script})` | start a fake agent pane and wait until it is idle. Returns a `Pane`: `screen()`, `waitScreen(text)`, `write(text)`, `key(name)`, `info()`, `kill()` |
| `t.panes()` | the app's pane list |
| `t.bridge.get/post/del(path, body)` | the app's HTTP bridge with the run's secret (set up or check state) |
| `t.fake.script(toml)` | the script the next fake agent plays; `t.fake.events()` / `waitEvent('prompt')` read what the fake agent logged (`start`, `ready`, `key`, `prompt`, `approval_shown`, `approval_answer`, `turn_done`, `exit`) |
| `t.project(name)` | a new empty folder to use as a project |
| `t.log(...)`, `t.skip(reason)`, `t.sleep(ms)` | |

**Targets** (first argument of click, waitFor, expect.visible ...): `'Send'` = visible text (substring, case-insensitive; also aria-label, title, placeholder); `'exact=Send'` = whole text; `'re=/^Send/i'` or a RegExp; `'css=.ck-side .ck-row'` = selector (to find a thing, never to check styling); `'aria=Close'`; or an object `{ text, within: 'css=...' | 'some text', nth }`. When several match, buttons and short texts come first; `nth` picks one.

The fake agent plays a `pm-fake-tui` script: see `tools/pm-fake-tui/README.md` (think, say, tool rows, approvals as `[y/n]` or numbered menus with `approvals = "menus"`, writing files, usage limits, crashes, exit codes). **TOML inline tables must stay on one line.**

### Conventions

- **One behaviour per test, independent of the others.** Every test starts from an app with no panes, the default fake script and a freshly loaded cockpit (the runner resets between tests). If a test changes persisted state (settings, UI mode, agent definitions), add `fresh: true` to `meta` so the app restarts first; `seed: { key: value }` starts the app with those settings (the first-run welcome is off by default; `seed: { onboarding_done: null }` turns it on).
- **Fake agents only.** Never a real CLI, never the network. Agent definitions the app should know go in `fixtures/agents/*.toml` (the runner copies the fake agent to each name the definition asks for, approves it, and puts it first on the app's PATH). To add a differently-shaped fake agent, add a definition there and a script with `agent = "nimbus"` layout.
- **Assert what a person sees**: text, buttons, counts, the agent's received input. Not colours, class names or pixel positions. Use `css=` only to find an element.
- **No sleeps.** `waitFor`, `expect.*` and `fake.waitEvent` poll. Wait for a pane to be idle before clicking rows in the sidebar (rows move while panes settle); `spawnPane` already does. With several panes select by name (`selectPane(t, 'Nimbus 2')`), not by position.
- **Name tests after the feature list** (`docs/FEATURES.md`): `area` = the feature area, `title` = what a person would say. Use `step()` for every action worth seeing in the video.
- Keep a test under about 15 seconds; set `timeout` in `meta` (ms, default 60000) when it needs more.

## How it works

- **Isolation.** The launcher starts the app binary with `PUPPET_MASTER_PROFILE=e2e` (its own app id, its own bridge port, never 17321 or 17322), `APPDATA`, `LOCALAPPDATA`, `HOME`, `USERPROFILE` and the WebView2 data folder pointing into `%TEMP%/pm-e2e/<run-id>/`, and a random free debugging port (never 9333). It refuses forbidden ports and the real app's data folder. Only the process tree it started is stopped, also on failure, Ctrl+C or a crash of the runner; a run that was killed hard leaves a record the next run uses to stop the leftover app (by pid, only if it is still a `puppet_master_app`). The run folders under `%TEMP%/pm-e2e` are kept for debugging (delete old ones by hand).
- **Fake agents.** The app gets a declarative agent definition (`fixtures/agents/nimbus.toml`) and copies of `pm-fake-tui` named after it. On Windows the app rebuilds its PATH from the registry, so the dev-only feature `dev-fake-tui` adds `PM_E2E_PATH_PREPEND` (see `src-tauri/src/shell_env.rs`); a normal or release build ignores it. The script a fake agent plays is the file named in `PM_FAKE_TUI_SCRIPT`; `t.fake.script()` rewrites it before each pane starts.
- **The window.** If the app opens its cockpit by itself (newer builds), the launcher uses it. Otherwise it spawns a shell pane through the bridge and detaches it, which opens the cockpit unfocused at the window anchor, then closes that pane.
- **Video.** CDP screencast frames (JPEG) with their timestamps become one H.264 file at 25 fps (ffmpeg). The page gets a visible cursor, click ripples, key caps, the step label (bottom left) and the test id (bottom right) from an overlay that never touches the app's own DOM.
- **Reset between tests.** Close all panes, default script, clear localStorage, navigate to a fresh cockpit page. Roughly 1.5 s.

## Limits

- Windows only for now (WebView2, `taskkill`, ConPTY). The launcher and driver have no other Windows-only code besides process handling.
- The videos show the app's temp folder path (it contains the Windows user name). They stay local (ignored by git); do not publish them without a look.
- The window opens on the monitor named by the anchor; it is never focused, but a window on screen can still be seen. `--headless` moves it off-screen.
- The fake agent has Claude-, Codex- and nimbus-shaped screens in the harness; the GUI runs the nimbus layout through a declarative definition. Tests of Claude- or Codex-specific screens need a matching definition (the harness in `src-tauri/tests/fake_tui_*.rs` already covers them without a UI).
