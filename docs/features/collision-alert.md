# Collision Alert

Code: `packages/app/src-tauri/src/collision/` (the engine, the git reads, the words and the watch thread), started
from `pane_activity::init`. Desktop UI: `components/cockpit/CollisionSection.tsx` (Inbox),
`components/chat/CollisionBanner.tsx` (row above a chat), `components/settings/blocks/CollisionSettings.tsx`
(Settings > Agents), and `lib/collision.ts`. The file-by-file map is in `docs/REPO_MAP.md` section 3.18.

Status: **experimental, not live-checked yet.** Read this first.

- The whole feature is on the branch: the pure parts (limits, hunks, finish-first rules, noise, repo detection,
  comparing, alert life, words, view, git reads, one engine step), the watch thread, and the desktop cards, banner
  and switch.
- The four Tauri commands are in `commands/collision.rs` and registered in `generate_handler!` in `lib.rs`:
  `collision_list`, `collision_ignore`, `collision_settings_get` and `collision_settings_set`. So the card and the
  banner read alerts, `Ignore` works, and the Settings switch saves `collision.json`.
- The guard tests are in `collision/tests.rs` (8 source scans). The fake-agent end-to-end test is in
  `tests/fake_tui_collision.rs` (8 scenarios).
- Not checked yet: nothing has been run in the live app. The cost figures below are design targets, not measurements.
- The watch thread starts with the app. When two agent panes share a repo and one is working, it can raise a
  Ready-lane attention item (`collision:<id>`, no buttons). Turn the feature off with the Settings switch, or set
  `"enabled": false` in `collision.json` in the app data folder by hand.

## What and why

When you run two agents on one project, they can change the same code without knowing it. Collision Alert tells you
early, while both are still working, not at merge time. For example:

> Claude Code and Codex both changed src/auth.ts. Lines 40-80 overlap. In separate copies.

It only tells. It never pauses, stops, types into or merges anything. Nothing is written to disk except one on/off
flag. Alerts live in memory and are gone after a restart.

## Where you see it

- **Inbox**: a card for each real overlap (a clash, or two agents in one folder). Quiet "same file, far apart" rows sit
  under the heading `No clash yet`. The card is `CollisionSection`, under the Missions block.
- **Chat**: one row above the chat of each of the two panes, for a clash or a shared folder only.
- **Settings > Agents**: the switch `Warn me when two agents change the same file` (on by default).
- **Ready-lane item**: a clash or shared folder also raises one attention item in the Ready lane. The phone does not
  draw that lane (see Known limits).

## How it finds an overlap

1. Once a second, the watch thread reads the list of agent panes (no process) and works out which folders are git
   repos. Only agent panes count: no shell panes, no companion panes.
2. Two or more agent panes in one repo form a group. A group is looked at only while one of its panes is working, plus one look 2 seconds after a turn ends.
3. For each checkout with a working pane, the thread takes a sample: the current commit, the changed files compared
   with the checkout's start commit, and the new untracked files. Names, times and kinds only. No file content.
4. Every pair in a group is compared on the latest samples. This runs after a look, and only then. Changed lines are
   read only for files both sides changed, and only when both started from the same commit.
5. The alerts are updated in place (one per pair of panes). Cards and the banner show them.

### Cost

These are design targets from the spec. They were not measured in a live run yet.

| Situation | Work done | Cost |
|---|---|---|
| No agent panes, or one per repo | Each second: read the pane list (one lock). Each new folder: read `.git` (cached 60 s) | no process |
| Two or more panes in one repo, none working | Nothing. The last result stays | none |
| A group with a working pane | A first look 8 s after it forms, then one look every 20 s per checkout, and one more look 2 s after a turn ends | three read-only git calls per look |
| A look is slow | Next gap = the larger of 20 s and 20 times the look's cost, at most 120 s | |
| Three slow looks (over 3 s) in a row | Turn-end looks only, for 30 minutes | |
| A git call runs over 10 s | Killed. Counts as a slow look | |
| A file both sides changed | One `git diff -U0` per side, cached by (checkout, file, modified time) | a few ms, once per change |
| A git call fails (not a repo, or another error) | That checkout is left alone for 5 minutes | |
| Listing changed files | Stat with `symlink_metadata` (never follows a link). At most 2,000 listed, newest 400 kept | negligible |

