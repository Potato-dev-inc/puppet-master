# Review Desk

Status: **being built**. The engine and the seven desktop commands are on the branch and have unit tests. The desk
window is built but not mounted in the app yet, and there is no sidebar count, palette entry, phone route or phone card
yet (see "What is not wired yet"). The file map is in `REPO_MAP.md` section 3.14. Spec: `SPEC-n33-review-desk.md`
(outside the repo).

## What and why

Agents finish work while the person is away: a separate copy, a Night Shift task, an issue run waiting for its push,
two turns in the person's own folder. Today those are found in different places (the copies list, the Morning page,
the issue card, the panes).

Review Desk is one list of that finished work, riskiest first. Each row says why it is where it is, in plain words,
for example `Test files changed: cart.test.ts`. The person can Keep, Ask to fix or Throw away each item.

Rules that hold for every source:

- Every Keep and every Throw away is the person's click. Nothing is merged, pushed or thrown away by a timer, by a
  bulk action or by an agent.
- The order is a fixed table you can read (below). It is never a model's opinion or a score.
- Titles, reasons, file names and diff lines are plain text. Secrets are masked and the home folder shows as `~`.

## Where the items come from

| Source | What it is | Included when |
|---|---|---|
| Separate copy (`Copy`) | a git copy made for an agent run | the copy is `unreviewed` (the agent is done or its pane closed), and no Night Shift or issue run owns it |
| Mission (`Mission`) | a copy made by a mission | same as a copy; the title and the checks come from the last round |
| Night Shift (`Night`) | a task of the current Night Shift run | the task is still waiting for review and its class is Ready, No checks or Needs a look |
| Assign an issue (`Issue`) | an issue run | the run is waiting for the push check, or is held |
| In your folder (`Folder`) | turns of one pane that changed files in the person's own folder | the turn is idle, changed files, and ended after the last "Looks good" for that pane |

Only work that finished in the last 14 days is listed. At most 200 items are shown. A copy that a Night Shift task or an
issue run owns is shown once, under that owner. Panes linked as helpers are grouped under one header,
`<root pane title> and helpers`. The grouping is display only.

## The risk order

The order is one table. The item's tier is its riskiest flag. Inside a tier: more reasons first, then bigger change,
then older finish time, then the key.

| Tier | Flag | Shown as | When |
|---|---|---|---|
| 1 | Tests weakened | High risk | a snapshot from a mission or Night Shift run shows a blocking test change (copies with no snapshot never get this) |
| 2 | Sensitive | High risk | a file may hold a secret, a line looks like a secret, CI files changed, or a line has the home folder |
| 3 | Checks failed | Check first | some checks did not pass |
| 4 | Tests or check settings changed | Check first | a test file, a snapshot, or a settings file (such as `jest.config.js`) changed |
| 5 | Package list changed | Check first | `package.json`, a lock file, `Cargo.toml`, `pyproject.toml` and the other names in the fixed list changed |
| 6 | Big change, or unknown | Check first | 20 or more files, or 400 or more changed lines; or the changed files could not be seen |
| 7 | none | Low risk | none of the above (the row shows "No flags") |

The table is in `review_desk/risk.rs`. It reads only the recorded facts and the test-file roles.

## Keys (desk window only)

| Key | Does |
|---|---|
| `J` / `K` | next / previous row |
| `Enter` or `O` | open the row's files and diff |
| `Y` | Keep: opens the confirm sheet, or marks "Looks good" for a folder row |
| `F` | Ask to fix: a small sheet with an optional note and `Put in chat` |
| `X` | Throw away: starts the 10 second Undo window |
| `Z` | Undo the pending throw away, or the last "Looks good" |
| `P` | open the agent's pane |
| `Esc` | close the detail; again closes the desk |

Keys are ignored in text boxes and inside any sheet or dialog. `Ctrl`, `Meta` and `Alt` combinations are ignored.

