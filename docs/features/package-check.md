# Package check: look before an agent installs a package

Code: `packages/app/src-tauri/src/prompt_risk/packages/` (offline: which packages a command names, look-alike names,
card sentences) and `packages/app/src-tauri/src/package_check/` (lookups: registry calls, cache, private-server check,
rows and view). UI: `components/cockpit/PackageFacts.tsx`, `components/settings/sections/PackageCheckRow.tsx`,
`lib/package-check.ts`. Spec: `SPEC-n33` (section 3 design, section 4 threats S1 to S16). Map: [`REPO_MAP.md`](../REPO_MAP.md).

Status: **experimental, not wired to the desktop yet.** See "Status on this branch" below.

## What it shows

An approval card for an install command ("Run `npm install lodahs`?") gets a **Package check** block with one row per
package the command installs. Two steps:

1. **At once, no network.** A look-alike name is flagged, for example `lodahs` looks like `lodash` (2 letters swapped).
   A look-alike name also raises the card to High risk, with the headline "Install a package with a look-alike name".
2. **Then, from the public registry.** The row adds facts: age, downloads, install scripts, deprecation, missing source link.

Facts and numbers only. The person decides. Package check never answers, allows, denies, or delays a prompt. It never
changes the buttons, their order, or the focus. Rows are fixed at first paint, so facts replace "Checking..." in place.

Example row text (every sentence is in `prompt_risk/packages/words.rs`):

- `Look closer`: `Looks like lodash (2 letters swapped)`, `New: first published 3 days ago`,
  `Few downloads: 12 a week`, `Runs a script when installed`, `Not found on npm. The name may be made up.`
- Plain: `First published 2 years ago`, `1,234,567 downloads a week`, `3 maintainers`, `No link to its source code`
- Not checked: `Installs from a folder or file on this computer. Not checked.`,
  `Not checked: this project uses its own package server`, `Not checked: Puppet Master can't look up winget packages yet`
- Could not check: `Could not check: npm could not be reached`, `... did not answer in time`, `... refused the request`

The header reads `Look closer · 1 of 3` when a row has a Look closer fact. The card shows at most 3 rows; the rest go to
Details as `+5 more in Details`. Words such as "safe", "verified", "trusted", "malicious", "approved" never appear
(a test scans `words.rs` and the component).

## Thresholds

All numbers are in `package_check/model.rs`.

| Fact | Threshold | Constant |
|---|---|---|
| New package | first published less than 30 days ago | `NEW_DAYS = 30` |
| Fresh version on an old package | newest (or pinned) version under 24 h old, package at least 90 days old | `FRESH_VERSION_HOURS = 24`, `FRESH_MIN_AGE_DAYS = 90` |
| Few downloads (npm, crates.io) | under 1,000 a week (crates.io: recent downloads / 13) | `FEW_WEEKLY = 1_000` |
| Few downloads (RubyGems) | under 10,000 in total | `FEW_TOTAL = 10_000` |
| Lookups per card | 8; the rest show "Not checked: too many packages in one command" | `MAX_LOOKUPS = 8` |
| Packages per command | 20 kept; the rest counted | `MAX_PACKAGES = 20` |
| Connect / one GET / whole card | 2 s / 4 s / 8 s (unfinished rows: "did not answer in time") | `CONNECT_MS`, `GET_MS`, `DEADLINE_MS` |
| GETs per package | at most 3 | `MAX_GETS_PER_PACKAGE = 3` |
| Answer size | 3 MiB for documents, 256 KiB for small answers; over the cap the fact is missing | `MAX_DOC_BYTES`, `MAX_SMALL_BYTES` |
| Requests per app | 60 a minute (fixed window, whole app) | `RATE_PER_MIN = 60` |

Cache (memory only, never on disk; cap 500 entries, oldest evicted): found 6 h, not found 2 min, failed 30 s.
"Check again" drops only the not-found and failed entries.

## Registries

| Ecosystem (tools) | Public registry | Requests | Facts used |
|---|---|---|---|
| npm (`npm`, `pnpm`, `yarn`, `bun`, `npx`, `pnpx`, `bunx`) | registry.npmjs.org + api.npmjs.org | 1. package document; 2. last-week downloads | created date, version dates, dist-tags, install scripts, deprecation, repository link, maintainer count (count only) |
| PyPI (`pip`, `pip3`, `python -m pip`, `uv`, `uvx`, `pipx`, `poetry`, `pdm`, `rye`) | pypi.org | 1. simple index JSON | first upload, newest non-yanked version, ready-built wheel or not |
| crates.io (`cargo add`, `cargo install`) | crates.io | 1. crate JSON | created date, recent downloads, newest version, yanked, repository link |
| RubyGems (`gem`, `bundle add`) | rubygems.org | 1. gem JSON; 2. versions JSON | total downloads, first version date, source link |
| Go module mirror (`go get`, `go install`) | proxy.golang.org | 1. version list; 2. oldest and newest `.info` | first version time, newest version time |

Not looked up: winget, Chocolatey, Scoop (shown as "Not checked"); local folders and files; `-r requirements.txt`
lists; links (only "Installs from a link, not from npm: host" is shown); names that look like secrets; names cut off
at the edge of the screen.

## Look-alike rules (offline)

Checked against the top lists in `prompt_risk/packages/popular/` (npm, PyPI, crates.io). A name that is itself on the list
is never a look-alike. Only list names of at least 4 characters are targets. Candidates are list names whose length
differs by at most 2. The first match in rank order wins.

