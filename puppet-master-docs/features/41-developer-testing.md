# Developer testing (fake agent, replay corpus, UI harness)

**Status:** Works today. This is for developers only. Nothing here is part of the app that users install. Windows only for the fake-agent tests.

## What it is

Real agents cost money and are slow and not predictable. Puppet Master (PM) has four tools to test without them:

1. **The fake agent** (`pm-fake-tui`). A small program that looks like Claude Code or Codex in a real terminal. It plays a script. It costs no tokens and behaves the same every time.
2. **The end-to-end tests.** They use the fake agent with PM's real code: real terminal, real `run_agent`, real approvals, real receipts.
3. **The replay corpus.** Recordings of real agent screens from old bugs, replayed in tests.
4. **The UI harness.** The real screens of the app in a plain browser, with made-up data, for tests and screenshots.

## Why it matters

Each live bug becomes a failing test first, and then it is fixed. The fix must make the test pass, and removing the fix must make it fail again. This keeps old bugs from returning. A vendor quirk (a menu that opens 600 ms late, an Enter key lost in a paste) is written down once and kept.

## How to use it

Run the normal tests:

1. `cd packages/app/src-tauri` and run `cargo test --lib` (2510 tests in 0.2.1) and `cargo test --bins` (47 tests).
2. From the repo root run `npm test` (the front-end tests, 1876 in 0.2.1) and `npm run typecheck`.

Run the fake-agent end-to-end tests (needs Windows and a free cargo target folder):

1. From the repo root run `npm run test:fake-tui`. It runs `cargo test --features dev-fake-tui --test fake_tui_e2e -- --test-threads=1`.
2. Add `-- --ignored` to run the tests that are marked as to-do.
3. The 0.2.1 run had 27 tests: 22 in the main file (`npm run test:fake-tui`, run 3 times) and 5 for Missions (`--test fake_tui_missions`). They all passed.
4. To try the fake agent alone (build it first with `cargo build` in `tools/pm-fake-tui`): `pm-fake-tui --agent claude --script my-script.toml`. Check a script with `--check FILE`. Set `PM_FAKE_TUI_LOG=<file>` to get one JSON line per event.

Write a fake-agent script (TOML or JSON): choose `agent = "claude"` or `"codex"`, then add `[[turn]]` blocks with steps such as `think`, `say`, `approve` (an approval card), `write_file`, `limit` (a usage-limit message), `crash` and `exit`.

Add a replay test from a live bug:

1. Capture the screens with their times. Keep the real text.
2. Build a cast file: `node tests/corpus/tools/screens-to-cast.mjs spec.json tests/corpus/<agent>/<version>/<name>.cast`.
3. Add a test in `src/replay/corpus_tests.rs`. Assert the **correct** result.
4. Run `INSTA_UPDATE=always cargo test --lib replay`, read the new snapshot, and commit it.
5. Revert the fix on purpose and check that the test fails. Then restore the fix.

Use the UI harness:

1. Run `npm run dev` and open the app in a browser with `?harness=<scenario>` (dev builds only).
2. For screenshots, run `node scripts/evidence/screenshot.mjs --out <folder> <scenario> [more scenarios]`. It uses headless Microsoft Edge, so it opens no window.
3. Scenarios are files in `packages/app/src/dev/scenarios/` (about 260). The phone has its own route, `phone`, with a fake bridge.

## How it works (simple)

- The fake agent draws the screens exactly as captured from Claude Code 2.1.299 and Codex 0.162.1. The tests read each screen back through PM's real agent profile, so if PM stops recognising a screen, a test fails.
- The end-to-end harness runs the app code without a window, with its own profile `fake-tui-e2e`, so a running Puppet Master is never touched. Test agent types are `fake_claude` and `fake_codex`.
- A replay runs a recorded cast through the same code that decides things, in **virtual time** (no waiting). `insta` stores a snapshot of what PM decided. A change of behaviour shows up as a readable diff.
- The corpus today covers Claude Code 2.1.299 and 2.1.300 and Codex 0.159.3.
- In the harness, `blockNetwork` stops all real network calls, and the mock layer answers from fixtures.

## Limits and what is not done yet

- The fake agent proves that PM handles the screens it knows. It does not prove that a new agent version behaves the same.
- The fake-agent tests are sensitive to timing on a busy PC and can fail by chance. They are not part of the normal `cargo test --lib` run. Run them alone.
- Windows only: they need ConPTY. Other systems get a best-effort fallback.
- Harness screenshots show example data. Some show a made-up user name `dev` in paths.
- The screenshot script expects Edge at its usual path on Windows.
- Live tests with real agents (`node scripts/pm-regression.mjs`) cost money. Use `--dry-run` to see the plan first.
- After you change the tool catalog or the MCP program, build a new MCP exe and reconnect agents. See `docs/wiki/developing.md`.

## For developers

- Fake agent: `tools/pm-fake-tui/` (`script.rs`, `engine.rs`, `render.rs`, `runtime.rs`; README has the full script format).
- App side: `packages/app/src-tauri/src/fake_tui/` (behind the `dev-fake-tui` cargo feature). Tests: `tests/fake_tui_e2e.rs`, `tests/fake_tui_missions.rs`.
- Replay: `src/replay/` (`cast.rs`, `pane.rs`, `startup.rs`, `receipt.rs`, `corpus_tests.rs`), corpus in `tests/corpus/`, snapshots in `tests/corpus/snapshots/`.
- Harness: `packages/app/src/dev/` (`harness.tsx`, `mock-tauri.ts`, `mock-bridge.ts`, `fixtures.ts`, `scenarios/`), `scripts/evidence/screenshot.mjs`.
- Other scripts: `npm run test:mcp-operations`, `npm run test:mcp-package`.

## Related

- [Agents controlling agents (MCP)](32-agents-controlling-agents-mcp.md)
- [Phone terminal](38-phone-terminal.md)
- [Phone approval card](37-phone-approval-card.md)