Caps: 4 groups, 6 panes per group (oldest first), 20 compared files per pair, 6 alerts, 5 files shown per alert,
256 KiB of patch per file, 500 changed spans per file.

### Which changes count

A changed file counts for a pane only when it is the pane's own work:

- An edit or a new file, modified at or after the pane was created (minus 2 seconds).
- A deletion, only if the file was not already deleted when the checkout was first looked at.
- Never a noise file (see below), a file with control or invisible characters in its name, a name over 400 bytes, or
  a path that tries to leave the folder.

The start commit of each checkout is recorded at its first look. A separate copy uses the commit it was made from.

Noise that never counts, at any depth: folders `node_modules`, `target`, `dist`, `build`, `.next`, `.puppet-master`,
`.git`, `__pycache__`, `.venv`, `coverage`; names `package-lock.json`, `yarn.lock`, `pnpm-lock.yaml`, `Cargo.lock`,
`poetry.lock`, `composer.lock`, `Gemfile.lock`, `go.sum`, `.DS_Store`, `Thumbs.db`; endings `.min.js`, `.min.css`,
`.map`, `.log`, `.tmp`, `.swp`. Files in `.gitignore` never show. There is no user list in v1.

### Two copies of one repo (`Copies`)

Matched by file name (on Windows, ignoring case). For each file both sides changed:

| What both sides did | Level | Card text (detail line) |
|---|---|---|
| Both made a new file | clash | `Both made a new file with this name.` |
| One deleted it, the other edited it | clash | `{X} deleted it. {Y} changed it.` |
| Both changed the same lines (touching lines count) | clash | `Lines 40-80 overlap.` or `Line 40 overlaps.` |
| Both changed a file that is not text | clash | `Both changed a file that is not text.` |
| Too many changes to compare (over 500 spans, or patch over 256 KiB) | clash | `The change is big, so lines were not compared.` |
| One made a new file, the other edited the old one | clash | `The change is big, so lines were not compared.` (see Known limits) |
| Both changed, different places in the file | same file (quiet) | `Different places in the file. No clash yet.` |
| Both changed, but they started from different commits | same file (quiet) | `They started from different versions, so lines were not compared.` |

Both deleted: no overlap. A file that only changed its stat (no changed lines) is dropped.

### Two agents in one folder (`SameFolder`)

Both panes are `working` at the same time in one checkout. The window is from the moment both were working until now
(or until one stopped). Files in the checkout modified inside that window are listed. No line check. No file, no
alert (two agents reading in one folder is normal). The card says `They can overwrite each other. Changed while both
worked: {file}, {file}.` It never says who wrote which file.

A pane in a checkout that also holds another pane gets both kinds of pair: pairs in one folder are `SameFolder`,
pairs across copies are `Copies`.

### Which side to finish first

A fixed rule table, not a model. First match wins:

| # | Rule | Result |
|---|---|---|
| 1 | Exactly one side is not working | That side is first (`AlreadyDone`) |
| 2 | Separate copies only: a side has at most 70 percent of the other side's files, and at least 2 files fewer | That side is first (`SmallerChange`) |
| 3 | Otherwise: the side whose checkout started first. Equal times: the lower pane id | `StartedFirst` |

`SameFolder` skips rule 2. A same-file (quiet) alert has no advice line.

The advice sentence, with `{first}` and `{other}` as the pane names:

