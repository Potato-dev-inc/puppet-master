# The Agent Store ("More agents")

Status: **experimental**. Desktop app only, Windows only. The code is built and has unit tests. The live check has not
been run yet (see Known gaps, and `TEST-PLAN.md` section 2.11). The file map is in `REPO_MAP.md` section 3.13.

## What and why

Many people want to try more than one terminal AI agent. Each agent has its own install, sign-in and setup steps.
The Agent Store is a grid of agents. Each card shows:

- the vendor and the license,
- how to sign in,
- how well Puppet Master knows the agent (the tier).

A person can install the program, add it to Puppet Master, check that it works, and remove it again.

Two rules hold for every agent:

- Sign-in happens in the agent's own pane. Puppet Master never sees a password, key or code.
- An added agent gets the same Chat tab, question cards and helpers as the built-in agents.

## Decisions

Nobody could be asked while this was built. So each choice below is the safe one.

1. The catalog is data in this repo. It is built into the app. There is no download and no remote code.
2. One catalog entry is one folder. It has `entry.toml` (the card), `definition.toml` (the normal agent definition)
   and an optional `fixtures/` folder. A built-in agent has only `entry.toml`.
3. Two tiers, shown on every card in plain words: **Tested** and **Basic support**.
4. Install runs the vendor's own command. It is shown first, the person confirms it, and it runs in a visible
   PowerShell window. The order is winget, npm, uv, and the vendor script last. Puppet Master never uses `pip` on the
   system Python, never uses `curl | sh`, and never runs a script from a host the entry does not list.
5. No pinned versions. Vendors update themselves, and an old build can break sign-in. The card shows the version the
   facts were checked on. After an install it shows the real version.
6. Add writes the shipped definition and approves it, in one step, after the program is found. Only a file that is
   byte-for-byte the built-in copy can get the Store's approval. A later edit makes the approval stale.
7. A Store definition cannot set `[mcp_host]`, `[hooks]`, `[env] set`, `allow_interpreter`, or any flag on the wizard's
   deny list. The catalog check and Add both check this.
8. Sign-in belongs to the agent's pane. The Store never types into a pane, reads a credential, or writes a key or an
   environment variable.
9. Uninstall has two levels. "Remove from Puppet Master" always works and can be undone. "Uninstall the program" works
   only for a program the Store installed itself. Node.js, Git and uv are never uninstalled.
10. "Check it works" runs the wizard's real check and keeps only the numbers. It writes no file and approves nothing.
    It uses about 4 small messages on the person's own account, and it says so before it starts.
11. Windows only in this version. On other systems the cards show, with no main button.
12. Some short program names belong to other programs (for example `cn`, `pi`, `vibe`). Those entries are marked
    `ambiguous_name`. The Store asks the person to confirm the program it found before Add.
13. No telemetry and no network request of the Store's own. Only the vendor commands the person confirmed touch the
    network.
14. The catalog records the publisher (the repository URL) of each npm and PyPI package. A script compares it with the
    registry. This catches a name trap, where a similar name belongs to someone else.

## Tiers

| Chip | Tier | What it promises | What the catalog needs |
|---|---|---|---|
| Tested | `verified` | Real screens from this agent were captured, and the definition passes them. Chat and question cards are tuned for it. | Captures in `fixtures/` (idle, working, reply, each with an `.expect.toml`) that pass the check. A `[profile] tested` version range. `windows_tested = true`. |
| Basic support | `starter` | The agent can run in a pane. Chat is read from the screen. Question cards come from the shape reader. | A generic definition with `experimental = true`. No `fixtures/` folder. The entry text may not say "tested" or "verified". |

The detail sheet says "We have not run this on Windows ourselves." whenever `windows_tested` is false, for any tier.

Right now only Kilo Code meets every Tested rule. The four built-in agents (Claude Code, Codex, Cursor Agent, OpenCode)
also carry the Tested chip, but their `windows_tested` flag is still false. See Known gaps.

## Card states

The state comes from the catalog, the agent registry, PATH, approvals and the Store record. It is one of seven values.

