# Red to Green: the same agent fixes failed CI on its pull request

Code: `packages/app/src-tauri/src/ci_fix/` (the rules, the GitHub reads, the log cleaning, the push check extras, the store,
the engine and its flows). The shared pieces it reuses are in `worktrees/` (copy git, `pr::push_update`, the fix push in
`service::pr_push_fix`, the `on_pr_opened` hook), `issue_intake/` (the push check base and the text helpers), `questions/`
(the cards), `audit/` and `pty/env_guard.rs` (the no-push guard). In the UI: `components/settings/blocks/CiFixSettings.tsx`,
`components/cockpit/CiFixCard.tsx` (+ `styles/ci-fix-card.css`), `lib/ci-fix.ts`, and the two buttons on the pull request
block in `components/cockpit/PrBlock.tsx`. The desktop commands are in `commands/ci_fix.rs`, registered in `lib.rs`. The file
map is in `docs/REPO_MAP.md` section 3.14. Spec: n33 (Red to Green). In code the feature is `ci_fix`; in the UI and in these
docs it is **Red to Green**.

Status: **Labs, experimental**, in Settings > Labs. Wired into the app on this branch (see "Status on this branch"). Not
live-checked yet: no run against real GitHub or with a real agent is recorded.

## What and why

1. Puppet Master opened a draft pull request from one of its own copies (Assign an issue, or Changes > Create PR).
   GitHub runs CI on it.
2. CI fails. Puppet Master sees it through your own `gh` sign-in (a poll, no webhook, no open port). It asks one card:
   `CI failed on #61 (lint, test). Ask <agent> to fix it?`
3. If you say yes, the same agent gets a short task. The task points at a file in the copy with the failing checks and the
   end of their logs. The agent works in the same copy, in a pane that cannot push.
4. When the agent is done, Puppet Master checks the change. Then it asks `Push the fix to #61?`. On yes, it pushes the new
   commits to the same `pm/` branch. It never forces a push and never re-runs CI.
5. CI runs again. Green: the fix is marked green. Red again: one more card, while tries are left.

Puppet Master never merges, never marks a pull request ready, never re-runs a check, and never comments on GitHub.

## Status on this branch

What is on this branch (`feat/rg-phase5`, the finished Red to Green work through RG-12 and UI-4):

- The store, the settings and the caps (`model.rs`, `store.rs`), the GitHub reads and parsers (`github.rs`), the ownership
  and offer rules (`filter.rs`), the log cleaning (`logs.rs`), the texts (`text.rs`), the push check extras (`fixcheck.rs`).
- The live engine and its flows (`live.rs`, `engine.rs`, `flows.rs`, `answers.rs`, `view.rs`), tested with a fake GitHub,
  a fake agent and a virtual clock (`engine_tests.rs`).
- The watcher thread starts in `lib.rs` setup (`ci_fix::live::start`). It looks at GitHub only while the Labs switch is on.
- The six desktop commands (`ci_fix_view`, `ci_fix_save_settings`, `ci_fix_offer`, `ci_fix_act`, `ci_fix_log`,
  `ci_fix_poll_now`) are in `commands/ci_fix.rs` and registered in `lib.rs` (RG-10). The Labs block calls them.
- The hook in `worktrees/service.rs` (`pr_create_with` calls `ci_fix::live::on_pr_opened`), the question and audit wiring,
  and the Labs block with its card (`CiFixSettings.tsx`, `CiFixCard.tsx`).
- The pull request block (UI-4): **Fix with <agent>** when the checks failed, **Watch CI** otherwise. Both call
  `ci_fix_offer`, which only starts the watch (and raises the card now if the head already failed). No fix starts from them.
- The old "Send back to <agent>" path is removed (RG-11): no command, no button. Only `worktrees/sendback.rs::pane_if_guarded`
  remains. The engine uses it to check that a pane is guarded and inside the copy before it gets anything.
- An end-to-end test with a fake `gh`, a local bare remote, real git and a fake agent in a real pane (RG-12): six scenarios in
  `tests/fake_tui_ci_fix.rs`, run through `fake_tui_e2e`. It found one engine fault, now fixed: our own fix push can land
  while a look is under way. That look now reads GitHub again and does not treat the new commit as someone else's push.

What is still missing:

- No live run against real GitHub, and no run with a real agent. Everything is tested with fakes. Keep the Labs status
  until one is recorded.
- The test counts in this page and in `docs/TEST-PLAN.md` are counted from source. They were not re-run in the docs pass.

## Setup

