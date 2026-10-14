# Simple mode: the cockpit with fewer buttons and plain words

Branch `feat/plain-mode-v0` (built on `fix/live-r12`). Design: `D:\pm-night\IDEAS-R8-PLAIN-MODE.md`. The UI calls it
"Simple"; the code says `simple` and `plain_mode`.

## 1. What and why

**Persona.** C: people who do not write code (Priya, Ana, Mr Chen, Sofia). Many read English as a second language.
They want something that works and that they can show, and they are afraid of losing real files.

**Switch.** Settings > General > "Simple mode: fewer buttons and plain words". `simple_mode` in settings.json.
**Off by default**, so the normal cockpit is unchanged for everyone else. Individual UI mode is never replaced.

**Agent-agnostic.** The agent is the first one the registry says can run a visible task and is on the computer
(`agent-pick.ts` reads declared capabilities). Nothing in generic code matches an agent name; the name shown in
the sentences comes from the agent's own label.

## 2. What the person sees

- **Home.** One button, "Make something", and her projects (a project is an agent working on a copy).
- **Make something.** Three tap questions, then the words PM will send ("Here is what PM will ask Claude Code"),
  editable, with the house rules folded under "PM also asks it to". Templates are words only and make one html
  file: no build step, no Node.
- **A project.** Tabs `Preview · Conversation · What changed · Go back`. Bottom bar: `Keep` (primary) and
  `Throw away`. Header says "Working on a copy".
- **Approvals.** One short question with Yes or No. No "Always allow". A high-risk step is not asked: PM answers
  the agent's own No and says so ("PM stopped Claude Code. ... Your files are safe.").
- **The terminal** is under Details > Agent's screen. It shows by itself when PM cannot read the screen (the agent
  waits for a person and there is no approval PM understands, or the pane is in a problem state).
- **Problems** are one card, say whether her work is safe, never show a code and never use the word for a failure.

## 3. Guards (Rust, `plain_mode/`)

- **A copy, always.** `plain_mode::run_request` is the only place a Simple task is built and it sets
  `isolation: "worktree"`. `plain_start` has no argument that changes it. The agent's cwd is the worktree; the
  folder only changes on Keep (`worktree_merge`).
- **Broad folders are refused** (`guard::verdict`): the profile, Desktop, Documents, Downloads, a OneDrive root, a
  drive root, anything above the profile, Windows, Program Files, ProgramData, AppData, the Simple projects folder,
  and (for a folder she already has) anything over 5,000 files or 500 MB. The UI has a mirror
  (`folder-guard.ts`); both are tested against one list, `folder-cases.json`.
- **A new project** is a folder under `Documents\PM` (`PM_SIMPLE_PROJECTS_ROOT` moves it), made a repository quietly
  (`worktrees::folder::init`) so Keep and Go back work.
- **A folder she already has** is accepted only when it is already a repository with a commit. PM never runs
  `git init` inside a folder she owns. Copying other folders is future work.

## 4. Reuse (no forked logic)

| Simple | Reuses |
|---|---|
| Conversation | `useRunData` (the Transcript tab's), `LastTurnContext` |
| What changed | `Changes` with a new `plain` prop (the same worktree diff) |
| Go back | the checkpoints of the History tab, restored through `restoreWithUndo` (new optional words) |
| Keep, Throw away | `worktree_merge`, `worktree_discard` |
| Agent's screen | `TerminalViewport` |
| Approvals | the `prompt_risk` result; Simple only words it differently (`approval-text.ts`) |

## 5. Language

Every string lives in `components/simple/copy.ts` (plus the template, approval and problem tables).
`language.test.ts` fails any sentence over 15 words, any card over 3 sentences, any word that is not in
`word-list.ts`, and any word Simple never uses (repository, branch, merge, commit, checkpoint, error, ...).
The word list is a curated everyday-English list, not a published one; a new word is a code-review decision.
`approval-text.test.ts` checks that each Rust headline the table is keyed by still exists in `prompt_risk/rules.rs`.

## 6. Not in v0

Preview with point-and-fix, "Looks right" named versions, screenshots in Go back, copying non-repository folders,
setup (doctor) steps, sign-in cards, "Careful / Fast", the work limit, plan usage, phone changes, `ask_user`
question cards in Simple, translations. See the report for the list and for what the live check proved.
