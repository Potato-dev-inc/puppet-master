# Scores on your code

Code: `packages/app/src-tauri/src/scores/` (the numbers), `packages/app/src-tauri/src/commands/scores.rs` (the one Tauri
command, `agent_scores`), and the page in `packages/app/src/components/cockpit/AgentScores.tsx`,
`lib/agent-scores.ts`, `styles/cockpit-scores.css`.

Status: **experimental**. Built and unit-tested. No live run with a real agent yet. The **Undone** column cannot fill
in this build (see Limits).

## What and why

Puppet Master keeps a record of each turn your agents take. Scores on your code reads that record. It shows which
agent works best in one project.

It uses only your own history on this PC. It does not look at other people's code, other computers or a server.

It does not rank agents. It shows plain counts, such as `8 of 10`. It has no overall score, no stars and no "winner".

## Where to find it

- Sidebar: **Scores**, under the list of panes.
- Command palette: **Scores on your code**.

With a pane selected, the page opens on **This project** for that pane's project. A separate copy of a project counts
as the project it came from. With no pane selected, it opens on **All projects**.

Controls:

- **This project: <name>** or **All projects**.
- A project list. It shows only in project view, and only when there is more than one project.
- A window: **Last 30 days**, **Last 90 days** (the default) or **All time**.

A turn counts in the window where it started. A run counts in the window of its last turn.

## What you see

One row per agent and account. Rows are sorted by turns, most used first, then by name. Rows are never sorted by score.

Under each agent name there is a grey line, for example `Typical turn 4 min · 38 lines · 2 waiting for review`. It is
only for reading. It is not ranked.

The columns:

| Column | What it counts | Higher is | Counted out of |
|---|---|---|---|
| Turns | Turns in scope. | - | - |
| Ended OK | Turns that did not end with a crash or an API error. A turn you stopped is not counted. It says nothing about the code. | better | Turns that ended OK or failed |
| Checks green | Runs that had checks and all of them passed. Only runs started with checks count. | better | Turns whose checks got an answer |
| Merged | Separate copies you kept, out of the copies you kept or threw away. A copy still waiting for review is not counted. It is your choice, not proof the code is right. | better | Kept + thrown away |
| Undone | Turns that changed files and that you later undid with Undo. | lower | Turns that changed files and are covered by the undo log |
| Per merged change | Typical turns and typical working time of merged runs. Shown only with 3 or more merged runs. | not ranked | Merged runs |
| Asked you | Turns where you had to answer a prompt, or the turn ended waiting for you. It depends on your permission settings, so it is never ranked or marked. | neutral | Turns with a turn summary |

## Small samples

The page does not show a number when there are too few runs.

- 0 runs: `-`.
- 1 to 4 runs: `-`. Hover to see "Only 3 so far. A number needs 5."
- 5 to 9 runs: `x of y` with a small **early** tag.
- 10 or more runs: `x of y` and the percent.

If no column in a row has 5 or more runs, the row shows one line: "Not enough runs yet. 3 turns so far."

There is one comparison mark, **ahead**. It shows in one cell only. It needs two or more rows with 5 or more runs in
that column. A row is marked only when its 90% range is clearly better than the range of every other row. For Undone,
lower is better. The mark never appears on **Asked you**. With small samples, the mark almost never shows. That is by design.

## Notes at the bottom of the page

The page lists short notes from the backend. For example:

- "Counting Undo since <day>. Earlier undos were not recorded." Or "Counting Undo from now on."
- "N turns had no file information, so they are left out of Undone and Asked you." (Turns with no operation record are
  also left out of Asked you.)
- "N of M turns had checks. Checks count only when a run is started with them."
- "Tokens are not counted. Only usage an agent reports itself could show here, and it is not saved per turn."
- "N records could not be read. Their turns are not counted."

## How to read this

The page has a **How to read this** list. In short:

- Ended OK is about reliability. It is not about code quality.
- Different tasks are not equal. A hard task can make an agent look worse.
- New versions of an agent change its numbers. The page shows your past.
- Agents never see this page.

## Limits

- **Only this PC.** Turns from other computers and from teammates are not counted.
- **Only what the app kept.** The app keeps the last 100 turn summaries per pane and up to 20 panes per project file.
  Older turns may be gone.
- **Runs only for runs started as a run.** A turn you type by hand into a pane has no run. It still counts in Turns,
  Ended OK and Asked you.
- **Checks only when a run starts with them.** A turn with no checks is left out of Checks green.
- **Undone is empty in this build.** The page reads an undo log, but nothing writes it yet. So Undone shows `-` for
  every agent, and the note says "Counting Undo from now on." It will fill only after Undo writes to the log.
- **No tokens and no cost.** Nothing keeps usage per turn.
- **No task type.** The page cannot tell a hard task from an easy one.
- **Some records may be skipped.** A damaged record is skipped and counted in the notes. Its turns are not counted.
- **Small or mixed samples.** Even with 10 runs, one bad week can change a number a lot. Read the counts, not only the percent.

## Privacy

- The page reads only facts it needs: agent, account, times, outcome, checks, file counts and line counts, prompt
  counts. It never reads your prompts, replies, task text, file names, approval text or pane ids.
- A project shows only its last folder name.
- The only entry is the Tauri command `agent_scores`. There is no bridge route, no MCP tool and no phone route.
  Agents cannot read the page or their own scores.
- The page makes no network calls.
- Account labels can be e-mail addresses. The page is local. There is no export or copy in this version.
- The page is read-only. It does not stop, merge, discard or undo anything.

## How to test

Automated (from the repo root):

| Suite | Command |
|---|---|
| Rust | `cd packages/app/src-tauri; $env:CARGO_TARGET_DIR='C:\Users\you\cargo-target-shared'; cargo test --lib scores` |
| App | `cd packages/app; npx vitest run src/lib/agent-scores.test.ts src/components/cockpit/AgentScores.test.tsx src/styles/cockpit-scores.css.test.ts` |

The Rust tests include the guard that no shipped file outside the module names it, and the test that a planted prompt,
path, pane id or e-mail never reaches the report.

Live: **not run yet**. Nothing was checked in the running app with real agent turns.

## Not done yet

- Undo does not write the undo log yet. The planned hook in `checkpoints/` was not added.
- No phone view, export, copy or team comparison.
- No tokens or cost.
- No split by model, agent version or task kind.
- No races. The page reads only the history the app already keeps.
