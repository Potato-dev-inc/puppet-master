# Review, merge and pull requests

**Status:** Works today. Changes and Merge were checked live. Create PR, PR checks and Merge when green are tested only by automated tests. No real pull request was made in a live check.

## What it is
When an agent works in a [separate copy](22-separate-copies-worktrees.md), its pane shows a **Changes** view. It lists the files the agent changed and shows the lines that were added and removed. From there you can **Merge** the work into your branch, make a draft **pull request** on GitHub (**Create PR**), or **Discard** the copy. A pull request (PR) is a request on GitHub to add a branch to another branch. A separate button, **Merge when green**, lives on the Team cards. It is a Labs feature.

## Why it matters
You should not have to trust an agent blindly. Changes lets you read the work before it touches your project. If you work with a team, a pull request lets other people check it too. Nothing in this feature changes your main branch without a clear click from you.

## How to use it
1. Open an agent that works in a separate copy. Click its **Changes** tab. This tab appears only for a pane that works in a copy.
2. Read the file list. Click a file to open its diff (the lines that changed). Added lines are heavier. Removed lines are lighter. There are no red and green colors.
3. Optional: run a [Second opinion](31-second-opinion.md). Notes appear on the lines. If there are concerns, the **Merge** button changes to **Merge anyway**.
4. Choose what to do. The buttons are off while the agent is still working.
   - **Merge**: a sheet says "Merge into <branch>?". Click **Merge** to confirm. The changed files go into your branch. The pane closes.
   - **Create PR**: a sheet shows a title and text taken from the receipt. Click the button to make a **draft** pull request. If a PR already exists, the button says **Update PR** and only pushes new commits.
   - **Discard**: a sheet says "Discard this separate copy?". Click **Discard** to delete the copy. This cannot be undone.
5. After a PR exists, the Changes view shows the GitHub checks. If a check fails, click **Send back to <agent>**. You first see the exact text, and it is sent only after you click **Send**.
6. **Merge when green** (Labs): turn on **Team** in Settings › General › Labs. A GitHub pull request then shows as a card in the Inbox. Click **Merge when green**, read the sheet, and confirm. GitHub merges the pull request when its own checks pass.

## How it works (simple)
- **Merge** first saves the agent's work as a commit on its branch. Then it makes a merge commit in your current branch. If any file conflicts, Puppet Master cancels the merge and tells you which files. Your project stays as it was, and the work stays safe on the copy's branch.
- Merge refuses when your project is not on a branch, is in the middle of another merge, or has unsaved edits to the same files. The message says so in plain words.
- **Create PR** uses the GitHub tool `gh`. It commits loose edits, pushes only the `pm/...` branch (never main, never with force), and runs `gh pr create --draft`. It needs a GitHub remote and a signed-in `gh`.
- **Checks** are read with `gh pr checks`. Puppet Master looks a limited number of times (30 looks of up to 20 seconds each).
- **Merge when green** runs `gh pr merge --auto` and pins the exact commit you saw. If anyone pushes first, GitHub refuses. Puppet Master never merges by itself here. The button appears only on cards made from your own `gh`, never on a teammate's message.
- A setting named **Let agents merge separate copies** (Settings › General › Safety) decides if an agent may merge its helper's copy. Today it is on. The founder may change this default. A person clicking Merge is never blocked by it.

## Limits and what is not done yet
- Create PR makes **draft** pull requests only. A live check saw only the "this project isn't on GitHub" message. It has not made a real PR yet.
- PR checks, Send back and Merge when green need real GitHub. They were not tested live.
- Merge has one method (`--no-ff` merge commit). No squash, no rebase, no per-file accept.
- The patch view is cut at 256 KiB. The file list is complete.
- Merge when green is part of Team (Labs), which is off by default.
- `gh` is not installed or signed in for you. The app explains what is missing.

## For developers
- Changes UI: `components/cockpit/Changes.tsx`, `CreatePrSheet.tsx`, `PrBlock.tsx`, `team/MergeSheet.tsx`, `team/TeamCard.tsx`.
- Git and PR code: `worktrees/mod.rs` (merge, discard), `worktrees/pr.rs`, `team/merge_green.rs`.
- Tauri commands: `worktree_diff`, `worktree_merge`, `worktree_discard`.
- MCP: `review_worktree(handle, action: diff|merge|discard)`. Agent merge is gated by `agent_authority/merge.rs` (`MERGE_NOT_ALLOWED`).
- Error codes: `ORIGIN_DIRTY_OVERLAP`, `MERGE_CONFLICT`, `ORIGIN_DETACHED`, `RUN_ACTIVE`.
- Settings key for authority: stored in `agent_authority.json` in the app data folder (not in `settings.json`).

## Related
- [Separate copies](22-separate-copies-worktrees.md)
- [Second opinion](31-second-opinion.md)
- [Missions](30-missions.md)
- [Security, bridge and devices](29-security-bridge-and-devices.md)
