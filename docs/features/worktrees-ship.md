# Worktrees that ship (ROADMAP 1-12: G1 + G3 + G6)

Branch `feat/worktrees-ship`, based on `fix/live-r9`, merged with `integration/r10`.

## What and why

- **Personas:** A (developers) and B (vibecoders). A separate copy now starts READY, and its work leaves as a
  pull request with the receipt as its text.
- **Agent-agnostic:** nothing here knows an agent's name. The copy, the setup, `gh` and the PR belong to the
  project and its branch. "Send back" goes through the same route as a second-opinion comment (a follow-up turn
  for a run, typed into the pane for a pane opened by hand).

## Design

| Piece | Where |
|---|---|
| Preset config (`[worktree] setup / copy / run`), setup runner, `conductor.json` import | `src-tauri/src/worktrees/preset.rs` |
| Bounded process runner, `PATH` and `PATHEXT` lookup (`npm.cmd`), `ToolEnv` for tests | `worktrees/proc.rs` |
| Create PR, checks, failure excerpt, send-back text, PR body | `worktrees/pr.rs` |
| "+" menu folder step: recents, git state, "Set up Git here" | `worktrees/folder.rs` |
| Service wrappers and Tauri commands | `worktrees/service.rs`, `commands/worktrees.rs` |
| Setup shown on the run receipt | `agent_runs/runtime/receipt.rs` (`ReceiptWorktree.setup`) |
| UI: project chip, Create PR sheet, PR block, Copies tab | `NewAgentMenu.tsx`, `CreatePrSheet.tsx`, `PrBlock.tsx`, `ProjectCopies.tsx`, `Changes.tsx`, `ProjectMemory.tsx` |

### Presets

`<project>/.puppet-master/project.toml`:

```toml
[worktree]
setup = [["npm", "ci"]]          # or "npm ci"
copy  = [".env", ".env.local"]
run   = ["npm", "run", "dev"]    # stored and shown; not started by Puppet Master yet
setup_timeout_secs = 600         # per step, optional
```

- Setup runs in the new copy, in order, as argv with no shell (`&&`, pipes, redirects are refused with a plain
  message). Each step's output goes to `<worktree>.setup.log`; the result (steps, exit codes, the last lines of a
  failing step, files copied and left out) is stored on the worktree and appears on the receipt and the Changes tab.
- It is never silent: the cockpit spawn screen says "Setting up the copy", then what ran.
- A failed setup keeps the copy and the agent still starts; the receipt lists "setting up the copy failed" under
  `not_verified`, and the Changes tab shows the failure.
- Copied files: skipped when tracked (the copy already has them), otherwise added to the repository's
  `info/exclude` unless an ignore rule covers them. Neither an agent's `git add -A` nor Puppet Master's own commit
  can put them in a branch. A path that leaves the project, `.git` and `.puppet-master` are refused.

### Where a new agent starts (Separate copy is OFF)

Since 2026-10-18 the "+" menu's **Separate copy** switch starts **off**, for every project. The agent works in
the folder the person selected, exactly (canonical path, no `.puppet-master` in it). Until then a Git project
started in a copy unless the person turned it off; that default was reversed on purpose.

- The switch stays. A person turns it on per agent, and the choice is remembered per project
  (`pm.separate-copy` in browser storage). Only a clicked switch is ever stored, in either direction, so there
  is nothing to migrate: a project nobody chose for reads as unset, i.e. off.
- The folder comes from the person: the picker, Recent projects, or the project of the selected pane. With none
  of those, "+" starts in the **last real folder** (the newest entry in the recents that the person used and that
  still exists), then in the open project. A separate copy's folder is always mapped back to its project
  (`projectOfCwd` in the UI, `worktrees::folder::real_folder` and `project_path::resolve_selected_folder` in Rust), so
  Recent projects, the project folder setting and the folder a new agent starts in never name a copy.
- The pane header says `separate copy of <folder>` when the pane works in a copy, and the folder's name (full
  path on hover) when it does not.
- Asking for a copy is still explicit everywhere else: `run_agent(isolation: "worktree")` (the default is
  `"none"`). These always work in a copy, by design: Missions, a Team update merge, Simple mode and the
  first-run welcome's first task (a task that must not be able to change the person's files).

### Create PR

`gh` only. `pr::status` (gh installed? signed in? a GitHub remote? a PR already?), then `pr::create`:
commit loose edits as one commit, push `refs/heads/pm/x:refs/heads/pm/x` with no force flag, `gh pr create
--draft --body-file`. Only `pm/...` branches are pushed, never the base or main. A diverged remote branch is
refused with a plain message. With a PR already open the same button becomes "Update PR" and only pushes.

### Checks back, Send back

`gh pr checks --json` (exit codes 1 and 8 still carry JSON), polled a bounded number of times from the Changes
tab (30 looks, each waiting up to 20 s in the backend). On a failure, "Send back to <agent>" first shows the
exact text ("last 40 lines, marked as output from GitHub", fenced as data, one line), and sends only after Send.

## Evidence

See `D:\pm-night\p0\worktrees-ship\REPORT.md`.

## Overlaps

`CockpitApp.tsx` is not touched. `Sidebar.tsx` (the "+" menu state), `useCockpit.ts` (`newAgent` takes a project),
`cockpit-api.ts`, `tray.rs` (`new_pane_in`), `commands/cockpit.rs` (`new_pane` takes `project_path`),
`second_opinion/route.rs` and `service.rs` (one function to deliver a ready message), `lib.rs` (command list),
`ProjectMemory.tsx` (tabs).

## Limits

- No visible setup pane: setup runs as a logged step (see decisions in the report).
- `run` is stored, not started. Warm worktree pool (G2) and phone Recent projects are not built.
- `gh` install and sign-in are explained, not automated (the doctor flow, 1-14, is another item).