| State | Meaning | Main button |
|---|---|---|
| `available` | Not on this PC, not added | Install (if a Windows install is listed), else How to install |
| `found_on_pc` | The program is on PATH, not added | Add |
| `ready` | Added and approved, program found | Open |
| `added_no_program` | Added, but the program is gone | Install |
| `needs_review` | The file changed, or the approval is stale or missing | Review |
| `own_version` | The person has their own definition with this id | none |
| `unsupported` | Not Windows | none |

Every main button opens the detail sheet first. The sheet holds the actions. The card shows one sentence about the
state.

Filter chips: **All**, **Ready**, **Open source**, **Needs a key**, **Tested**. "Needs a key" means sign-in kind
`api_key`. "Open source" means license kind `open_source`.

## What each action asks before it acts

### Install

1. In the detail sheet, press **Install <name>**.
2. The install sheet lists the steps. A Node.js, Git or uv step comes first when the method needs one. Each step shows
   the exact command and "This comes from <host>".
3. Press **Run**. The command runs in a visible PowerShell window. Puppet Master does not type into that window.
4. A script step needs one more tick: "I trust <host>". Run stays off until it is ticked. A script never runs
   straight after another step.
5. When the agent step ends, Puppet Master refreshes PATH and checks the program. It records how the program was
   installed. Then it adds the agent. For an ambiguous name it asks the person to press Add instead.

Asks first: **Run** (one confirm). A script also needs the tick. If a step fails, the sheet says so. If the program
is still not found after a successful install, the message says so. When a restart may help, it says to close and open
Puppet Master, so the new PATH is seen.

### Add

- The button is **Add to Puppet Master** for a program already on this PC. The sheet says that adding makes a file in
  the agents folder and does not change the program.
- For an ambiguous name the sheet asks "Is it <name>?" and the button is **Yes, add <name>**.
- Add refuses before it writes anything when the program is not on this PC (`ADD_PROGRAM_MISSING`), when an ambiguous
  name is not confirmed (`ADD_AMBIGUOUS_NOT_CONFIRMED`), or when a file with this id already exists
  (`AGENT_DEF_DUPLICATE_ID`). It never overwrites a file.
- It writes `<app data>/agents/<id>.toml`, and `<id>.fixtures/` for a Tested entry. It approves that file as a Store
  approval, then reloads the agent list. Settings > Agents then reads "Added from the Agent Store".

Asks first: one button. An ambiguous name also needs the yes.

### Open (sign in)

- **Open <name>** starts the agent in its own pane, the same way the Agent Doctor does. Sign in there if the agent asks.
- The Store types nothing. A sign-in or trust screen becomes a card in Needs you. The person answers it there or in the
  pane.

Asks first: nothing more.

### Check it works

- Only for a Ready agent. The sheet says what runs: the shipped definition, in the wizard's own pane, in an isolated
  temp folder. It says the check uses about 4 small messages on the person's own account.
- Press **Start**. **Cancel** stops it. The result line reads "N of M checks passed on version X".
- The wizard answers trust and allow-once questions only by its own rules, and only in its own temp-folder panes.
  A sign-in screen stops the check with "Sign in to <name>, then press Retry."
- If a repair round is needed and neither Claude Code nor Codex is installed to write it, the check stops and says so.
- Nothing is written to the agents folder. Nothing is approved. Only the numbers and the version go into the Store
  record.

Asks first: **Start**, after the cost sentence.

### Remove from Puppet Master

- The confirm sheet says that Puppet Master deletes its file for the agent and that the program stays on this PC. It also
  says the file can be put back from Settings > Agents for a while.
- Only a file that is byte-for-byte the built-in copy can be removed. An edited file is refused
  (`REMOVE_NOT_STORE_FILE`). The person removes that file in Settings > Agents.
- Removal uses the same recoverable path as Settings > Agents. The Store has no restore button of its own.

Asks first: one confirm.

### Uninstall the program

- The button is shown only when the Store's record says it installed the program with winget, npm or uv, and the current
  catalog entry names the same method and package. Otherwise there is no button. The sheet shows the reason in its place
  ("The Store did not install this program" or "This was installed another way").
