# Night Shift (Preview)

Night Shift works through a list of tasks while you sleep. Before bed you write up to 10 tasks in the **Tonight** sheet,
pick which plans (agent logins) it may use and a stop time. Puppet Master runs the tasks one after another on this computer,
each in its own separate copy of the project, and runs your tests in that copy. In the morning the **Morning** page shows
what each task did. Nothing is merged until you press **Keep**.

It is off by default. Turn it on in **Settings > General > Labs > Night Shift** (the setting is `night_shift_enabled`).
While the switch is off, the `+` menu item `Tonight…` and the palette commands are not drawn, and Start refuses with
`A person can turn Night Shift on in Settings > General > Labs.`

## What a night does

1. **Tonight** (`+` menu, or the palette command `Night Shift: plan tonight`). Each task has a project, the words of the task,
   an agent (`Any with usage left` or one plan), the project's tests, and under `More` whether agents may change the tests
   (off), the turns (12) and the minutes (90). The sheet also has the plans to use, a stop time (default 07:00), Start now or
   at a time, how many tasks at once (1 or 2), and a switch to keep the computer awake. It prints the worst case
   (`3 tasks x 12 turns = 36 turns, at most 2 at a time, until 07:00`). A disabled Start says why in one line.
2. **Tests are declared by you.** Per project, in the Tests popover: a command such as `npm test`, saved in the app data
   folder (not in the project). Puppet Master may suggest what it finds (for example `npm test` in `package.json`), but
   nothing is used until you press Use. With no declared tests the task still runs; the Morning page says
   `Done. No tests declared, nothing was checked.` and never says "tests pass".
3. **A task is a mission.** With declared tests a task runs as a mission (one agent, Undo on, the test-change guard on, quiet):
   the agent works, the tests run in the copy after each turn, and a turn that makes things worse is rolled back. Without
   declared tests it runs as one turn in a separate copy.
4. **Order and limits.** The list is the priority order; tasks are never reordered, only skipped while their project or plan is
   busy. At most 2 tasks at once, one per plan and one per project, and never a mission slot a day-time mission holds. No new
   try starts when less than 25 minutes are left. Each task gets at most 2 tries.
5. **Usage limits.** When an agent stops at a usage limit, Puppet Master waits 3 minutes. If the agent will continue by itself
   after the reset and that fits before the stop time, it waits in place. Otherwise it ends that try, marks the plan out
   until its reset (one hour when no reset time was shown) and runs the task again, once, on a plan of a different company. It
   never rotates between two logins of one company. The first try's copy is kept as an earlier try.
6. **Stop time.** At the stop time every running task is stopped and shown as `Cut off at 07:00 while working`; tasks that
   never started say `Didn't run: no time left before 07:00`. Copies stay. **Stop Night Shift** (chip, or the phone) does the
   same at once.
7. **Keep awake.** Asked once, in the sheet (`Puppet Master will stop this computer from sleeping until Night Shift ends, then
   let it sleep again…`). Released when the night ends, when you press Stop, and when the app exits. Closing the lid or
   pressing the power button still sleeps the computer; the page says when the computer slept.
8. **After a restart** the app takes over an unfinished night when it starts (also with `--background`); missions resume
   themselves, and a task that had no mission yet is queued again.
9. **Quiet.** While a night runs, its panes and missions raise no Inbox card, no push and no sound. The one exception is a
   single morning push at the stop time (or when the last task ends, whichever is later) with counts only, for example
   `4 of 5 done, tests green. 1 needs you.` It is not sent if you already opened the Morning page, and it never carries task
   text, project names or paths. Phones set to hide details get `Night Shift is ready`.

## What you see

- **The chip** at the bottom of the sidebar, while a night exists: `Night Shift  3 of 5 until 07:00` with a spinner (running),
  `Waiting for Claude Code until 01:10` (every plan out of usage), `Starts 23:30` (scheduled), a count of tasks that need you,
  and `4 of 5 done · Open Morning` at the end. Clicking opens a small list of tasks (running) or the Morning page (finished).
  `Stop Night Shift` asks `Stops everything now. Copies are kept.`
- **The Morning page** (a full page opened from the chip or the palette). The summary line, then groups: `Ready to keep`,
  `Needs a look`, `Didn't run`, and the night's event line. A row opens to show what the agent did (turn sentences, redacted),
  the files with `+/-`, each declared check with its counts, any test changes, and every try. The history switcher shows the
  last 14 nights (older ones are read only). **Save as page** writes one self-contained HTML file (redacted, no paths).
  Opening the page counts as "seen" and cancels the held morning push.
- **Keep** (press twice: `Merge into main?`) merges the task's copy into the project's current branch with the same merge the
  copies use everywhere. If it conflicts with something you already kept, the merge is aborted, the project and the copy stay
  as they were, and the row says `This conflicts with a task you already kept (cart.js). Throw it away, or merge it yourself
  from its branch …`. **Throw away** (press twice: `Delete this copy?`) deletes every copy the task made, earlier tries
  included. **Keep all that pass** keeps the ready tasks in list order and stops at the first one that does not merge.
- **A task that needs a look** says why in plain words: `Codex asked to trust the folder.`, `Stopped to ask a question`,
  `Tests still fail (9 of 12, was 7 of 12)`, `The checks pass, but the tests were changed`, `Ran out of usage`, `Cut off at
  07:00 while working`. A green run whose tests were changed is never `Ready`.
