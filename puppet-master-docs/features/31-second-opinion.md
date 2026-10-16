# Second opinion

**Status:** Works today, but tested only by automated tests. It was never run with a real reviewer agent.

## What it is
**Second opinion** asks a different agent to read what the first agent just changed. The reviewer can only read. It cannot edit files or run commands. Its answer comes back as short notes pinned to the changed lines, and one verdict line. You can send any note back to the agent that wrote the code with one click.

## Why it matters
Reading a diff (the list of changed lines) takes time, and you may not be able to judge the code. A second agent is a cheap extra pair of eyes before you merge. It works across companies: for example, Codex can review work done by Claude Code. An app that has only one agent cannot do that. Developers get a quick review loop. People who build with AI get a plain answer to "does this look right?".

## How to use it
1. Open the pane of the agent whose work you want checked.
   - If the agent works in a [separate copy](22-separate-copies-worktrees.md), use its **Changes** tab.
   - Otherwise, use the **Receipt** tab.
2. Click **Second opinion**.
3. A sheet asks **Who should review?** It lists agents that can run read-only and are installed. An agent marked "ready" is already warm and is chosen first. The agent that wrote the code is hidden. You can tick **Also allow** to add it.
4. Click **Start review**. A spinner shows while it works. Click **Stop** to cancel.
5. Read the result:
   - A verdict line: "Looks good", "Some concerns" or "Changes needed".
   - Notes under the lines they are about. A mark in the gutter shows how serious each one is: `!!` high, `!` medium, `·` low, `–` just a note. There are no colors.
   - Notes that do not match a line are listed under "Elsewhere".
6. Click **Send to <agent>** on a note to send it to the author. You can also write your own comment under any line, then send it.
7. If there are concerns, the **Merge** button becomes **Merge anyway**. See [Review, merge and pull requests](23-review-merge-and-pull-requests.md).

## How it works (simple)
- **Which changes are reviewed.** In this order: the separate copy's changes, then the changes of the last turn (from checkpoints), then `git diff HEAD`. The sheet says which one was used. The diff is cut at 48 KiB, at a file edge, and the reviewer is told.
- **Read-only is enforced.** The review run is a background run with no shell, no trust grant and a fresh context. The app refuses an agent that cannot be held to read-only.
- The prompt has the task, a short summary of the turn, receipt facts, your shared project memory and the diff. It asks for a fixed answer shape: lines of `file:line | severity | finding`, and a final `VERDICT:` line.
- A forgiving reader parses the answer. If the reviewer does not follow the shape, the text is kept as plain text.
- **Send to agent.** If the pane belongs to a run, the note becomes the next turn of that run (or waits behind a running turn). If you opened the pane by hand, the note is typed into the pane and submitted. A closed pane gives "nobody to send this to".
- The review is a snapshot. If the agent starts working again, the panel says the notes may be out of date. Run it again to refresh.

## Limits and what is not done yet
- **No live run with a real reviewer.** The parts around it are tested. The path through a real agent was not. Try it by hand first, for example Codex reviewing a Claude Code pane.
- The quality of a review is not checked. Reviewers may drift from the format. The reader accepts many shapes, but some answers will show as plain text.
- `git diff HEAD` skips new files that Git does not know yet. New files in a folder without a copy may not be shown. With checkpoints off, that can end as "nothing to review".
- Notes are pinned to line numbers of the new file. A note on a removed line goes to "Elsewhere".
- Not every agent can review. OpenCode, a plain shell, and unknown agents are refused as reviewers.
- Review of a copy reads your original folder, where the files may not show the change. The prompt tells the reviewer to trust the diff.
- The panel checks for the answer every 2 seconds. The phone has no Second opinion screen.
- A reviewer uses your plan like any other agent. A review is not free.

## For developers
- Module: `packages/app/src-tauri/src/second_opinion/` (`source.rs`, `prompt.rs`, `parse.rs`, `route.rs`, `service.rs`).
- Tauri commands: `second_opinion_agents`, `second_opinion_start`, `second_opinion_poll`, `second_opinion_cancel`, `second_opinion_send` (`commands/second_opinion.rs`). No HTTP route and no MCP tool.
- Reviewer gate: `tui::read_only_backends()` (`Features.read_only_headless`), shared with `quick_ask`. Run is `read_only: true`, `allow_shell: false`, `trust_workspace: false`.
- Last-turn diff: `checkpoints::last_turn_patch`.
- UI: `components/cockpit/SecondOpinion.tsx`, `AnnotatedDiff.tsx`, `review-diff.ts`, `second-opinion-store.ts`; the notes show inside `Changes.tsx`.
- Tests: 26 Rust tests in `second_opinion/tests.rs` and several front-end test files.

## Related
- [Review, merge and pull requests](23-review-merge-and-pull-requests.md)
- [Separate copies](22-separate-copies-worktrees.md)
- [Tamper guard](26-tamper-guard.md)
- [Missions](30-missions.md)