- The removal runs in a visible window: `winget uninstall --id <id> -e --source winget`,
  `npm uninstall -g <package>`, or `uv tool uninstall <package>`.
- A script install is never removed by the Store. For an added agent with no Uninstall button, the sheet shows the
  vendor's removal note ("To remove the program: ...") when the entry has one.
- Node.js, Git and uv are never removed. Sign-in data and the agent's own sessions are never touched.

Asks first: one confirm (the button reads **Uninstall <name>**).

## The shipped agents (19)

Tier, license and sign-in come from each `entry.toml`. Install methods come from each `definition.toml`, or from the
built-in code profile for the four built-in agents. A built-in card is titled with its agent's own label (for example "Claude Code").

| Id | Card name | Tier | License | Sign in | Windows install | Notes |
|---|---|---|---|---|---|---|
| `aider` | Aider | Basic support | Apache-2.0 (open source) | API key | uv `aider-chat` (Python 3.12), vendor script | License from the planning table. The PyPI license field was empty. |
| `auggie` | Auggie | Basic support | Proprietary | Either | none, so How to install | The npm name was planned, but no install step is shipped. |
| `claude` | Claude Code | Tested chip, `windows_tested` false | Proprietary | Account (paid plan) | winget, npm, script | Built-in agent. |
| `cline` | Cline | Basic support | Apache-2.0 (open source) | Either | npm `cline` | npm lists no maintainer, so the check is by repository only. |
| `codex` | Codex CLI | Tested chip, `windows_tested` false | Apache-2.0 (open source) | Account | winget, npm, script | Built-in agent. |
| `continue` | Continue | Basic support | Apache-2.0 (open source) | Either | none, so How to install | Short name `cn`. Confirm before Add. |
| `copilot` | GitHub Copilot | Basic support | Proprietary | Either | winget, npm | Vendor says PowerShell 6 or newer is needed. License from the planning table. |
| `crush` | Crush | Basic support | FSL-1.1-MIT (source available) | Either | winget, npm | The winget id was not run with `winget show` yet. |
| `cursor_agent` | Cursor Agent CLI | Tested chip, `windows_tested` false | Proprietary | Account | vendor script | Built-in agent. Installs the Cursor Agent tool, not the editor. |
| `droid` | Droid | Basic support | Proprietary | Account | none, so How to install | License from the planning table. The vendor page had no Windows command. |
| `gemini` | Gemini CLI | Basic support | Apache-2.0 (open source) | Either | npm | |
| `goose` | goose | Basic support | Apache-2.0 (open source) | Either | none, so How to install | The repository moved to a new organisation. The vendor page gives no Windows command. |
| `kilo` | Kilo Code | Tested | MIT (open source) | Either | none, so How to install | The only entry with captures from a Windows run (kilo 7.8.8, checked 2026-10-09). |
| `kimi` | Kimi Code | Basic support | MIT (open source) | Either | vendor script only | The script needs Git for Windows. The planned npm package is not shipped. |
| `kiro` | Kiro | Basic support | Proprietary | Account | none, so How to install | The plan said Tested. It has no fixtures, so it is Basic support. |
| `mistral_vibe` | Mistral Vibe | Basic support | Apache-2.0 (open source) | API key | uv `mistral-vibe` (Python 3.12 or newer) | Short name `vibe`. Confirm before Add. The vendor says it targets UNIX. |
| `opencode` | OpenCode | Tested chip, `windows_tested` false | MIT (open source) | Either | winget, npm | Built-in agent. |
| `pi` | Pi | Basic support | MIT (open source) | Either | none, so How to install | Short name `pi`. Confirm before Add. The npm name differs between sources, so none is shipped. |
| `qwen` | Qwen Code | Basic support | Apache-2.0 (open source) | API key | npm `@qwen-code/qwen-code` (Node 22 or newer) | The planned script from a cloud bucket is not shipped. |

Counts: 5 entries carry the Tested chip (Kilo Code, and the four built-in agents). 14 are Basic support. 12 have a
Windows install command, so they show an Install button when the program is not on the PC. 7 show How to install.
Only Kilo Code has `windows_tested = true`.