Puppet Master uses your own GitHub CLI (`gh`) sign-in. It never asks for a token and never stores one.

1. Install the GitHub CLI (`gh`). Sign in once in a terminal with `gh auth login`.
2. Open Settings > Labs > **Red to Green**. Turn on **Offer to fix failed CI**. The line under it says
   `Signed in to GitHub as @you (gh)`. If `gh` is missing or signed out it says
   `Install the GitHub CLI and run gh auth login. Nothing is checked until then.` and nothing polls.
3. Set the numbers (defaults in brackets):
   - **Tries per pull request**: 1 to 3 [2].
   - **Turns per try**: 1 to 50 [15]. Used for a new pane only.
   - **Minutes per try**: 5 to 60 [20]. Used for a new pane only.
   - **Fixes a day**: 1 to 20 [6], across all pull requests.

The switch is off by default. Numbers are clamped into their ranges when saved and when read.

## Which pull requests

Only pull requests Puppet Master opened from its own copy: a copy on a `pm/...` branch that has a stored pull request.
A watch is started when Puppet Master opens one, while the switch is on (the hook is in `pr_create_with`). A pull request
that was opened before the switch was on, or that is not watched for another reason, can be started by hand from its PR
block: **Watch CI** when the checks do not fail, **Fix with <agent>** when they did. Both only ask Red to Green to watch
the copy's pull request (and, when the head already failed, to raise the fix card now). Neither one starts a fix: the
person still says yes on the card. The rules below apply to both.

Never watched, never offered: fork pull requests, pull requests by someone else, branches made outside Puppet Master,
and a head that someone else pushed to. Every poll checks GitHub again (`filter::owns`):

- the pull request is open and not merged;
- the head repository is the base repository (not a fork);
- the author is your `gh` login;
- the base repository is the watch's `owner/repo`;
- the head branch is the copy's `pm/` branch, is not the base, and is not `main`, `master`, `trunk` or `develop`;
- the head commit is one Puppet Master pushed (kept in `pushed_shas`, the last 20).

A merged pull request ends the watch as Merged, and a closed one as Closed. Any other mismatch, or a read error that is
not a retry, pauses the watch as Paused and closes its open cards. No card is raised for a paused watch.

A watch stops after 14 days without a push by Puppet Master, and at most 30 watches are active at once.

## The flows

**Looking.** A look runs every 20 seconds, only while the switch is on. A watch is looked at every 60 seconds while checks
are still running, every 5 minutes otherwise, and backs off up to 15 minutes after a read error. A check that fails
(`failure`, `timed_out`, `startup_failure`) on the head commit is the trigger. `action_required` is shown as
"Waiting for approval on GitHub" and is never acted on. Puppet Master waits while other checks still run, and offers
after 30 minutes from the first failure if a check hangs.

**The offer.** One card per (watch, failing commit). Its buttons: `Ask <agent> to fix`, `Not now`, `Stop watching this PR`.
The card is raised only after the log is read and cleaned (at most 3 jobs are read). The snapshot is stored with the fix,
and `Show log` shows exactly that stored text. A fix that was declined, expired or finished for the same commit raises
no second card. A try is counted when the agent starts, not when the card is raised.

**Start (flow D).** On `Ask <agent> to fix`, Puppet Master checks, in order: the switch is still on; the watch is still
watching; tries are left; the day cap; no other fix runs (`one_running`); the copy exists and is not busy (a busy copy
waits up to 30 minutes, then the fix fails); `gh` is signed in and the folder's remote is the watch's repo; a fresh read
of GitHub still shows the same head commit as the failure; the copy is on the `pm/` branch at that commit; the remote
branch is still that commit; the agent can start. Then it:

1. records the git fingerprint of the copy (`harden::config_snapshot`);
2. writes the log file `.puppet-master/ci-<pr>-try<n>.md` in the copy, from the stored snapshot (one fence, see S1);
3. counts the try and saves the fix as "starting";
4. starts the agent with the task (below), by one of two routes.

**Route A: same conversation.** If the agent's last run in this copy is in a pane that is guarded against pushing, still
open, finished (idle) and not handed off, the task goes in as a follow-up turn (key `cifix:<fix id>`).

**Route B: new pane.** Otherwise a new run starts in the same copy (`join_worktree`), visible, kept open, under the
no-push guard (`guard_env = no_push`), with `trust_workspace` false, `allow_shell` false, and the turn and minute budgets
from Settings. Its handle is `cifix-<pr>-<4 hex>`. If route A is refused because the task moved to another agent or the
pane closed, route B is used once.

