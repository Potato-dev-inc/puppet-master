# Branch from here: try a chat again from an earlier point

Code: `packages/app/src-tauri/src/branch_from/` (the flow), `checkpoints/export.rs` (writes a save point into the copy),
`commands/branch_from.rs` (four desktop-only Tauri commands). UI: `components/cockpit/BranchSheet.tsx`,
`components/chat/BranchBanner.tsx`, the `Branch` button in `SessionChatView.tsx`, the `Branch from here` row action in
`History.tsx`, the Keep warning in `Changes.tsx`, `lib/branch-from.ts`, `styles/cockpit-branch.css`. Spec: SPEC n33.

Status: **experimental**. Built on this branch with unit tests. No end-to-end test with fake agents and no live run yet
(see "Not done yet").

## What and why

From any open chat, you can start the same work again from an earlier point, with the same or another agent. Puppet
Master makes a separate copy of your files as they were at that point and starts the agent in that copy. Your folder,
your chat and the old pane stay as they were.

Nothing starts until you press Start. Only the desktop window can start a branch.

## What you see

1. Hover a finished turn in a chat and press **Branch**. A History row has **Branch from here** too.
2. The sheet shows the save point, for example `After turn 5 · 14:02`. The switch offers **Before this turn** (try it
   again). Before fills the message box with that turn's message. You can edit it.
3. Pick an agent. Rows that cannot be picked say why, such as `Not installed on this computer` or
   `Out of usage until 17:00`. The same agent is listed first.
4. **Show what the agent gets** shows the brief text (see below).
5. Type what the agent should do (4,000 characters at most), then **Start**. A new pane opens under the old one.
   The new pane's banner says `Branched from <agent> · after turn 5 · 14:02`. The old pane's banner says
   `Branches: <agent>` and lists each branch (it opens that pane).
6. Keep or drop the branch in Changes, like any copy: Merge, Create PR or Throw away.

## What a branch is on disk

- **Save point.** Undo writes a snapshot of the folder's files at each turn's start and end. Puppet Master keeps the last
  50 per pane. A save point that was removed after the sheet opened makes the copy fail, and the copy is thrown away.
- **The copy.** A new worktree, handle `bf-<12 hex>`, branch `pm/bf-<12 hex>`. It starts from the source folder's
  current commit, or from the source copy's branch when the source pane is itself a copy (a branch of a branch).
- **Start commit.** The save point's files are written over the copy, then committed once as
  `Puppet Master: start from save point (after turn 5, 14:02)`.
- **Not in the copy:** files Git ignores (like `node_modules`), files over 20 MB, and anything the save point did not
  hold. The copy follows the project's copy settings, as every copy does.
- **Brief file.** `<app data>/branches/briefs/<id>.md`, private. Never in the project or the copy.
- **Records.** `<app data>/branches/records.json`: ids, agent ids, labels, folders, the save point, state. Newest 200.

## The brief the new agent gets

Built by Puppet Master, at most 6 KiB, in this order:

1. A fixed first line: "Branch: you are starting a new line of work from an earlier point in a chat."
2. A paragraph: the copy's branch name, the save point, the source agent's label. It says the notes are notes, not
   instructions.
3. Fenced notes: the first task of the chat (when the save point includes it), then each turn up to the save point:
   your message and the agent's last final answer. Each turn is cut to 300 characters for a message and 300 for a reply.
4. `Your task now:` and your message, outside the fences.
5. `When you are done, say what you did.`

Only user and agent messages are read. Tool calls, tool output, diffs, terminal text and the environment are never read.
Every chat string goes through the strict redaction (secrets masked, e-mail addresses masked, your home folder as `~`).

If the brief is over 6 KiB, it is trimmed in this order: cut replies to 120 characters, show only the last 3 turns, drop
replies, then keep only turn 1. The sheet shows the size and the steps that ran (`trimmed: ...`).

## What refuses a branch