Filter results from the entries: "Open source" shows 12 cards. "Needs a key" shows Aider, Mistral Vibe and Qwen Code.

### Not in the catalog

- **Amp, Grok CLI and Codebuff.** Their vendor pages did not confirm the facts the entry needs (install name, license
  or sign-in). They were left out on purpose.
- **Antigravity CLI (`agy`).** Not in this version. No confirmed install source was found.
- Other agents are not listed. A new entry must follow the steps below.

## Known gaps and honest limits

- **Basic support is not tested on Windows.** Most entries say `windows_tested = false`. Their install commands, sign-in
  sentences and screens have not been run on this PC. Test one on your own PC before you rely on it. Use Check it works.
- **The four built-in agents show the Tested chip** but their entries say `windows_tested = false`. The rule for Tested
  exempts built-in entries, so nothing stops this. Until it is fixed, the detail sheet says "We have not run this on
  Windows ourselves" next to a Tested chip.
- **"Tested" and "Basic support" are chips, not a promise.** Basic support means "it can run in a pane". It does not
  mean the vendor's install or sign-in steps were checked.
- **Proprietary licenses show "Paid terms".** That wording is one fixed label for every proprietary entry. It is not
  checked per vendor. Use the terms link on the card.
- **Five starter entries ship no install command** (Auggie, Continue, Droid, goose, Pi). Kilo Code and Kiro have none
  either. Their cards say How to install and link to the vendor.
- **Some install commands are partial.** Kimi Code has only the script. Qwen Code has only npm.
- **Copilot CLI** needs PowerShell 6 or newer, according to the vendor. The pane host is Windows PowerShell. Check it on
  your PC.
- **Kiro** is Basic support, not Tested as the plan said. It has no fixtures.
- **Winget ids not confirmed.** `astral-sh.uv` (in `agent_install/tools.toml`, marked UNVERIFIED), `charmbracelet.crush`
  and `GitHub.Copilot` were not run with `winget show`.
- **Licenses for GitHub Copilot, Droid and Aider come from the planning table.** They need checking against the vendor
  pages. The card shows the vendor's wording and a link, not our reading of it.
- **The "Newer than we tested" chip is not built.** The card does not warn when the installed version is outside the tested range.
- **The "Not tested here yet" sentence is not on the card.** The starter card shows the Basic support chip and the
  state reason only.
- **The end-to-end test is not on this branch.** The planned test with a made-up agent is missing.
  `npm run test:fake-tui` runs only the existing fake-TUI tests.
- **No live run yet.** The grid, Install, Add, Check, Remove and Uninstall have unit tests only.
- **The catalog check runs only as a test.** It is part of the Rust unit tests (`cargo test --lib`), which CI runs. The app does not run it at start.
  At Add time the app checks only the bytes of the file and the rule that forbids unsafe settings.
- **Rule 10 (warnings) and rule 11 (drift) are not shown in the app.** They are checked by tests only.
- **Versions are not pinned.** A vendor release can change how an agent behaves. The card shows the date and version
  the facts were checked on.
- **Check it works uses the person's own quota.** It costs about 4 small messages.

## How it is built

The file map is in `REPO_MAP.md` section 3.13. The main parts are:

- `agent_store/` (Rust): the catalog (embedded files), the card state, the record file, Add and Remove, the uninstall
  plan, and the catalog check.
- `commands/agent_store.rs`: the Tauri commands.
- `components/store/` (React): the grid, the detail sheet, the Check sheet, the install chain, and the host that owns
  which sheet is open.
- `lib/agent-store*.ts`, `lib/agentStoreCheck.ts`: the client and the pure rules for buttons, chips, search and sort.

Tauri commands (desktop only):