- **The phone** (paired, with the `watch` power): `GET /night` returns a redacted copy of the page (no project names, no pane
  ids, no turn sentences, no agent questions, titles cut), and `POST /night/stop` (needs `control`) stops the night. Keep and
  Throw away are not on the phone. The phone card that draws this (NS-20) is not in this build yet; the routes are.

## Safety

- **Never answers a trust, permission or sign-in screen.** Night Shift has no way to type into a pane: its adapter can start a
  task, read its state and stop it. A trust or permission screen, a sign-in or an agent's question ends that try as
  `Needs a look`; the pane stays open showing the question. The same plan is not tried again in that folder tonight (another
  plan gets one fresh try when the task allows any plan). The end-to-end test reads the fake agent's flight log and fails on
  any `gate_answer` or text typed at a gate.
- **Copies only.** Every task works in its own separate copy of the project. Your project's files are not touched during the
  night. A stop never deletes a copy.
- **Merges only on your Keep.** `Keep`, `Throw away` and Start are desktop commands. No MCP tool, no bridge route and no phone
  path reaches them: the bridge route file does not use the review code, and the runner's source is scanned for any call that types into a pane.
- **"Tests pass" means the tests you declared**, run from the snapshot taken at Start, with the test-change guard on. An agent
  that skips or weakens tests does not turn a task green.
- **Privacy.** The run record, the declared tests and the history live in `<app data>/night-shift/`. Text that can leave that
  folder (the page, the saved page, the phone view, the push) is redacted and cut first.
- **Bounded.** At most 10 tasks, 2 tries each, 2 at once; per task 3 to 40 turns and 20 to 240 minutes; a stop time 30 minutes
  to 14 hours after the start. One night at a time.

## Where it lives

| Part | Files (under `packages/app/src-tauri/src` unless noted) |
|---|---|
| Engine | `night_shift/`: `model` (types, limits), `store` (`run.json`, `history/`, `settings.json`, `projects.json`), `checks` (declared tests), `plan` (who runs next, the limit decision), `classify` + `report` (results and the exact words), `runner` (the supervisor, one tick per 5 s), `io` + `io_live` + `io_start` + `io_facts` (the adapter over missions, `run_agent` and the copies), `review` (Keep, Throw away), `view` + `page` (what the UI, the phone and the saved page read), `power` (keep awake), `quiet` (silent panes and runs) |
| Seams in other modules | missions: `Starter::Night` and `quiet`; budgets and pushes check the quiet set; `push/night.rs` (one push kind); the audit kind `night_shift`; the Labs key `night_shift_enabled` |
| Commands | `commands/night.rs`, `commands/night/start.rs` (everything Start checks): `night_state`, `night_plans`, `night_checks_get/set`, `night_start`, `night_stop`, `night_keep`, `night_throw_away`, `night_keep_all_ready`, `night_get`, `night_seen`, `night_report_html/save`, `night_settings_get/set` |
| Bridge | `bridge/routes/night.rs`: `GET /night`, `POST /night/stop` |
| UI (`packages/app/src`) | `components/cockpit/night/` (`TonightSheet`, `TestsPopover`, `PlansPicker`, `TonightHost`, `NightChip`, `MorningPage`, `TaskRow`), `lib/night.ts`, `lib/night-host.ts`, `styles/night*.css`, `dev/night-fixtures.ts` |

## Tests

- **Unit:** `cargo test --lib night` (the runner runs against a fake adapter and a fake clock; classify, plan, store, checks,
  review, view, page, report, quiet and the start checks have their own).
- **End to end, `pm-fake-tui`:** `tests/fake_tui_night.rs`, included from `tests/fake_tui_e2e.rs`. Zero tokens: the real Start
  checks, runner, mission engine, checks (`node run.js`), copies, Keep and Throw away; only the two agents (`fake_claude`,
  `fake_codex`, two companies) are fake, and the night's clock is the test's (`NightRig::advance_min`). Three tests:
  1. four tasks: two finish green; one hits a usage limit that is not armed, is ended after the 3-minute grace and finishes
     green on the other plan; one pinned to a plan that shows its trust screen ends as `Needs a look` and nothing is ever typed
     at it (no `gate_answer`, no `typed_at_gate` in the flight log);
  2. a stop time 6 minutes away: the task queued behind a long task is told `Didn't run`, the long task is cut off at the stop
     time and its copy is kept with the files written so far;
  3. Keep merges a copy into the project, Throw away removes a copy, and a second Keep that edits the same line conflicts and
     leaves the project and its copy as they were.
  Run: `cargo test --features dev-fake-tui --test fake_tui_e2e night -- --test-threads=1`.
- **vitest:** the files next to each component in `components/cockpit/night/`, and `lib/night*.test.ts`.

## Limits (v1)

- Real agents are not covered by the end-to-end test: the limit and trust rules for real Claude Code and Codex screens come
  from their profiles and have not had a live night.
- No Keep Going at a limit (a task never continues in the same copy on another plan), no backup agents inside a night task, no
  recurring nights, no cross-project budget, no baseline run of the tests at Start, no phone Keep.
- Tasks are cut from the project's commit at Start. Two kept tasks that edit the same lines conflict; the second says so.
- The sheet has no "start when I sign in" switch; the app takes over an unfinished night when it starts.
