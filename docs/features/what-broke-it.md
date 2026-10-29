# What broke it ("What broke the tests?")

Branch `feat/wb-phase5`. Finds the first agent turn after which the person's own test command fails, names the turn and
the agent, and can undo only that turn. Spec: `SPEC-n33-what-broke-it` (section 3 and 4). Code map:
[`REPO_MAP.md`](../REPO_MAP.md) section 3.16.

**Status: experimental, not live-checked yet.** The whole feature is on this branch: the engine, the saved results, the
scores count, the sheet and result card, the Tauri commands (`wb_*`, WB-16), the History button and palette item and the
sheet mount (WB-17), and the end-to-end test (WB-18). Each part has tests. Nobody has run the feature in the running app
yet. The end-to-end test is the only run through the real engine; it uses a fake pane list and fake agent turns.

## 1. What and why

- **Story:** "The tests passed yesterday and fail now. Which agent turn broke them?" The person declares one test
  command for a folder. What broke it runs that command in a throw-away copy, narrows the range of save points by
  halving, and names the turn. Undo only this turn puts back that turn's own lines and nothing else.
- **Plain words only** in the UI: "save point", "turn", "undo only this turn", "separate copy". The strings live in
  `checkpoints/bisect/words.rs`, and a test rejects the words `bisect`, `commit`, `ref`, `tree`, `HEAD`, `sentinel`.
- **Agent-agnostic:** generic code never names an agent. The agent label and the sentence come from the turn summary
  of that turn. No summary means the label "An agent".

## 2. Flow (what the code does)

Steps follow `checkpoints/bisect/job.rs` (`search`) and `live.rs` (`Live::start`).

1. **Start checks.** Checkpoints on, git installed, folder exists, one job at a time (`WB_BUSY`), at least one declared
   test for the folder (`WB_NO_TEST`). Limits are clamped: runs 1 to 30 (default 16), minutes 1 to 60 (default 20).
2. **"Now".** One save point is taken of the folder as it is now (pane key `wb-…`, note "What broke it: now").
3. **Timeline.** All `after` save points of the folder (every pane) are sorted in time order, equal states merged, and
   the newest 100 kept (`points.rs`). Fewer than 2 points stops with `WB_TOO_FEW`.
4. **Separate copy.** The save points are written into `<app data>/what-broke-it/copies/<8 hex>/` from the shadow
   repository, never from the person's folder. The project's setup steps run once in the copy. A failing setup stops
   with `WB_SETUP_FAILED` and the redacted end of its output.
5. **Search** (`search.rs`, pure). Test "now" first; if it passes the answer is `TestsPass`. Then find a good point:
   the one the person picked, or walk back 1, 2, 4, ... points until one passes. Halve between good and bad until they
   are neighbours. Run the good point once more: a different answer is `NotSure(flaky)`.
6. **Each run.** The declared argv runs in the copy (never a shell string), with stdin closed, `CI=1`, `GIT_*`,
   `PUPPET_MASTER*` and `PM_*` variables removed, and below-normal priority on Windows. Timeout is the test's own
   minutes, cut to what is left of the job's minutes (at least 1 second). A failing run is run a second time; two
   different answers mean "not sure", never a guess (S7).
7. **Blame** (`blame.rs`). The turns whose time range overlaps the gap between last good and first bad point:
   - one turn: `Culprit`. If that turn started from files other than the good point, its starting files are tested
     too. If they already fail, the turn is not blamed (`BetweenTurns`);
   - two or more turns (two panes at once): `Several`. No Undo;
   - none: `BetweenTurns` ("something outside the agents' turns changed it").
8. **Result saved** to `findings.json` (newest first, 100 kept). Kept with it: the last 40 lines of the first failing
   run (redacted) and up to 5 failing test names.

Possible results (`Outcome`): `Culprit`, `Several`, `BetweenTurns`, `TestsPass`, `AlwaysRed` (every point fails),
`GoodWasRed` (the point the person picked fails), `NotSure` (reasons: flaky, timed out, the program was not found,
setup failed, stopped, out of time, out of runs), `CopyDiffers` (a link in a save point was refused) and `Failed` (a
code, see section 6).

**Stop** sets a flag. The run's process tree is killed by PID (never by image name) and the search ends with the reason
"stopped" (`NotSure`).

**Keep looking.** For `NotSure` with "out of runs" or "out of time" only: the same search again, reusing the answers
already measured (the cache is saved with the finding), with a fresh budget.

**Check it again.** After an undo: one run of the declared command on the folder as it is now (at least 2 runs
allowed). Passes gives `TestsPass`; fails gives `WB_STILL_FAILING`.

## 3. Undo only this turn

- **Dry run first** (`undo_plan`, writes nothing). For every path the turn changed, the code compares the bytes before
  the turn (B), after it (A) and on disk now (C) (`revert.rs`, `decide`). Text files are merged with `git merge-file`,
  so later edits on other lines stay.
- **Refused, nothing written:**
  - any agent in the folder is running or has an open run (`WB_UNDO_RUNNING`);
  - a path that the turn and later work both changed on the same lines, a deleted-later file, a file created and then
    edited, a binary or unreadable file, a link, or a file over 20 MiB (`WB_UNDO_CONFLICT`, the file name is shown);
  - the saved turn's save points are gone (`WB_UNDO_GONE`); an undone finding cannot be undone again
    (`WB_NOT_UNDOABLE`).
- **All or nothing.** One conflict means the folder is not touched. The plan is re-made at undo time; an old plan is
  never trusted.
- **Safety save point first.** Before the first write, a save point "What broke it: before undo" is taken (pane key
  `wb-undo`). It is a normal save point of the folder; the search timeline ignores `wb-` save points.