| Relation and reason | Sentence |
|---|---|
| Copies, stopped | `Finish {first} first. It has stopped working. Look at its changes and merge them first. Then {other} can build on it.` |
| Copies, fewer files | `Finish {first} first. It has changed fewer files, so it is quicker to finish. Merge it first. Then {other} can build on it.` |
| Copies, started first | `Finish {first} first. It started first. Merge it first. Then {other} can build on it.` |
| Same folder, stopped | `{first} has stopped working. Ask {other} to read the file again before it changes more.` |
| Same folder, started first | `Let {first} finish first. Then ask {other} to read the file again before it changes more.` |

## The words

Simple English, and the same on every card. Names are the sidebar headings (`Claude Code 2`), cut to 40 characters.
File names are cut to 120 characters.

| Where | Text |
|---|---|
| Title, one file | `{A} and {B} both changed {file}` |
| Title, more files | `... and 1 more file` or `... and {n} more files` |
| Title, same folder | `{A} and {B} are working in the same folder` |
| Place | `In separate copies.` or `In the same folder.` |
| File line label | `Lines 40-80`, `Line 40`, `New in both`, `Deleted in one, changed in the other`, `Not a text file`, `Different places`, `Different starting versions`, `Changed while both worked`, `Lines not compared` |
| Footer (every card) | `Nothing was paused. You decide.` |
| Buttons on a card | `Open {A}`, `Open {B}`, `Ignore` (pane buttons hide when the pane is gone) |
| Quiet rows | heading `No clash yet`, the title, the detail, `Ignore`, the footer. No advice line |
| Chat banner | `{Other} also changed {file} . {label} . Open {Other} . Ignore` |
| Switch | `Warn me when two agents change the same file` |
| Switch footer | `Puppet Master reads the list of changed files, and the changed lines when both agents changed the same file. It never pauses or stops an agent.` |
| Save failed | `Could not save. Try again.` |
| Phone (not drawn, see Known limits) | title, then the detail and the advice in one sentence, no buttons |

Every string that comes from a file name or a pane title goes through: remove control and invisible marks, write the
home folder as `~`, redact known secrets and token shapes, mask e-mail addresses, flatten to one line, then cut. The
view holds no folder and no process text.

## Alert life

- One alert per pair of panes. Its id is `ca-` plus 12 hex characters of a hash of the two pane ids (sorted). The same
  pair always has the same id.
- A look that finds the pair gives an update in place. A look that does not find it counts a miss. Two misses in a row,
  or one pane closing, resolve the alert.
- `Ignore` hides the alert and remembers its clash and shared files. It comes back only when a new file clashes for
  that pair. A same-file alert never comes back by itself.
- At most 6 alerts. Extra ones are dropped, quietest first.
- A clash or shared folder is raised as an attention item (key `collision:<id>`, Ready lane, no actions). Resolved
  items are cleared as `gone`, ignored ones as `withdrawn`. The window is told with the `collision://changed` event.
- Switching the feature off clears every alert and stops every git call. Nothing is probed while it is off.

## What it never does

- Never pauses, stops, interrupts, types into, presses keys in, merges, or discards anything. The collision code has
  no write path to an agent. Checked by the guard test `no_write_path`.
- Never reads file content. Only file names, modified times, kinds and line numbers come from git.
- Never runs a repo's own git hooks, config, filters or text converters. Every git call goes through the hardened read
  (`worktrees::harden::read`), which allows only `diff-index`, `ls-files`, `rev-parse` and `diff`, and refuses the
  rest.
- Never takes `.git/index.lock`. It sets `GIT_OPTIONAL_LOCKS=0` on every call.
- Never sends anything to another agent. No bridge route, no MCP tool, no team, no live room, no push, no Telegram.
  Outside the module, only `pane_activity.rs` (starts the thread), the Tauri command file and `lib.rs` (registers the
  four commands) name it. The guard test `not_reachable_from_agents` checks this.
- Never names who wrote a change in a shared folder.
- Never keeps alerts on disk. Only `collision.json` (`{"enabled": true}`) is saved.

## Known limits