| Command | Does |
|---|---|
| `agent_store_list` | Every card, with the catalog version |
| `agent_store_detail(id)` | One card, plus the install plan, the run command, and the uninstall summary |
| `agent_store_add(id, confirmed_found)` | Adds the agent to the list (see Add) |
| `agent_store_remove(id)` | Removes the Store's own file |
| `agent_store_record_install(id, step)` | Records how the program was installed, from a fresh plan |
| `agent_store_uninstall_plan(id)` | The removal the Store would run. Runs nothing |
| `agent_store_uninstall_run(id, step, shown_command)` | Runs one removal step in a visible window |
| `agent_store_check_source(id)` | The definition and the fixture text for the Check run |
| `agent_store_record_check(id, passed, total, version)` | Saves the numbers of one Check run |

The Install steps use the existing `agents_install_*` commands. They also find a Store entry that is not added yet.

There is no bridge route and no MCP tool. The test `store_commands_are_desktop_only` checks that `bridge/`,
`tool_registry/` and `mcp_shim/` never name the Store. So an agent, the companion or a phone cannot add or install an agent.

Files on disk (paths are relative to the app data folder, shown as `<app data>`):

- `<app data>/agents/<id>.toml`: the added definition. Only the Store writes this file for a Store agent.
- `<app data>/agents/<id>.fixtures/`: the fixture files, for a Tested entry only.
- `<app data>/agent-store.json`: the Store record. It holds ids, dates, the catalog version (8 hex), the install method,
  package and version, and the last check's numbers. It holds no paths, no names and no screen text. A missing or broken
  file means "nothing recorded".

## How to add an agent to the catalog

1. **Pick an id.** Use lowercase letters, digits and `_`. Start with a letter. Keep it at 40 characters or fewer. It must
   not be the id of a code agent (`claude`, `codex`, `cursor_agent`, `opencode`).
2. **Read the vendor's own page.** Copy sentences only from the pages you name in `source`. Note the date and the version
   you checked. Use placeholders in examples, never a real name or path.
3. **Create the folder** `packages/app/src-tauri/catalog/agents/<id>/`. The folder name must equal the id.
4. **Write `entry.toml`.** Use the schema from `entry.toml` in the sections of this page. Set `schema_version = 1` and
   fill every table: `[store]`, `[vendor]`, `[sign_in]`, `[checked]`, `[publisher]`, `[uninstall]`. Keep the tagline at 90
   characters or fewer, with no web address. Start with `tier = "starter"`.
5. **Write `definition.toml`.** Copy a starter, such as `gemini`. Set `[agent]` with `id` (the same as the folder),
   `label`, `icon`, `kind = "generic"` and `experimental = true`. Set `[binary] names`. Add `[install.windows]` only if
   the vendor gives a command: `source`, `order`, and one of `winget`, `npm`, `uv` or `script_url`, plus a `note`.
6. **Fill the publisher.** Set `npm_repo` for each npm package and `pypi_repo` for each uv package. Set `script_hosts` for
   each script host.
7. **Do not add** `[mcp_host]`, `[hooks]`, `[env] set`, `allow_interpreter`, or any setting the wizard refuses.
8. **Starter: no `fixtures/` folder.** The entry text must not say "tested" or "verified".
9. **Tested only, after a real run on Windows.** Add `fixtures/` with the captures `tui_idle`, `tui_working` and
   `tui_reply` (or `headless_success` and `headless_error` for a headless-only agent). Each capture needs an
   `.expect.toml` next to it. Add `[profile] tested = "<version range>"` to the definition. Set `windows_tested = true`.
   Keep the captures free of names, paths and tokens.
10. **Run the generator** when you add or remove a file: `node scripts/gen-store-catalog.mjs` from the repo root. It writes
    `packages/app/src-tauri/src/agent_store/embedded.rs`. Commit that file. An edit to an existing file needs no
    generator run, because the text is compiled in on the next build. The Rust test
    `the_embedded_list_matches_the_catalog_folder_on_disk` fails if the file list is out of date.
11. **Check the catalog.** Run `cargo test --lib agent_store::check` in `packages/app/src-tauri`, with your own
    `CARGO_TARGET_DIR`. Fix every problem it names. The test names the entry and the rule.