- **Audit.** One line in the audit log (kind `what_broke_it.undone`, local user): "Undid turn N of <agent label> in
  <folder name>". Only the folder's last name, never its path.

## 4. Invariants (guards)

From `checkpoints/bisect/mod.rs`. Every file of the feature keeps them.

| Guard | Rule |
|---|---|
| S1 | Tests run only in a copy under `<app data>/what-broke-it/copies/`, never in the person's folder |
| S2 | Only the desktop window starts a search or an undo. No bridge route, MCP tool, companion or phone path |
| S3 | Only the person's declared command runs, as argv |
| S5, S6 | One job at a time; every run has a timeout and a Stop; only this job's PIDs are killed |
| S7 | A failing run is run twice; a different answer is "not sure" |
| S8 | Undo is all-or-nothing, refuses while an agent in the folder works, and a safety save point comes first |
| S9, S10 | Output is redacted before it is stored (secrets masked, e-mail addresses masked, paths cut down); only one failing run's tail is kept |
| S11 | Ids are checked (`wb-` plus 12 lowercase hex) before they touch a path |
| S13 | Generic code names no agent |
| S14 | Nothing is typed into any pane |

## 5. Limits

| What | Value |
|---|---|
| Runs per job | default 16, range 1 to 30. In practice a narrowed range needs at least 4 runs: a probe reserves 2 runs for its confirm run. The end-to-end run-cap scenario uses 4 |
| Minutes per job | default 20, range 1 to 60 |
| Save points searched | newest 100 |
| Copy size | refused above 1.5 GiB |
| Undo: file size | over 20 MiB is a conflict, never read |
| Undo: diff shown | cut at 100 KiB |
| Test output kept | last 40 lines of the first failing run, redacted |
| Failing test names | at most 5, each at most 120 characters |
| Saved results | newest 100 per app |
| Jobs kept in memory for `status` | last 20 |
| Old copies | `sweep_stale` removes copies older than 1 hour that this process does not hold. It runs once at app start (desktop build only, not in tests; `lib.rs`, WB-16) |

## 6. Error codes

Plain strings from the backend; the window shows the words, never a raw path.

| Code | Meaning |
|---|---|
| `WB_BUSY` | another search is running |
| `WB_NO_TEST` | no declared test for the folder, or the command is refused (for example it has shell operators) |
| `WB_TEST_NOT_FOUND` | the program named by the test was not found |
| `WB_SETUP_FAILED` | the project's setup step failed in the copy |
| `WB_TOO_BIG` | the copy or a save point is too big |
| `WB_TOO_FEW` | fewer than 2 save points |
| `WB_BAD_ID` | an id or a save point picked by the person does not exist |
| `WB_UNDO_CONFLICT` | a file cannot be put back cleanly; nothing was written |
| `WB_UNDO_RUNNING` | an agent in the folder is working |
| `WB_UNDO_GONE` | the turn's save points were removed |
| `WB_NOT_UNDOABLE` | not a culprit, or already undone |
| `WB_STILL_FAILING` | "Check it again" still fails after the undo |
| `WB_UNAVAILABLE` | checkpoints off, git missing, folder gone, or a save point could not be made |
| `WB_INTERNAL` | a search stopped by an error nobody planned for |

## 7. Scores: "Broke the tests N times" (known limit)

The Scores page (`scores/gather.rs`, `scores/aggregate.rs::count_broke`) adds one number per agent row: how many
saved findings blamed a turn of that agent and account, in this project and in the window. It is shown under the agent
name only when the number is above zero. It never changes the ranking, `ahead`, any rate or `enough`.

**Known limit:** the count still counts a culprit the person later undid. The undo sets `undone_ms` on the finding,
but the Scores code does not read it. So a turn that was blamed, undone, and then the tests still failed for another
reason, is counted. The count shows "the search blamed this agent", not "this agent's turn is still broken". Fixing it
(skip findings with `undone_ms`, or count them apart) is a v2 item.

Only `Culprit` findings with a known agent id count. A finding without an agent (the turn summary was gone) is never
counted.

## 8. What is not done

- A live check in the running app (test plan, section 2.14).
- The Scores count excludes undone culprits (section 7).
- Undo for `Several` (no single turn to undo) and for more than one turn at once.
- No phone or MCP path, by design (S2).

## 9. Tests

- Unit: `cargo test --lib checkpoints::bisect` (the engine loop with a fake `LiveIo`, search, blame, points, redaction,
  the undo cases), `cargo test --lib scores` (the count), `cargo test --lib commands::what_broke_it` (the `wb_*`
  commands: error shape, id checks, limits), and
  `npx vitest run src/components/cockpit/WhatBrokeIt src/components/cockpit/Palette.whatbroke.test.tsx src/lib/what-broke-it src/lib/agent-scores`.
  The History button is tested in `History.test.tsx`.
- End to end: `cargo test --features dev-fake-tui --test what_broke_it_e2e -- --test-threads=1` in
  `packages/app/src-tauri`. Windows only (the file is `cfg(windows)`). It needs `node`; without it each scenario prints a
  note and passes. The feature roots app data in a temp folder, so the real app data is not touched. Six scenarios:
  1. culprit named: turn 4 of the second agent (`pane-claude`), found by running the declared `node check.mjs` in a copy;
  2. undo only that turn keeps the later lines, and the recheck passes;
  3. a later edit to the same line refuses the undo (`WB_UNDO_CONFLICT`);
  4. a flaky check is "not sure", never a culprit;
  5. a run cap of 4 gives a narrowed range, and "Keep looking" finds the turn;
  6. Stop ends a long run in under 3 seconds and kills only its own process.
  The test writes the agents' files, save points and turn summaries itself (the runtime writes them in the app), so it
  does not prove the live agent hooks.