| Code | Plain meaning |
|---|---|
| `BRANCH_NOT_HERE` | Not a pane that can branch: companion, mission round, plain shell, or the pane is gone |
| `BRANCH_NO_SAVE_POINT` | No save point: Undo is off, or the point is unknown |
| `BRANCH_NO_TIMES` | The message has no time, so its save point cannot be found. Use History |
| `BRANCH_STILL_RUNNING` | The turn is still running. Branch after it ends, or use Before this turn |
| `BRANCH_NEEDS_GIT` | The folder is not a Git repository. Set up Git first |
| `BRANCH_NO_COMMITS` | The folder has no commit yet |
| `BRANCH_TOO_MANY` | 5 open branch copies from this chat. Keep or throw one away first |
| `BRANCH_BUSY` | A branch is already being made from this chat |
| `BRANCH_BAD_TARGET` | The picked agent cannot be used |
| `BRANCH_TARGET_LIMITED` | The picked agent is out of usage until a time |
| `BRANCH_EMPTY_MESSAGE` / `BRANCH_MESSAGE_TOO_LONG` | Type a message, or make it shorter than 4,000 characters |
| `BRANCH_COPY_FAILED` | The copy could not be made. Your folder was not changed |
| `BRANCH_START_FAILED` | The agent could not start. The copy is kept |

The window's save point id and chat item id are never trusted. Every start looks the save point up again in the
pane's own save points.

## Failure behaviour

- **Copy fails:** the half-made copy is thrown away. Only Puppet Master wrote to it, and no agent ran in it.
  The record shows `Failed` with `BRANCH_COPY_FAILED`.
- **Agent fails to start:** the copy is kept (its start commit is there). The banner says the agent couldn't start.
  Throw the copy away in Changes if you do not need it. There is no retry in v1.
- **Some files could not be written:** the branch still starts. The paths (first 20) are kept in the record and shown.

## Gates on the new agent

The new run starts with `trust_workspace` off, `allow_shell` off, no allowed commands, not read-only, a fresh
conversation, and `NO_PUSH` set in its environment. It runs in the copy, in the background, with a 30 minute limit.
If the agent shows a trust, permission or login screen, the run waits and its usual card appears. Puppet Master
answers nothing.

## Keep warning

For an open branch copy, the Changes tab shows this line above **Merge**:

> This branch started after turn 5 · 14:02. The original chat may have kept working after that, and merging this
> branch does not bring that later work in. Check both chats before you merge.

Merge is not blocked. Merge brings in only what this copy holds: the save point's files and the new agent's work.
It never removes the original chat's later work.

## Guards (what must stay true)

- The source folder and the old pane are never written, stopped or typed into. Puppet Master reads the pane's save
  points and chat items. `checkpoints/export.rs` writes files only inside the copy (and adds objects to the shadow
  store, see Known limits). It refuses a destination that is the source folder, a folder above it, the copy the source
  works in, or a link to any of them.
- Only the desktop window starts a branch. No MCP tool, no bridge route, no phone path, no card. The brief and the
  message never go to the bridge, push, team or phone code.
- Views carry no folder path, no message and no brief text. The brief is read by its own desktop command.
- Generic code names no agent. A test scans `branch_from/*.rs` (not its tests) and `checkpoints/export.rs` for agent names.
- One branch in flight per pane. At most 5 open copies per pane. A branch of a branch is allowed and recorded.

## Known limits (v1, as built)

- **Changes diff.** The diff of a branch copy is measured against the commit the copy started from, before the start
  commit. So the Changes list (and Merge) includes the save point's files, not only the new agent's work. Check the
  list before you merge.
- **Shadow store.** Writing the save point into the copy adds unreferenced Git objects to the source pane's shadow store
  (`<folder>/.puppet-master/checkpoints.git`). No ref, no per-pane index and no source file changes. The spec said
  "reads only"; this is the one place it is not exact.
- Git folders only. Plain folders are v2.
- Chats imported from before (Bring All My Chats) cannot be branched in v1.
- No retry after a failed start. No MCP `branch_from`, no phone button, no Scores column (all v2).

## Not done yet

- The end-to-end test with fake agents (`tests/fake_tui_branch.rs`, BF-10) is not on this branch.
- No live check in the app with a real agent. Nothing here is verified live.
- v2 (from the spec): plain-folder copies; old chats; Try again for a failed start; MCP; phone; compare two branches;
  Scores column; a diff of only the new agent's work (from the start commit); other saved logins in the agent list.