| Shown as | Rule |
|---|---|
| only - _ . differ | npm and RubyGems only: equal after removing `-`, `_`, `.` |
| look-alike characters | equal after `0`->`o`, `1`->`l`, `5`->`s`, `3`->`e`, `rn`->`m`, `vv`->`w` |
| 2 letters swapped | one adjacent swap; name at least 5 characters |
| 1 letter different | one edit; name at least 5 characters |
| 2 letters different | two edits; name at least 10 characters |

## What is never sent

- Only the package name is put in a registry URL (Go also sends the version the mirror listed). A name is sent only when
  it matches the strict name rules for its ecosystem.
- Headers are exactly `User-Agent: PuppetMaster-PackageCheck/<version>` and `Accept`. No cookies, no authorization,
  no referer, no project path, no agent name, no pane id.
- No redirects are followed, no proxy is used (`try_proxy_from_env(false)`), and only `https://` URLs on the
  registry list are allowed. Anything else is refused before any socket opens.
- Transport error text is never logged (it would name the URL). Logs carry counts only.
- Names are never taken from the UI. The lookup re-reads the pane's live prompt by its `prompt_id`; a stale id is refused
  with `STALE_PROMPT` and makes no request.

## Project's own package server

No lookup (row: "Not checked: this project uses its own package server") when a name would come from a server other
than the public one. Checked in this order, reading only the lines named below and never storing or logging them:

- npm: the command's `--registry`; `.npmrc` `registry=` or `@scope:registry=` in the project or home folder;
  `.yarnrc.yml` `npmRegistryServer` / `npmScopes`; env `NPM_CONFIG_REGISTRY`. Lines with `_auth`, `_authToken` or `//` are skipped unread.
- PyPI: `--index-url` and similar flags; env `PIP_INDEX_URL`, `UV_INDEX_URL` and others; `pip.conf`, `uv.toml`, `pyproject.toml` indexes.
- crates.io: `.cargo/config.toml` with `replace-with`.
- Go: `GOPROXY` other than the public proxy (including `off`, `direct`); `GOPRIVATE` / `GONOPROXY` patterns.
- RubyGems: a `source` line in the `Gemfile` or `.gemrc` other than rubygems.org.

Puppet Master reads its own environment, not changes made inside a pane's shell (v2).

## Settings

Settings > General > Safety > **Check packages before installs** (on by default). Off means look-alike names are still
checked on this computer, and nothing is sent. The setting is stored in `<app data>/package-check/settings.json`
(private file). A missing or bad file means on. It is desktop only and not in the phone settings.

## Allowed commands (`allowed_commands`)

An install of a look-alike name, or of a package from a link, is never auto-approved by a run's `allowed_commands`
policy (`agent_runs/approval_policy.rs`, `hard_denied`). The card goes to the person. The policy only asks more; it
never approves more.

## Status on this branch (`feat/pc-docs`)

Built and unit-tested: parsing of install commands (npm family, pip/uv/pipx/poetry/pdm/rye, cargo, go, gem/bundle,
winget/choco/scoop), the look-alike check, the five registry readers (against fixtures, with a fake fetcher), the HTTP
client (against a local test server), the cache, the private-server check, the card sentences, the card block
(`PackageFacts`), the settings row, the `allowed_commands` refusal, and the packages field on the prompt.

Not on this branch yet:

- **The desktop lookup is not wired.** The UI calls `package_check_lookup`, `package_check_settings_get` and
  `package_check_settings_set`, but `commands/package_check.rs` does not exist and `lib.rs` does not register them (task PC-13).
  In the real app the lookup call fails, and the card shows "Could not check" under the offline rows. The offline
  look-alike still shows, because it comes with the prompt.
- **End-to-end test** with a fake registry server (task PC-14, `tests/fake_tui_package_check.rs`) is not written.
- **Live check** (spec section 6) has not been run. See `TEST-PLAN.md` section 2.13.

## Known limits

1. **A whole-program allowlist entry can approve a look-alike install.** If a Cursor run's `allowed_commands` lists a bare
   program such as `npm`, and the screen shows only Cursor's `Not in allowlist: npm` line (no full command line), the policy
   judges only the program name. The package names are never seen, so a look-alike install such as `npm install lodahs`
   can be approved automatically. The look-alike rule applies only when the full command is on screen. A possible
   fix, not planned yet: refuse bare-program entries for installs, or require the full command line for them.
2. **The look-alike lists are hand-written, not fetched.** The files in `prompt_risk/packages/popular/` say so in their
   headers ("hand-compiled, not fetched"). They were written from model knowledge, so their order is approximate. The npm
   list has 2,000 names, PyPI 1,786 and crates.io 881 (the spec asked for 2,000, 2,000 and 1,000). The script
   `scripts/package-check/popular.mjs` fetches the real lists (dev time, needs network); it has not been run.
   A miss or a wrong rank means a missed look-alike, not a false one.
3. **A late answer is dropped.** A registry call still running at the 8 s card deadline shows "did not answer in time".
   Its answer is discarded, not cached (the spec had said it could fill the cache). "Check again" asks again.
4. **No proxy support.** Behind a proxy every row says "could not be reached".
5. **Cut-off names are not checked.** The last name of a command that may be cut at the screen edge gets no look-alike
   and no lookup.
6. **Only names on screen are checked.** Packages the agent installs without showing the command are not seen.
7. **Phone, Telegram and push** show only the offline risk (headline and reason). Network facts are not on the phone.

## Not in v1 (spec section 3.14)

Check of lockfile and manifest changes after a turn (`tamper/`, Review Desk flag); network facts on the phone (bridge
route with caps); OpenCode native permission cards; Chocolatey lookups; reading `-r requirements.txt` lists; combined
names such as `lodash-utils`; maintainer-change signal; proxy support; pane-level environment for private-server
detection; look-alike lists for RubyGems and Go.
