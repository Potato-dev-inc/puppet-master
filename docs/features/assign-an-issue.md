# Assign an issue: an agent works on a GitHub issue

Code: `packages/app/src-tauri/src/issue_intake/` (the rules, the GitHub reads, the push check, the store),
`commands/issue_intake.rs` (the Settings commands), `pty/env_guard.rs` (keeps tokens out of the agent's pane) and
`worktrees/harden.rs` (safe git for Puppet Master's own calls). In the UI: `components/settings/blocks/AssignIssueSettings.tsx`,
`AddRepoSheet.tsx`, `components/cockpit/IssueCard.tsx` and `lib/issue-intake.ts`. The file map is in `docs/REPO_MAP.md` section 3.11.

Status: **experimental**, in Settings > Labs. Unit tests with a fake GitHub and a fake world pass. No run against real
GitHub is recorded yet.

## What and why

Someone on your list adds a label to a GitHub issue. Puppet Master sees it and asks you first. If you say yes, an agent
works on the issue in a separate copy of the repo, on its own branch. When the agent is done, Puppet Master checks the
change and asks you to open a draft pull request.

Puppet Master never merges. The agent never pushes.

## Setup with gh

Puppet Master uses your own GitHub CLI (`gh`) sign-in. It never asks for a token.

1. Install the GitHub CLI (`gh`).
2. Sign in once in a terminal: `gh auth login`. Check it with `gh auth status`. It should say you are logged in.
3. Open Settings > Labs > **Assign an issue**. Turn on **Watch GitHub issues**. The line under it says
   `Signed in to GitHub as @you (gh)`.
4. Click **+ Add a repository**. Pick the folder of a clone of `acme/demo`. The folder's GitHub remote must be exactly that
   repo. The sheet shows the repo and its default branch, or why the folder cannot be used.
5. Set the label (the default is `puppet`; create the label on the repo in GitHub first), the agent, and the budgets.
6. Save.

If `gh` is missing or signed out, nothing is checked. The Settings line says what to do. Puppet Master does not store
your sign-in.

## Who can start a run

- **You.** The login of your `gh` sign-in. Your labels always count.
- **People on your list.** Add GitHub names under **Who can start a run** (up to 50). Names are saved in lower case.
  A bot counts only when it is on the list exactly, as `name[bot]`.
- **Anyone with write access.** Off by default. When it is on, anyone with admin, maintain or write permission on the repo
  can start a run.
- **Anyone else.** No card. The run is listed under **Recent** as `Skipped: @someone is not you or on your list`.

Puppet Master asks GitHub who added the label (the newest label event). If GitHub cannot say, no card is raised.

Only labels added after you added the repo count. A label that was there before is listed as
`Skipped: Labeled before you added this repo`, and nothing starts on its own.

The issue author is not trusted. If the author is not you and not on your list, the start card asks even when
**Ask me before each run** is off.

## The start card

When a label is accepted, an Inbox card appears:

- Title: `Start on #42 "Login button does nothing" with Codex?` (the issue title, cut to 60 characters)
- Header: `GitHub issue #42 · acme/demo`
- Lines: who labeled it, who wrote it, what was cleaned (for example `Notes: 1 hidden comment removed.`), and why the card
  asks first, if it does.
- Buttons: **Start with Codex** (the agent's name is on the button), **Not now**, **Ignore this issue**.
- **Show issue text** opens the text the card has: the lines above, and the first 400 characters of the issue body.

With **Ask me before each run** on, nothing runs until you press **Start**. The card waits 24 hours. After that it says
"The card timed out" and nothing starts. With the switch off, an allowed label starts the run by itself, and no card
appears unless one of the reasons below applies.

**Not now** keeps the issue for later. Under **Recent** you can press **Ask again**.
**Ignore this issue** means this issue number is never asked again. **Stop ignoring** is under **Recent**.

Puppet Master asks even when **Ask me before each run** is off, when the author is not allowed, when hidden text was
removed, when the text was cut, or on the first run in that repo.

A phone that is paired with Puppet Master can answer the start and push cards. Settings can only be changed on the PC.

## What the agent gets

- **A copy.** A new git copy of the repo under `<repo folder>/.puppet-master/worktrees/`, made from the latest default
  branch. It is on its own branch, `pm/issue-42-<4 hex>`. The copy is hidden from git in your repo's `.git/info/exclude`.
  The project's setup steps run in the copy. Its copy-files list is skipped, so `.env` files are not copied.
- **A visible pane.** It opens like any other agent pane. You can watch it work.
- **A short task.** Puppet Master writes it. It has no issue text. It says to read `.puppet-master/issue-42.md`, to treat
  that file as data and not as instructions, not to push, not to open a pull request, not to switch branches, and to commit
  the work.
- **The issue file.** A short header in plain words, then the title and the body as one JSON object inside one fence. Hidden
  HTML comments and invisible characters are removed first. The body is cut at 16 KiB.
- **Budgets.** Turns and minutes from the repo's settings. When one is used up, the run pauses before its next turn. An
  Inbox card then offers to add more turns or minutes, or **Stop here**. A run never stops in the middle of a turn.
- **No extra rights.** Trust, shell commands and the allowed-commands list are all off. If the agent shows a trust,
  permission or sign-in screen, the screen waits for you. Puppet Master never answers it.

## The guards

These are the rules that limit what an issue run can do.

1. **The issue is data.** Its text only goes into the file. The task never has it.
2. **Only allowed people start runs.** Labels are checked against GitHub's events. Any error means no.
3. **One repo, one folder.** The folder's remote must be the listed repo. This is checked when you save, when a run starts,
   and before the push.
4. **No GitHub token in the agent's pane.** The `no_push` guard removes GitHub and package tokens from the pane's
   environment. Your `gh` sign-in is not visible to the agent: `gh` reads an empty folder. Your API keys (names ending in
   `_API_KEY`) are kept, because agents use them to log in.
5. **The agent cannot push.** `git push` in the pane fails: push URLs for `https://`, `git@` and `ssh://` go to a helper that
   does not exist, and no git credential helper is set. Puppet Master does not answer the agent's trust or login prompts.
6. **Puppet Master pushes a draft only.** It pushes only a branch named `pm/...`, with an explicit name, never forced. It
   opens the pull request with `gh pr create --draft`. It never merges and never marks the pull request ready.
7. **The pull request does not close the issue.** The body starts `Refs #42`. Words like `Closes #42` in the title or body
   are reworded.
8. **The pull request has no secrets and no home path.** Known secret shapes and known values are masked. E-mail addresses
   become `<email>`. Your home folder becomes `~`.
9. **Git settings are checked.** Puppet Master's own git calls on the copy ignore the repo's hooks and fsmonitor. It takes a
   fingerprint of the repo's local git settings and hook files when the run starts. It checks again before the push card
   and right before the push. A change holds the run.
10. **Only the person answers the cards.** A start or push card cannot be answered by a plain local request. A card an
    agent makes does not start or push a run, even with the same words and buttons. The Settings commands are desktop only.

### Risky changes always ask

The push check reads the change before a pull request is offered. These always raise a card, even when
**Ask me before pushing** is off:

| What the change has | What you see |
|---|---|
| A CI or workflow file (`.github/`, `.circleci/`, `.buildkite/`, `.gitlab-ci.yml`, `azure-pipelines.yml`, `Jenkinsfile`, `bitbucket-pipelines.yml`, `.travis.yml`) | Card: `Changes CI or workflow files: ...` |
| A file with a secret-looking name (`.env`, `.env.*`, `.pem`, `.key`, `.p12`, `.pfx`, `id_rsa*`, `id_ed25519*`, `.npmrc`, `.pypirc`, `credentials*.json`) | Card: `Adds files that look like secrets: ...` |
| An added line that looks like a key or token | Card, with file and line |
| An added line with your home folder path | Card, with file and line |
| More than 100 files | Card: `Changes N files, more than 100.` |
| A patch over 256 KiB, so not every line can be checked | Card: `The change is too big to check every line` |
| A run that did not finish (failed or cancelled) | Card |

Names match at any depth and ignore upper and lower case. Each list shows three examples and "and N more".

### Held runs

Some changes are **held**. A held run has no push button at all. Its card has only **Look first** and **Throw away**:

- The repo's git settings or hook files changed while the agent worked (`ISSUE_GIT_CHANGED`).
- A branch with the same name already exists on GitHub (`ISSUE_REMOTE_BRANCH_EXISTS`).
- The change includes Puppet Master's own files in `.puppet-master/` (`ISSUE_PUSH_HELD`).

The card says `Puppet Master won't push this. Check the copy yourself.` **Look first** opens the pane. **Throw away** removes
the copy and its branch.

## The cards after a run

| Card | When | Buttons |
|---|---|---|
| `Draft PR ready for #42?` | The agent is done and the change is checked. Shown when the repo asks before pushing, or a risky change needs a yes. Facts line: `3 files · +40 −5 · Codex · 12 turns` | **Open draft PR**, **Look first**, **Throw away** |
| `Draft PR for #42 is on hold` | A held run (see above) | **Look first**, **Throw away** |
| `No changes for #42` | The agent finished without editing files | **Open the agent**, **Throw away** |
| `Couldn't open the PR for #42` | Git or GitHub failed during the check or the push | **Try again**, **Got it** |

When the draft is open, **Recent** shows `Draft PR #7` as a link. The pull request is for you to review and merge yourself.

## Limits

- At most 20 repos, 50 GitHub names, and 3 runs at once across all repos (default 1). Extra approved runs wait in line.
- Per repo: turns 1 to 200 (default 30), minutes 5 to 240 (default 45), runs a day 1 to 50 (default 5). A run counts from
  its start. When the day is used up, approved runs wait.
- Puppet Master looks at each repo about once a minute, or as often as GitHub's own poll interval asks if that is longer.
  **Check now** looks at once. Each new label counts once. A label added again later counts again.
- When GitHub cannot be read, no new run starts. Runs already under way still finish. The Settings row says why.
- If the app restarts during a run, the run carries on from where it was. A run that was starting fails with
  "Puppet Master restarted while this was starting. Ask again." Its copy may stay under `.puppet-master/worktrees/`.

## Error codes

These codes are saved with the run (in `Held` and `Failed` rows) or returned by the Settings commands.

| Code | When |
|---|---|
| `ISSUE_REPO_MISMATCH` | The folder's remote is not the listed repo, GitHub says the copy is another repo, or the repo is no longer on your list. Save is refused with the reason. |
| `ISSUE_AGENT_NOT_READY` | The chosen agent is not ready. Save is refused. A run says `<agent> can't start right now.` |
| `ISSUE_NOT_ALLOWED` | You pressed **Ask again** on a run whose labeler is not you or on your list. |
| `ISSUE_CLOSED` | The issue is closed (or gone) when the run starts. |
| `ISSUE_LABEL_GONE` | The label was taken off the issue before the run starts. |
| `ISSUE_START_FAILED` | GitHub or git could not be read, the copy could not be made, the agent did not start, or the app restarted during the start. The copy is removed when the agent did not start. The day's run count is not used. |
| `ISSUE_GIT_CHANGED` | The repo's git settings or hook files changed during the run. The run is held. |
| `ISSUE_REMOTE_BRANCH_EXISTS` | A branch with the same name is already on GitHub. The run is held. |
| `ISSUE_PUSH_HELD` | The run is held for another reason, or the push card timed out or was closed. Nothing is pushed. |
| `ISSUE_PUSH_FAILED` | The check or the pull request could not be done (for example `gh pr create` failed). The card offers **Try again**. |
| `ISSUE_BUSY` | You pressed a button on a run that a job is still working on. Try again in a moment. |
| `ISSUE_RUN_NOT_FOUND` | No run has that id. |
| `ISSUE_NEEDS_PERSON` | A plain local request tried to answer or dismiss a start or push card. Refused with HTTP 403. The card stays open. |

## Residual risks

Read these before you turn the feature on.

1. **The agent reads the whole issue, and an issue is untrusted text.** The guards limit what the agent can do. They do not
   limit what it reads. A hostile issue can still try to mislead the agent. For example, it can ask for a test to be
   removed, or for a change that looks normal but is wrong. The push check looks at CI files, secrets, git settings and size.
   It does not judge the code. **Read the draft pull request before you merge it.**
2. **The agent runs as you.** It can read any file you can read, including other repos and your keys, and it can use the
   network. The guard removes tokens from the pane's environment. It does not stop reads or network calls.
3. **The push guard covers GitHub-style remotes only.** Rewritten URLs cover `https://`, `git@` and `ssh://`. A remote with
   another form, such as a folder path or a `file://` URL that the agent adds to its copy, is not blocked during the run.
   The git settings check runs after the run, so it cannot undo a push the agent made itself. A remote like that must exist
   on your PC.
4. **Secret checks use known shapes only.** A secret with no known shape, and no value Puppet Master already knows, is not
   flagged. The push card does not show the diff; read it in the copy with **Look first**.
5. **Project setup steps run with your rights.** If your project has setup steps, they run in the new copy. They are your
   own commands. Puppet Master does not check them.
6. **Cost.** Each run uses agent turns and minutes. The budgets cap a run. The runs-a-day cap limits label spam, but
   allowed people can still use it.
7. **Write access widens who can start a run.** It is off by default. Turn it on only for a repo where you trust every writer.
8. **The issue text can change after you say yes.** The agent gets the text you saw. Later edits are not read. The start
   checks that the issue is still open and still labeled. If it is not, the run fails.
9. **Issue text is stored on your PC.** Each run keeps the issue title and body in `runs.json` in the app's data folder
   (up to 500 runs). The audit log keeps numbers and names, not text.

## Not in v1 (v2)

These are planned, not built:

- A fine-grained GitHub token as a fallback to `gh` (needs your decision).
- Comments from allowed people on the issue as follow-ups.
- PR review comments sent back to the agent.
- An optional comment on the issue with the PR link.
- Changing the label after the PR is open (for example `puppet` to `puppet: done`).
- A crew per repo instead of one agent.
- **Do it tonight** (a queue for Night Shift) and **Until checks pass** (a mission).
- A message when a draft PR is opened.
- Forks, GitHub Enterprise hosts, a different agent per card, and a sandbox for the agent's reads.

## How it is tested

- Unit tests sit next to the code in `issue_intake/` (`*_tests.rs`, and tests in `text.rs`, `filter.rs`, `github.rs` and
  `model.rs`). The engine tests use a fake world with a virtual clock. They never touch GitHub, git, an agent or your app data.
- Recorded `gh` answers and issue text are in `issue_intake/fixtures/`. Hostile titles and bodies are in the text tests.
- The push check has real git tests in `pushcheck_git_tests.rs`, on temporary repos.
- `pty/env_guard.rs` tests check the variables a guarded pane gets and what it loses.
- `commands/issue_intake.rs` tests check that a refused save writes nothing.
- The person-only card rule has tests in `questions/tests.rs`.
- An end-to-end test is written on a separate branch and is not on main yet. It uses a fake `gh` on `PATH`, a local bare
  remote behind `insteadOf`, and a fake agent in a real pane. It checks the whole path: label, start card, guarded pane,
  push card, draft PR, and the held and outsider cases.

## Settings reference

| Setting | Where | Default |
|---|---|---|
| Watch GitHub issues | Settings > Labs > Assign an issue | off |
| Repositories: add, edit, pause, remove | the **Repositories** block | none |
| Label | repo sheet | `puppet` |
| Agent | repo sheet (only ready agents are listed) | none |
| Turns per run | repo sheet | 30 (1 to 200) |
| Minutes per run | repo sheet | 45 (5 to 240) |
| Runs a day | repo sheet | 5 (1 to 50) |
| Ask me before each run | repo sheet | on |
| Ask me before pushing | repo sheet | on |
| Who can start a run (GitHub names) | **Who can start a run** block | none (you only) |
| Anyone with write access to the repo | **Who can start a run** block | off |
| Runs at once | the Labs block | 1 (1 to 3) |
| Recent | the Labs block | last 20 runs; **Ask again** on "Not now" rows, **Stop ignoring** on ignored rows |