- **Desktop only for now. The phone has no "Ready for you" list.** The phone's Needs you list draws only the Blocking
  lane. A Ready-lane item is never drawn there and never counted (`NeedsYouList.collision.test.tsx` pins this). So no
  clash card reaches the phone, and nothing is pushed.
- **Not live-checked yet.** The commands, the window and the switch are wired, but no run has been done with two real
  agents. See the status at the top.
- **Alerts can come late on a very busy PC.** When looks are slow, the watch backs off (the next look waits longer,
  up to 120 s, and after three slow looks only turn-end looks run for 30 minutes). A clash can then show after the
  agents have already edited the same lines.
- **Git repos only.** A folder without git is not watched. Folders without git are a v2 item.
- **Agent panes only.** Edits made by hand in an editor are not seen. Helpers and crew members count like any pane.
- **Same folder cannot say who wrote what.** It says "changed while both worked" and lists the files.
- **Lines are compared only when both sides started from the same commit.** Otherwise the card says "different
  starting versions" and does not guess.
- **One new file against an edit of the old one** counts as a clash, but the card says "The change is big, so lines
  were not compared." That sentence is not accurate for this case. A fix is a separate change to `copy.rs`.
- **Every pair is compared on each look.** The compare reads nothing new (the lines are cached), so the cost is the
  git calls of the look itself. An alert is only resolved after two looks in a row find nothing for its pair.
- **Noise rules are fixed.** There is no list the person can edit. Two agents both touching `package-lock.json` is
  not news, by design.
- **Alerts live in memory.** A restart clears them.
- **Costs are estimates** until a live run is done (the spec's live plan: two demo agents, the CPU and git process
  count over two minutes, the `index.lock` check, and a hostile `.git/config`).

## v2 (not built)

- `Hold {agent}'s next turn`: a person-only hold, checked by the outbox and by follow-up tasks. Never mid-turn. This is
  the only way to pause, and it needs its own design.
- Folders without git, using the checkpoint snapshots.
- A merge-time preview ("these two copies will conflict in 3 files"), using `git merge-tree` in the review sheet.
- Orchestrator view (`list_agents` shows overlaps), and declared `file:` resource locks as an explanation.
- A user ignore list, a line-range hint on the card (from `git diff`, never agent text), a sidebar dot.

## Tests

Not run in the docs pass. Run from `packages/app/src-tauri` (set `CARGO_TARGET_DIR` as in `docs/TEST-PLAN.md`):

- `cargo test --lib collision`: the module tests (model, hunks, advice, noise, repo, groups, compare, alerts, copy,
  view, probe, engine, live), the command tests in `commands/collision.rs` (id check, unknown id, list, settings round
  trip), and the 8 source-scan guards in `collision/tests.rs`.
- `cargo test --features dev-fake-tui --test fake_tui_e2e collision:: -- --test-threads=1`: the fake-agent end-to-end
  test (`tests/fake_tui_collision.rs`, 8 scenarios, included from `tests/fake_tui_e2e.rs`). Two fake agents
  (`pm-fake-tui`) run in real panes with the real watch thread and git probe. The scenarios cover: two copies that
  change the same line (one clash, nothing paused); the same file in different places (quiet row); different files
  (no alert); the advice when one agent ends its turn; two agents in one folder; Ignore and its return; the switch off;
  and a hostile `.git/config`, which must never run.

Run from `packages/app`:

- `npx vitest run src/lib/collision.test.ts src/components/cockpit/CollisionSection.test.tsx src/components/chat/CollisionBanner.test.tsx src/components/settings/blocks/CollisionSettings.test.tsx src/styles/cockpit-collision.css.test.ts src/components/mobile/NeedsYouList.collision.test.tsx src/dev/collision-fixtures.test.ts`

None of these were run in the docs pass. The live check (two demo agents, CPU and git process count over two minutes,
the `index.lock` check, a hostile `.git/config`) is still to do.
