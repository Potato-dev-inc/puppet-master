# Separate copies (git worktrees)

**Status:** Works today. Checked live with real agents. The setup presets were also checked live. Some small parts are tested only by automated tests.

## What it is
A separate copy lets an agent work on its own copy of your project. Your own files do not change while the agent works. The copy is a "git worktree". A git worktree is a second working folder that shares the history of your project but has its own branch (a named line of work). You look at the result later. Then you keep it or throw it away.

## Why it matters
Two agents in one folder can overwrite each other's files. One agent can also break your project while you watch something else. A separate copy removes both problems. Developers use it to run many agents at once. People who build with AI use it to try something risky. If they do not like the result, nothing is lost.

## How to use it
1. In the sidebar, click **+** to open the **New agent** menu.
2. Pick the agent and the project folder.
3. Turn on the **Separate copy** switch at the bottom. For a Git project that has at least one commit, this switch starts on. For other folders it is not available.
4. Start the agent. The screen says "Setting up the copy" while it prepares the folder.
5. When the agent is done, open the pane and click its **Changes** tab. See [Review, merge and pull requests](23-review-merge-and-pull-requests.md) for Merge and Discard.
6. In **Simple mode** (a Labs feature, off by default), the bottom bar has **Keep** and **Throw away**. Keep puts the changes into your own folder. Throw away deletes the copy. Both ask first.

**Presets (what a new copy gets).** Open **Project memory**, then the **Copies** tab. There you set:
- Setup commands, for example `npm ci`. They run in the new copy, one after the other.
- Files to copy, for example `.env`. These files are never committed to Git.
- A start command. It is saved and shown, but Puppet Master does not start it yet.

The same settings live in the file `.puppet-master/project.toml` inside your project, in a `[worktree]` section.

**From an agent.** An agent can call `run_agent` with `isolation: "worktree"`.

## How it works (simple)
- Puppet Master makes a folder under `<project>/.puppet-master/worktrees/` and a branch named `pm/<handle>`. The copy starts from your latest commit.
- The folder is added to Git's private ignore list (`.git/info/exclude`). Your `.gitignore` file is not changed.
- Setup commands run without a shell. Things like `&&` or pipes are refused with a plain message. Each step writes a log file. If setup fails, the copy is kept, the agent still starts, and the receipt says setup failed.
- When the agent finishes, the copy stays. Only Merge, Discard, Keep or Throw away remove it.
- **Empty copies clean up.** Sometimes a run never really starts. For example, you decline "Trust this folder", or the run fails, or you cancel it. Since 0.2.1, when you close that pane, the empty copy and its branch are removed. A copy that has any changes is never removed this way.
- At app start, Puppet Master also cleans up half-made folders. It never touches real work.

## Limits and what is not done yet
- The copy starts from your last commit. Changes you did not commit are not copied.
- A folder that is not a Git project cannot use this. You must run `git init` and make a first commit.
- The word to use is "isolated", not "sandboxed". A separate copy protects your files. It does not stop an agent from using your computer in other ways.
- Merge uses one method only (a merge commit). There is no squash and no per-file choice.
- Windows has a limit on path length. The copy folder names are short, but very deep paths inside a project can still be too long.
- Tools that scan the whole project folder can also see the copies under `.puppet-master/worktrees/`.
- Setup commands run as a logged step. There is no visible setup pane.
- The "start command" is stored only. It is not started for you.
- The sidebar does not yet show a badge for copies nobody reviewed.

## For developers
- Git layer: `packages/app/src-tauri/src/worktrees/mod.rs`, `git.rs`, `service.rs`, `preset.rs`, `proc.rs`.
- Empty-copy cleanup: `agent_runs/pane_close.rs` (`release_pristine_worktree`, `release_worktree_of_closed_pane`).
- MCP: `run_agent(isolation)` and `review_worktree` (in `tool_registry/catalog/agents.rs`).
- Bridge routes (app only): `GET /worktrees`, `GET /worktrees/{handle}/diff`, `POST /worktrees/{handle}/merge`, `POST /worktrees/{handle}/discard`.
- Preset file: `<project>/.puppet-master/project.toml`, keys `setup`, `copy`, `run`, `setup_timeout_secs`.
- UI: `components/cockpit/NewAgentMenu.tsx`, `ProjectCopies.tsx`; Simple mode in `components/simple/`.
- Error codes: `NOT_A_GIT_REPO`, `NO_COMMITS`, `GIT_UNAVAILABLE`.

## Related
- [Review, merge and pull requests](23-review-merge-and-pull-requests.md)
- [Budgets](24-budgets.md)
- [Missions](30-missions.md)
- [Second opinion](31-second-opinion.md)
