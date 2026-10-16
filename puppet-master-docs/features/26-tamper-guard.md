# Tamper guard

**Status:** Preview (Labs, off by default). The engine is built and tested by automated tests. Its only user today is Missions, which is a Labs feature.

## What it is
The tamper guard finds out when an agent made tests pass by weakening the tests. For example, the agent adds a "skip" to a failing test, deletes it, or changes a check so that it is easier to pass. The guard takes a snapshot of the test files and the check settings at the start. Later it compares the project with the snapshot and describes what changed, in plain words. It reads files only. It uses no AI and no scores. It follows fixed rules, so the same input gives the same answer.

## Why it matters
"All tests pass" is only good news if the tests are still the same tests. An agent that wants to finish can take a shortcut. This is a real risk when you leave an agent alone to "make the tests green". The guard stops a fake green and asks you.

## How to use it
Today you do not use the guard on its own. [Missions](30-missions.md) uses it. To see it:
1. Open **Settings**, then **General**, then **Labs**, and turn on **Missions (Preview)**.
2. In the sidebar, click **+**, then **Start a mission…**.
3. Fill in the goal and the checks. The guard needs no setup. It takes its snapshot when the mission starts.
4. The switch **Agents may change tests** is off by default. Leave it off to keep the guard strict. If you turn it on, the findings are only notes and do not stop the mission.
5. When all checks pass, the guard looks at the test files. If it finds a problem, the mission stops. A card appears in the Inbox: **The checks pass, but the tests were changed**. It lists each change in a short line.
6. Choose:
   - **Undo the test changes and keep going**: Puppet Master puts the test files back and the agent works again.
   - **Accept the changes**: a sheet asks "Count these tests as passing?". Click **Accept and finish** to finish with the changed tests.

## How it works (simple)
- At the start it finds the protected files: test files, and the settings that decide how tests run (for example, a test script or a runner config). It saves a snapshot in `<project>/.puppet-master/tamper/`.
- Later it compares and checks 11 kinds of change:
  1. A skip was added.
  2. A focus marker was added (only run this test), or a name filter was added to the test command.
  3. Tests were removed, or a test file was deleted or moved away.
  4. Fewer assertions than at the start.
  5. An assertion became weaker.
  6. A snapshot file was rewritten or deleted.
  7. How tests run was changed.
  8. The test runner reported fewer tests than at the start.
  9. A marker that hides errors was added (like `@ts-ignore` or `eslint-disable`).
  10. A test file changed in some other way. This is a note.
  11. A changed file is in a language the guard has no patterns for. This is a note.
- Each finding is "blocks" (stops the mission) or "note" (only shown).
- Rules for languages are in files: JavaScript and TypeScript, Python, Rust and Go, plus a common set and a generic one.
- The guard is checked by 51 test fixtures. Each fixture is a small project before and after a change.
- Undo restores only the files the caller names.

## Limits and what is not done yet
- It is a Preview feature. It has not had long use on real projects. The word "tamper" does not mean the agent did something bad on purpose. It only means the tests changed.
- It can only check languages it has rules for. For others it says "couldn't check" as a note.
- No AI is used. A clever change that looks like normal code may not be found.
- There is no screen for the guard outside Missions. The only other entry is a read-only HTTP route (see below).
- Phone: the tamper card reaches your phone through the attention list. Missions has no phone view yet.
- Since 0.2.1 it has a user (Missions). Before that, nothing called it.

## For developers
- Module: `packages/app/src-tauri/src/tamper/` (`snapshot.rs`, `signals.rs`, `protect.rs`, `lexer.rs`, `facts.rs`, `config.rs`, `undo.rs`, `store.rs`). Language rules: `tamper/rules/*.toml`. Fixtures: `tamper/fixtures/` (51).
- API: `tamper::take`, `save`, `check`, `undo`. Callers: `missions/engine.rs` and `missions/verdict.rs`.
- Read-only route: `GET /tamper/check?project=<folder>&since=<snapshot>[&total=<n>&skipped=<n>]` (`bridge/routes/tamper.rs`). It needs a saved snapshot, so it cannot read any other folder. A phone with `watch` access may call it.
- Tauri command: `commands/tamper.rs`.
- Mission field: `may_change_tests` (default false). Card choices: `undo_tests`, `accept_tests`.

## Related
- [Missions](30-missions.md)
- [Separate copies](22-separate-copies-worktrees.md)
- [Second opinion](31-second-opinion.md)
