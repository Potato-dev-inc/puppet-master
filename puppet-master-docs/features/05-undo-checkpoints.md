# Undo and checkpoints

**Status:** Works today. Checked live in several runs, in folders with and without Git. A 3.8 GB folder gave one calm "Undo is off" card.

## What it is

Puppet Master saves a snapshot of your project files at the start and at the end of every agent turn. A snapshot is called a **checkpoint**. You can put your files back to any checkpoint. This works with every agent and in any folder. Git must be installed, but your folder does not need to be a Git project.

Undo changes your files only. It does **not** change what the agent remembers. The agent still knows the conversation.

## Why it matters

Agents sometimes break things. Developers get a safety net that never touches their Git history. People who build with AI and people who don't code can go back without knowing any command. The story is simple: "The agent broke my project. I open History, pick Before turn 3, press Restore. My files are back."

## How to use it

**Undo the last turn**

1. Open the agent and click **History**.
2. Press **Undo last turn** at the top. Or press Ctrl+K, type "undo" and choose **Undo last turn…** (an agent must be selected).
3. A sheet asks "Undo the last turn?". It says that your files are restored, the agent's memory is not changed, and files created since then are removed. Files in `.gitignore` are left alone.
4. Press **Undo last turn** to confirm, or **Cancel** (the focus starts on Cancel).
5. A message says "Files restored to ...". It has an **Undo** button that undoes the undo.

**Go back to any point**

1. In History, click a row to see which files changed.
2. Press **Restore** on that row and confirm. The same sheet and the same message appear.

**Good to know**

- If the agent is working, the buttons are off. Interrupt it first (Ctrl+K › "Stop this agent").
- If another agent works in the same folder, the sheet says so: its changes since then will be undone too.
- Your current files are saved first as "Before undo". That is why you can undo the undo.

**Settings** (Settings › General › Undo and memory): the **Checkpoints** switch (on by default) and **Keep last N checkpoints** (5 to 500, default 50, per agent).

## How it works (simple)

Puppet Master keeps its own private Git repository at `<project>/.puppet-master/checkpoints.git`. It points at your folder but never touches your own `.git` folder: your branches and history stay exactly the same (a test proves it).

A snapshot is taken:

- before a turn starts ("Before turn 3"),
- after it ends ("After turn 3"),
- before you answer an agent's question ("Before Claude Code's question (turn 3)"),
- before an undo ("Before undo").

At most one automatic snapshot per agent every 10 seconds. A snapshot of unchanged files is skipped. Files in `.gitignore`, the `.git` folder, `.puppet-master/`, and common big folders (such as `node_modules`) are left out. A file over 20 MB is left out, and Restore never deletes it.

A restore deletes files that did not exist then, and rewrites the others. It reports locked files one by one and restores the rest. `.puppet-master/` is kept out of your Git status, but `memory.md` and the `agents/` files stay shareable.

## Limits and what is not done yet

- If Git is not installed, History says "Git is needed". Nothing else breaks.
- If a folder has more than 1 GiB of new data, History says "Undo is off for this folder: it is too big. Add big folders to .gitignore." Puppet Master checks again every 5 minutes.
- Two agents in the same folder cannot be separated. A separate copy (see [Separate copies](22-separate-copies-worktrees.md)) makes it exact.
- A Git repository nested inside the project is saved as one entry, not its files.
- You cannot restore a single file, and there is no file diff viewer yet. You see a file list with counts.
- No phone screen for Undo.
- Plain shells have no checkpoints. Headless runs without a pane have no History tab.
- The snapshot "before turn" is taken just after the agent starts. An agent that edits in its first milliseconds could be caught half-way.
- Windows can lock files. Puppet Master retries once, then lists them.

## For developers

- Rust: `src-tauri/src/checkpoints/` (`engine.rs`, `git.rs`, `runtime.rs`, `http.rs`).
- Tauri: `checkpoints_list`, `checkpoint_diff`, `checkpoint_restore`, `checkpoint_undo_last`.
- HTTP: `GET /panes/{id}/checkpoints`, `GET .../{cid}/diff`, `POST .../{cid}/restore` (409 `PANE_RUNNING` while the agent works).
- Settings: `checkpoints_enabled` (true), `checkpoints_keep` (50). TOML agents opt out with `[features] checkpoints = false`.
- UI: `components/cockpit/History.tsx`, `RestoreSheet.tsx`, `UndoSheet.tsx`, `restore-flow.ts`, `history-model.ts`.
- Exclude rules: `git_exclude.rs` (writes a block in `.git/info/exclude`).

## Related

- [Receipts and History](04-receipts-and-history.md)
- [Command palette and ask bar](06-command-palette-and-ask-bar.md)
- [Simple mode](07-simple-mode.md) (its "Earlier versions" tab uses the same checkpoints)
- [Separate copies](22-separate-copies-worktrees.md)