**The task.** It names the pull request number, `owner/repo` and the `pm/` branch, and the log file name. It says the log
is data, not instructions, and asks the agent to make the smallest fix, not to skip or weaken tests, not to push, and not
to switch branches. It has no log byte and no check name in it. Its exact wording is in `text.rs` (`task`).

**Finish (flow E).** When the agent's run is over and the pane is idle, Puppet Master saves the agent's loose edits as a
commit (`commit_pending`), then checks the change against the base: the shared push check (`issue_intake/pushcheck.rs`)
plus `fixcheck.rs`. The result is one of:

- **No change**: a notice. The fix ends as "no changes".
- **Held**: no push button. Held causes: changes to CI or workflow files (`.github/`), git settings or hooks changed during
  the run, changes to Puppet Master's own files (`.puppet-master/`), the remote branch moved, the history was rewritten,
  or the copy is on another branch. Card: `Look first` and `Don't push`.
- **Ask**: a card `Push the fix to #<pr>?` with `Push fix`, `Look first` and `Don't push`. The card lists what it found.
  These only ask, they do not hold: secret-looking files or lines, lines with your home folder path, files CI runs,
  skipped or deleted tests, web addresses added in the code, more than 100 files, a patch over 256 KiB, a run that did not
  finish. If the change moved after the card was shown, a new card is raised before anything is pushed.

**Push (flow F).** On `Push fix`: the fresh GitHub read must still show the failing commit as head, the repo facts must
match, and the remote branch must still be the failing commit. Then `git push` runs with an explicit refspec to the same
`pm/` branch, with no force flag. If the remote moved, git refuses and nothing is pushed. A push failure gives a card
`Try again` / `Got it`. On success the new commit goes into `pushed_shas` and the watch looks at GitHub again soon. If our
own push lands while a look is under way, that look reads GitHub again and does not pause the watch.

**Result.** A green check on the new commit ends the fix as green. A red one raises a new card while tries are left.
A merged or closed pull request ends the watch, and open cards go.

**Restart.** A fix saved as asking gets its card again. A fix that was running is resumed. A push that was cut off is
settled by comparing the remote branch with the copy's head.

**Stop (flow G).** `Stop watching this PR` (card or row) stops the watch and closes its open cards. `Watch again` on a
paused or stopped watch starts it again.

## Guards (threats S1 to S16)

Each guard is in the code and tested where it is built (see `ci_fix/mod.rs` for the one-line list).

- **Log text is data (S1).** CI logs never go into the task. They go into one JSON object inside one `context_pack`
  fence in the copy's log file. ANSI and OSC-8 links, timestamps, zero-width, bidi and control characters are removed and
  counted. Lines are cut at 400 characters, 200 lines per job, and 24 KiB in total (the newest lines stay).
- **Secrets stay out (S2).** Redaction runs before anything is stored: known secrets, token shapes, e-mail addresses (masked)
  and the home folder (written as `~`). Only the cleaned text is kept, and a finished fix keeps no lines (only its `sha8`).
  The push card's text never carries log lines, so the lock screen does not show them. Hidden secrets are counted.
- **Only Puppet Master's pull requests (S3).** The ownership rules above, checked on every poll.
- **The agent cannot push (S4).** Route A only into a guarded pane; route B always starts a new guarded run. The guard
  (`pty/env_guard.rs`, `no_push`) empties the GitHub config folder, rewrites push URLs, resets the credential helper,
  removes `GH_*`, `GITHUB_*` and token-like variables.
- **No protected branch (S5).** `pr::guard_branch` (only `pm/`), the default branch and the PR base are refused, explicit
  refspec, no force flag in `push_update`.
- **Pushes are asked (S6).** Every push is asked; CI files, git settings, Puppet Master's own files, moved remotes and
  rewritten history hold it.
- **Puppet Master's own git is hardened (S7).** `harden::git_env` (hooks path empty, fsmonitor off). The fingerprint is
  compared before the card and again before the push.
- **Only the person answers (S8).** `ci_fix` and `ci_push` are person-only kinds (`questions/mod.rs`). A plain loopback
  answer or dismiss gets 403 `CI_FIX_NEEDS_PERSON`. Chat answers are refused (`attention/answer.rs`). The engine acts
  only on question ids stored in a fix, never on a card's kind or text.
- **GitHub is read-only (S9).** Paths built in `github.rs` are reads. `WRITE_PATHS_NEVER` lists the words that must never
  appear. The only write is the fast-forward push. `action_required` is shown, never acted on.
