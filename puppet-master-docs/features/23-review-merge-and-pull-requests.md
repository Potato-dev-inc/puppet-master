# Review, merge and pull requests

**Status:** Works today. Changes and Merge were checked live. Create PR, PR checks and Merge when green are tested only by automated tests. No real pull request was made in a live check.

## What it is
When an agent works in a [separate copy](22-separate-copies-worktrees.md), its pane shows a **Changes** view. It lists the files the agent changed and shows the lines that were added and removed. From there you can **Merge** the work into your branch, make a draft **pull request** on GitHub (**Create PR**), or **Discard** the copy. A pull request (PR) is a request on GitHub to add a branch to another branch. A separate button, **Merge when green**, lives on the Team cards. It is a Labs feature.

![The Changes tab of a separate copy](../images/23-real-changes.webp)

*Real run (0.2.1). The Changes tab of a separate copy: "Separate copy of main - 1 file - +3 -5", the changed file, and Merge, Create PR and Discard. It says "Not merged yet".*

![The terminal of the copy after the fix](../images/23-real-terminal.webp)

*Real run (0.2.1). Before the review: the agent in the copy fixed the bug and ran the tests. Claude Code's own diff panel is open next to its answer.*

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

![A file diff in the Changes tab](../images/23-real-diff.webp)

*Real run (0.2.1). Open a file to read the lines that were added and removed (src/cart.js).*

![The Merge into main confirm sheet](../images/23-real-merge-sheet.webp)

*Real run (0.2.1). Merge asks first: "Merge into main?" and "If anything conflicts, nothing is merged."*

![Changes details](../images/23-real-details.webp)

*Real run (0.2.1). Details in the Changes tab: the branch, what it is based on and the folder of the copy.*

![Right after a real merge](../images/23-real-merged.webp)

*Real run (0.2.1). Right after a real merge into main, the agent is closed and the Inbox is clear. In the demo repo, the log showed the merge commit and npm test passed (3 of 3). That was checked in a terminal, not in the app.*

![The Create a draft pull request sheet](../images/23-pr-sheet.webp)

*Example data. Create PR: title, text made from the receipt, and the base branch. It never overwrites a branch.*

![Create PR when GitHub CLI is missing](../images/23-pr-no-gh.webp)

*Example data. If the GitHub command line is missing, the sheet gives the commands to run by hand.*

![Create PR when the project has no remote](../images/23-pr-no-remote.webp)

*Example data. If the project has no GitHub remote, it says so and offers the commands.*

![A pull request with failed checks](../images/23-pr-checks.webp)

*Example data. After a PR is open: "2 GitHub checks failed" with "Send back to Claude Code" and Check again.*

![The Send back sheet](../images/23-send-back.webp)

*Example data. "See exactly what is sent": the failing job's last lines, marked as output, not instructions.*

![Merge when green cards](../images/23-merge-when-green.webp)

*Example data. Team cards (Labs) with "Merge when green" for an open pull request.*

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