## What Keep and Throw away do, per source

| Source | Keep | Throw away |
|---|---|---|
| Copy or mission | confirm "Merge into main?", then the normal merge (`--no-ff`; the copy's panes close). Same refusals as the Copies tab | the copy is discarded, after the 10 s Undo |
| Night Shift task | the same confirm, then the Night Shift keep | the Night Shift throw away, after the 10 s Undo |
| Issue, waiting for push | confirm "Push as draft", then the issue push check. Puppet Master opens a draft pull request and never merges | the issue run is thrown away, after the 10 s Undo |
| Issue, held | no Keep. The button shows "Held. Open Settings > Assign an issue." | the issue run is thrown away, after the 10 s Undo |
| In your folder | "Looks good": a mark only. Nothing is merged or changed. Undo for 10 s | the folder goes back to how it was before the first unlooked turn. Offered only when the pane is idle, a save point exists, and no other agent changed that folder since. Checked again at the moment of the throw. A "Before undo" safety point is taken first |

Every action carries the fingerprint of the item the person saw. If the item changed since, the action is refused with
"This changed since you looked. Look again."

## Ask to fix

Ask to fix puts a draft in the item's agent chat box and opens that pane. It never sends the draft and never types into
a terminal. The draft has only fixed sentences built from the item's reasons, and the person's own note (1,000 characters
at most):

```
Before I keep this, please fix:
- Test files changed: cart.test.ts
<the person's note, if any>
When you are done, say what you changed.
```

If the agent's pane is closed, Ask to fix is disabled with "The agent's pane is closed."

## Safety rules (SPEC n33 section 4)

- Only the desktop window can keep, throw away or fix. No MCP tool, bridge action, companion, Telegram channel or
  teammate reaches those actions (a source scan test checks this).
- The desk never writes to a pane and never answers a trust, permission or login question. It starts no agent.
- Review Desk redacts secrets and the home folder before any text leaves Rust. Its audit lines hold the action, the
  source, the item key, the file count and the tier. No titles and no paths.
- Ids are parsed strictly. A bad id is refused before anything is read.
- Work that reads git runs off the UI thread. Per-copy facts are cached for 60 seconds. Patches above 256 KiB are not
  scanned; the row then says only file names were checked.

## What is not wired yet

- **Desk window (UI-4, in progress):** the view, row, detail, Keep sheet and Fix sheet are built and tested as components.
  They are not mounted from the main window, the sidebar, or the palette yet.
- **Sidebar count and palette (UI-4, in progress):** the sidebar "Review" button with its count, and the palette entries
  "Review Desk" and "Review the riskiest item".
- **Phone route (RD-11, in progress):** `GET /review-desk` through the normal bridge auth, read only, MCP sessions refused.
  The view it returns is built (`review_desk/view.rs`, `phone_view`), with no route yet.
- **Phone card (UI-5, in progress):** `ReviewSummary` under the Night summary on the phone home, "4 to review · 1 high
  risk", up to five rows, no buttons.
- **Count path:** the sidebar count calls the full gather with the 60-second facts cache. The spec asked for a cheaper
  cached total; that is not done.

## Not in v1 (spec section 3.10)

Bulk Keep with one confirm; a snapshot for every copy at creation (so plain copies can show "tests were weakened");
headless in-place runs; Package Check facts; a "second opinion" button on a row; a Never Again hook after a throw away;
phone actions (Ask to fix only); a push digest line; a per-project filter; forks from Branch From Here.

## Tests

- Rust: `cargo test --lib review_desk` (also `cargo test --lib commands::review_desk`). Covers the risk table, the
  converters, gather and act on a fake desk (`engine_tests.rs`), marks, the view, and the source guards (`guard_tests.rs`).
- UI: `npx vitest run src/components/cockpit/review src/lib/review-desk src/styles/review-desk src/styles/review-detail src/dev/review-desk-fixtures`.