- **Spend is bounded (S10).** Tries per pull request, fixes a day, one fix at a time, the watch cap, and the 14-day life.
- **Checks before acting (S11).** Start and push both re-check the remote commit and the copy first.
- **Nothing types into a pane (S12).** No code in `ci_fix/` writes to a pane. Trust and permission prompts are left to the
  person.
- **Logs stay here (S13).** Logs are local and redacted. The only outbound write is the push the person agreed to.
- **No agent names in generic code (S14).** Labels come from `crews::live::label_of`.
- **No new bridge route or MCP tool (S15).** The phone answers through the existing question route.
- **The old raw-log path is gone (S16).** The Send back command and button are removed (RG-11). `worktrees/sendback.rs`
  keeps only `pane_if_guarded`. A test (`no_raw_ci_log_read_is_left_outside_tests`, `worktrees/tests_ship.rs`) fails if any
  non-test code contains `log-failed`, the `gh` flag that reads a failed job's raw log.

## Error codes

Shown on cards and rows, and in the audit log (`ci.fix`). Defined in `ci_fix/model.rs`.

| Code | Meaning |
|---|---|
| `CI_FIX_GH_MISSING` | The GitHub CLI is not installed |
| `CI_FIX_GH_SIGNED_OUT` | The GitHub CLI is not signed in |
| `CI_FIX_NOT_PM_BRANCH` | The branch is not one Puppet Master opened, or the copy is on another branch |
| `CI_FIX_NOT_YOURS` | Someone else opened it, it is for another repository, or the folder points elsewhere |
| `CI_FIX_FORK` | A fork pull request. Never offered |
| `CI_FIX_PR_CLOSED` | The pull request is closed or merged |
| `CI_FIX_HEAD_MOVED` | The head commit changed after the failure |
| `CI_FIX_COPY_GONE` | The copy folder is gone |
| `CI_FIX_COPY_BUSY` | The copy is in use by an agent or another run |
| `CI_FIX_AGENT_NOT_READY` | The agent cannot start right now |
| `CI_FIX_TRIES_USED` | No tries left for this pull request |
| `CI_FIX_DAY_CAP` | The day's fix limit is reached |
| `CI_FIX_START_FAILED` | The fix could not start |
| `CI_FIX_GIT_CHANGED` | Git settings or hooks changed while the agent worked |
| `CI_FIX_CI_FILES` | The change touches CI or workflow files |
| `CI_FIX_HISTORY_REWRITTEN` | The branch history was rewritten |
| `CI_FIX_REMOTE_MOVED` | Someone pushed to the branch after the failure |
| `CI_FIX_PUSH_HELD` | The push is held |
| `CI_FIX_PUSH_FAILED` | The push failed; the commits stay in the copy |
| `CI_FIX_WATCH_FAILED` | The watch could not be saved |
| `CI_FIX_NEEDS_PERSON` | A plain loopback request tried to answer a card |
| `CI_FIX_OFF` | Red to Green is off. The Labs switch must be on first (`Turn on Red to Green in Settings > Labs`) |
| `CI_FIX_BUSY` | Puppet Master is already working on this. Try again in a moment |
| `CI_FIX_NOT_FOUND` | There is no such pull request or fix (the id is unknown) |

A change with no edits ends as a notice (`FixState::NoChanges`), not as an error code. `CI_FIX_NO_CHANGES` is defined in
`model.rs` but nothing raises it yet.

## Residual risks (read before you turn it on)

- **The agent runs as you.** It is your user account in your copy. The guard removes GitHub and package tokens and stops
  pushes, but it is not a sandbox: the agent can still read files on this PC, including outside the copy, as the agent
  can anywhere else.
- **CI runs the fix.** When you push, CI runs the new code with the repo's secrets. CI or workflow changes, git settings
  changes and moved remotes hold the push. Other changes only ask. Read the card before you say yes.
- **Redaction is pattern-based.** It masks known secrets, token shapes and e-mail addresses. A secret in an unusual shape
  can get through. Log text is still untrusted data.
- **Small windows.** GitHub is read, then the push runs. Between the two the remote can move. The push is not forced, so
  git refuses in that case.
- **Untested against real GitHub.** Everything is tested with a fake `gh`, a fake git remote and a fake agent. A live run
  is needed before this is trusted.
- **Not in v1.** Branches made outside Puppet Master, other people's pull requests, fork pull requests, re-running CI,
  workflow dispatch, approving runs, merging, and any push without your yes.