12. **Check privacy.** Run `node scripts/check-no-personal-paths.mjs` from the repo root.
13. **Check the publishers before a release.** A maintainer runs `node scripts/store-verify-publishers.mjs` (it asks the
    npm and PyPI registries over the network). Keep the output.
14. **Add a line to the list** in this page, and to `CHANGELOG.md`.
15. **Try it.** Open the Agent Store, find the card, press Install (if listed), then Add, then Check it works.

## The 11 check rules

The check is in `agent_store/check.rs`. Every rule is an error, except rule 10, which is a warning. It runs as a test over
the embedded files. The app runs only the part that matters at Add time (rule 4, and the byte check).

| # | Rule | Refused when |
|---|---|---|
| 1 | Names | The folder name, `store.id` and `agent.id` differ. A code agent's id is used (unless `builtin = true`, and then it must be one). A built-in entry has a definition file. A file is listed twice or sits outside the folder layout. |
| 2 | Entry | `entry.toml` does not parse, has an unknown key, or fails validation (lengths, closed lists, https only, no control characters). |
| 3 | Definition | `definition.toml` does not load as a user definition or is too big. A starter is not `experimental = true`. The profile does not build. |
| 4 | Forbidden settings | The definition has `[mcp_host]`, `[hooks]`, `[env] set`, `allow_interpreter`, or any value the wizard's deny list refuses. |
| 5 | Install source | The install host is neither the vendor's homepage host nor a listed script host. A script URL host is not listed. An npm install has no `npm_repo`. A uv install has no `pypi_repo`. |
| 6 | Tested | The entry is `verified` but has no fixtures, lacks a capture or its `.expect.toml`, fails `check_dir`, has no `[profile] tested`, breaks a conformance rule, or has `windows_tested = false`. |
| 7 | Starter | A starter has a fixtures folder, or its text says "tested" or "verified" (tagline, vendor name, license, sign-in sentence, uninstall note). |
| 8 | Privacy | A catalog file holds a real-looking user folder, a consumer email address, or a token-shaped string. Placeholders are allowed. The value is never printed. |
| 9 | Size | A fixture is over 2 MB, an `entry.toml` is over 4 MB, or the whole catalog is over 8 MB. |
| 10 | Dates (warning) | `checked.date` is more than 180 days old, or the entry is deprecated with no `replaced_by`. Nothing is refused for this. |
| 11 | Copies | A catalog definition that has a copy in `tests/agent-definitions/` differs from it. Today that is `kilo` and `kiro`. |

## Publisher check: `scripts/store-verify-publishers.mjs`

- Reads every `catalog/agents/*/entry.toml` and `definition.toml`.
- For each npm package it runs `npm view <package> repository.url` and compares the result with `publisher.npm_repo`.
- For each uv package it reads the PyPI JSON API and compares `project_urls` with `publisher.pypi_repo`.
- Prints a table. Exit codes: 0 when every package matches. 1 for a mismatch, a missing publisher, a lookup error or a broken
  catalog file. 2 for a usage error.
- `--offline` reads the catalog and lists the packages without any network call. `--root <dir>` uses another tree (the tests use it).
- It is a network check. A maintainer runs it before a release. It is not in CI.

Why it exists: some names belong to someone else. For example, the npm name `kimi-code` is a third party's package, not
the vendor's. The check flags a package whose repository is not the one the entry records.

## Tests

Not run in the docs pass. Counts are static (the number of test functions in each file).

- Rust: `cargo test --lib agent_store` (about 110 test functions in the module, including the catalog check
  `the_shipped_catalog_has_no_problems` and the embedded-list test). Also `cargo test --lib agent_install`,
  `cargo test --lib tui::install` and `cargo test --lib tui::declarative` (the approval changes).
- The check on its own: `cargo test --lib agent_store::check`.
- UI: `npx vitest run src/components/store src/lib/agent-store src/lib/agentStoreCheck` in `packages/app`.
- Scripts: `node --test scripts/gen-store-catalog.test.mjs scripts/store-verify-publishers.test.mjs`.
- Generator freshness: `node scripts/gen-store-catalog.mjs --check`.

The manual cases are in `TEST-PLAN.md` section 2.11.
