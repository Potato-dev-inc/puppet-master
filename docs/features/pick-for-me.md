# Pick for me

Code: `packages/app/src-tauri/src/scores/pick/` (the decision), `packages/app/src-tauri/src/commands/scores.rs` (the one
command, `pick_agent`), and the menu in `packages/app/src/components/cockpit/NewAgentMenu.tsx`, with
`lib/pick-for-me.ts` and `styles/cockpit-pick.css`.

Status: **experimental**. Built and unit-tested. No live run with real agents yet.

## What and why

When you press **+** to start an agent, you may not know which one to pick. Pick for me looks at your own history in
this project and suggests one agent. It says why in one plain sentence, for example:

> Codex passed its checks 9 of 10 times here; Claude Code 5 of 10.

It is only a suggestion. Nothing starts until you press **Start**. You can always press **Choose myself** and use the
normal list.

It uses only your own results on this PC, in this project, from the last 90 days. It is worked out on your computer.
It does not know what you are about to ask an agent.

## Where to find it

- Press **+**. The first row of the menu is **Pick for me**.
- The row shows a short hint on its right:
  - `Looking...` while the answer is worked out.
  - The agent's name when there is a suggestion.
  - `None ready` when no agent can run.
- Press the row to open the pick panel. It shows the agent's name, the sentence, and two buttons:
  - **Start Codex** (or the name of the suggested agent). It starts that agent in this folder, the same way a row in the
    list does. The separate-copy switch and the other start rules still apply.
  - **Choose myself**. It goes back to the normal list.
- If the suggested agent is not in the menu or cannot be picked there, the panel says `<name> can't be started from this
  menu.` and shows only **Back**.
- If there is no suggestion, the panel shows only **Back** and the sentence says why.
- **Why this one** (closed at first) shows one line per agent, and the notes.
- The row is not shown when the answer fails, and never in the browser build or on the phone. The rest of the menu
  works as before.

## How it decides

1. **One candidate per agent.** Each agent is listed once. Its login is the saved login you used most in this project.
   If you never used a saved login here, the agent's own login is used. A saved login that no longer works is not used.

2. **Skip what cannot run now.** The first rule that matches wins:

   | Skipped because | Text in "Why this one" |
   |---|---|
   | It cannot run: not installed, not approved yet, the start check says no, or its saved login is gone | The plain reason, for example `Not installed on this computer` or `Approve this agent in Settings > Agents first.` |
   | Its own login is out of usage | `Out of usage until 17:00`, or `Out of usage` when no time is known |
   | It shares a plan with another ready agent that is out of usage | `Shares a plan with Codex, which is out of usage` |

   A limit never moves the suggestion to another login or to another agent of the same company. A company counts only
   when the agent names it in its limits table.

3. **Compare the numbers.** The three measures are the same counts as on the Scores page. They are checked in this order:

   1. **Checks green**: passed its checks.
   2. **Merged**: had its separate copy kept.
   3. **Ended OK**: ended without an error. This is about reliability, not quality. It decides only when the other two
      cannot.

   An agent counts on a measure only with at least **5 runs** for that measure.

   The first measure where two or more ready agents have 5 runs or more decides. The search stops there. Puppet Master
   does not try the next measure to find a better answer.

   - **Clear leader.** One agent is clearly ahead when its 90% range is fully above every other agent's range. The same
     rule decides on the Scores page. That agent is picked, and the sentence shows the counts.
   - **Close call.** No clear leader. The usual agent is picked, and the sentence says the agents look about the same.

4. **Too early.** No measure has two agents with 5 runs or more. The usual agent is picked, and the sentence says it is
   too early to tell.

5. **The usual agent.** The default agent in the Ask bar. If there is none, the agent you used last. If that one is not
   ready, the first ready agent in the list.

More rules:

- Only one agent is ready: it is picked, and the sentence says so.
- No agent is ready: there is no pick. The sentence says why, for example `No agent is ready: Codex - Out of usage until 17:00`.
- A tie goes to the usual agent.
- A clear leader wins even when it is not your usual agent.
- One agent with many runs and no other agent with runs is "too early". A history of one agent is not a comparison.
- A measure with 5 to 9 runs is **early**. The sentence says "so far" and the panel shows a small `early` tag. With 10 runs
  or more the numbers are steady.

### The sentences

These are the sentence shapes. Names and numbers are examples.

| Case | Sentence |
|---|---|
| Clear leader | Codex passed its checks 9 of 10 times here; Claude Code 5 of 10. |
| Clear leader, few runs | Codex passed its checks 6 of 7 times here so far; Claude Code 1 of 5. |
| Clear leader, another agent out of usage | Claude Code is out of usage until 17:00, and Codex passed its checks 9 of 10 times here. |
| Only one agent ready | Only Codex is ready. |
| Only one ready, another out of usage | Only Codex is ready, because Claude Code is out of usage until 17:00. |
| Too early | Too early to tell your agents apart here, so your usual one, Codex. |
| Close call | Your agents look about the same here so far, so your usual one, Codex. |
| Nothing ready | No agent is ready: Codex - Out of usage until 17:00. |

"Why this one" shows a line for each agent. For the measure that decided, the line reads, for example `9 of 10 checks green`,
`4 of 6 copies kept` or `18 of 20 turns ended OK`. A count under 5 adds `(too few to count)`. An agent with no runs
shows `No runs here yet`.

## What it never does

- **It never reads or sends what you type.** The menu has no task box. The only input to the command is the project
  folder path.
- **Nothing leaves the computer.** The pick code has no network code and does not ask any agent a question.
- **It stores nothing.** No file is written, nothing is logged, no audit entry is made and no run is created. The
  answer lives in the menu until you close it.
- **It starts nothing until you press Start.** No automatic start, no timer and no remembered "always pick".
- **It does not switch logins or agents to get around a usage limit.** The limit stays where it is. Keep going is the
  only feature that moves a task, and it needs its own press.
- **Agents never see it.** No bridge route, no MCP tool and no prompt uses it.
- **It does not answer the agent's questions.** Trust, permission and sign-in questions of the agent that starts are
  not touched by this feature.
- **It does not show private text.** The sentence has names, counts and reset times. It has no folder path, no prompt
  and no reply. A saved login's label can be an e-mail address. It is shown only in the menu.

## Honest limits

- **Often "too early".** Each agent needs 5 runs for a measure before it counts. A new project, a new agent or a few
  runs means many answers say "too early" and use your usual agent.
- **It is about the project as a whole, not your task.** It cannot tell which agent is best for the task you are about
  to give. An agent that is good at tests may be weaker at styling. There is no task kind in this version.
- **Counts, not quality.** "Passed its checks" depends on the checks that project has. "Ended without an error" does not
  mean good work.
- **Limits are only the ones Puppet Master has seen in this app session.** After a restart it does not know a limit it
  has not seen again. The agent's own screen still shows its limit.
- **Only the last 90 days, only this project.** A separate copy of a project counts as the project it came from. There
  is no fallback to other projects.
- **Undone is not used.** It has no data yet, so it does not count.
- **Desktop app only.** It is not in the browser build or on the phone.
- **Not checked live.** It is built and unit-tested. It has not been tried with real agents, and the menu row has not
  been checked in the running app.

## Tests

- Rust: `cargo test --lib scores::pick` (see [`docs/TEST-PLAN.md`](../TEST-PLAN.md)).
- App: `npx vitest run src/lib/pick-for-me.test.ts src/components/cockpit/NewAgentMenu.pick.test.tsx src/styles/cockpit-pick.css.test.ts`.

Related: [Scores on your code](agent-scores.md).
