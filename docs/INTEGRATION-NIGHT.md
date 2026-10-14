# Integration night (2026-10-14)

Branch `integration/night` (worktree `D:\pm-integration`) carries every verified night-shift branch on top of
`feat/ui-monochrome`. Backups: `backup/integration-night-start` (before round 1, `7651d54`) and
`backup/integration-round2-start` (after round 1). Nothing was pushed.

## Merged branches (head hash at merge time, merge commit)

Round 1:

| # | Branch | Head | Merge commit |
|---|---|---|---|
| 1 | `fix/tunnel-auth` | `6ba4cd0` | `8f6ba31` |
| 2 | `feat/agent-doctor` | `fbbbec6` | `42c3755` |
| 3 | `feat/approval-cards` | `ef2a423` | `96f6eb6` |
| 4 | `feat/loop-guard` | `4e00703` | `bfc13c3` |
| 5 | `feat/trust-fixes` | `5d86a6b` | `1346273` |
| 6 | `feat/account-profiles` | `6f3c071` | `7f4934e` |
| 7 | `feat/worktree-isolation` | `0472d14` | `a8eeca9` |
| 8 | `feat/ask-user` | `adb4977` | `3e9e87e` |
| 9 | `feat/checkpoints` | `bda1655` | `d74163a` |
| 10 | `feat/tunnel-push` | `2a5f46e` | `8a8dd8c` |

Round 2:

| # | Branch | Head | Merge commit |
|---|---|---|---|
| 11 | `feat/ui-monochrome` (stream offsets, reclaim fix, remount nudge) | `e4959fb` | `0ec633e` |
| 12 | `feat/ambient` | `655726a` | `c1b68ea` |
| 13 | `feat/toolbelt` | `246fe96` | `b5fe5e0` |
| 14 | `feat/one-brain` | `9b8aa0f` | `2d8caa7` |
| 15 | `feat/away-digest` (contains checkpoints) | `3ba902c` | `a20a626` |

Follow-up commits on the integration branch: `8d71d6f` (catalog pin), `e07b3ed` and `385c1d9` (test stubs), `7ac80dd`
(CRLF fixture), `43edd18` (CSS fix), `002d589`, `305d298`, `6ff3fc7` (evidence harness).

## Conflicts and how they were resolved

Almost every conflict was "two branches appended to the same registration point". Both sides were kept.

- `Cargo.lock`: took one side every time and let `cargo check` re-resolve. Never hand-edited.
- `Cargo.toml`: `tauri-plugin-global-shortcut` (loop-guard) and `keyring` (trust-fixes) both kept.
- `lib.rs`, `commands/mod.rs`, `bridge/routes/mod.rs`, `bridge/mod.rs`, `bridge/sse.rs`: all module lines, invoke handlers,
  route groups and re-exports kept. In `push_pane_status_sse` the hooks now run in this order: activity note, ambient,
  operation observer, checkpoints, push, turn summary. `push_operation_sse` keeps the push hook.
- `pane_activity.rs` tick: dev-server preview observer and loop-guard observer both run.
- `tui/profile.rs`, `declarative/profile.rs`, claude/codex/cursor/opencode profiles: `TuiProfile` keeps `account_scheme`,
  `activity`, `risk_rules`, `is_error_line`, `checkpoints`, `mcp_host`, `instruction_files`, `reprints_on_resize`.
- Declarative schema, validate and fingerprint: `risk`, `accounts`, `mcp_host` (+ `project_config_path`, `entry_http`),
  `instructions` all kept. Each table is added to the fingerprint only when declared, so a definition with none of them
  keeps its old fingerprint (the fingerprint tests are green).
- `PaneInfo` (Rust and TS): `account`, `account_label`, `title`, `reprints_on_resize` all kept. Every literal sets all of
  them.
- `run_agent` MCP schema: worktree-isolation's schema plus the `account` property. `entry.rs` keeps both the
  `ACCOUNT_MISMATCH` check and the isolation-on-adopt check. `assign_pane` keeps the account match and the
  "isolated runs never share a pane" rule.
- `tray.rs` / `new_pane`: one `new_pane_with(app, agent, origin, account, isolated)`. `new_pane_for` and
  `new_pane_for_account` are thin wrappers. The Tauri command takes `account_id` and `isolation`.
- Cockpit: the "+" menu keeps the accounts step and the "Isolated (worktree)" toggle; `newAgent(agent, account, isolated)`.
- `PaneView`: tabs are Terminal, Transcript, Receipt, History. Receipt holds Changes, Last turn and the receipt facts.
  The header keeps rename, project, account label, activity line, Preview chip and the stuck card.
- `Inbox`: questions first, then approval cards (with risk), then stuck cards.
- `Sidebar` / `Palette` / `CockpitApp`: Set up agents, Project memory, Stop all agents, Undo last turn, Toolbelt, preview
  entries; loop-guard, last-turn and digest providers nest in `CockpitApp`.
- Settings: General keeps loop guard, checkpoints, shared project memory and away digest; Agents keeps Accounts;
  API keys masked; Mobile PWA keeps Public address; MCP & bridge keeps Toolbelt.
- `mobile_pairing/auth.rs`: the fail-closed `requires_pairing_token` from `fix/tunnel-auth` and tunnel-push's public
  `/push/act` exception both present (tests in `mobile_pairing/tests.rs` and `tunnel_auth_tests.rs`).
- Two branches both added `MobilePairingStore::device_id_for_token`: removed the duplicate (compile error E0592).
- `cockpit.css`: both sides' blocks kept; the ambient merge left one rule unclosed, which broke the production build
  (`43edd18`). A brace-balance scan now passes.
- Test stubs: cockpit test doubles gained `asked: { questions: [] }` (ask-user made the sidebar and inbox read it).
- `one_brain` test used a CRLF-sensitive `replace` on a fixture; normalised (`7ac80dd`).

## MCP catalog

- Tools: **68**. Pin: `catalog_version_is_pinned` = `68-e4687e50f1687299` (recomputed after merging account-profiles,
  worktree-isolation and ask-user; round 2 did not change it).
- `run_agent` has `account` and `isolation`; `review_worktree` and `ask_user` are present.
- `docs/FEATURES.md` quotes the new pin. The per-feature docs still quote the pin from their own branch, on purpose.
- The installed MCP exe must be rebuilt before use (`npm run build --workspace=@puppet-master/app` does it, into
  `packages/mcp-server/dist`). Your installed copy is not touched.

## Results

```
npm run build --workspace=@puppet-master/shared        -> tsc -p tsconfig.json (no errors)
npx tsc --noEmit -p packages/app                       -> exit 0, no output
npm test --workspace=@puppet-master/app                -> Test Files 77 passed (77); Tests 578 passed (578)
cargo test --lib   (src-tauri)                         -> test result: FAILED. 1395 passed; 1 failed; 2 ignored
                                                          (only bridge::tests::definitions::approve_requires_app_session)
cargo test --lib visible_pane_launch_renders_tui_argv  -> ok. 1 passed
cargo test --bins  (src-tauri)                         -> ok. 33 passed (puppet-master-mcp); ok. 0 passed (app)
npm run build --workspace=@puppet-master/app           -> built (vite, cloudflared, PWA stage, Rust MCP release, bundles)
```

The production build rewrites `src-tauri/resources/*.bundle.*`; those were reverted with `git checkout --`, not
committed. Every `#[tauri::command]` (137) is registered in `lib.rs`.

## Screenshots (`D:\pm-night\evidence\integration`, 34, all opened and checked)

Run: `node scripts/evidence/screenshot.mjs --out D:\pm-night\evidence\integration` (headless).

| File | Shows |
|---|---|
| agent-doctor | "Set up your agents": Not tested / Needs login / Not installed / Problem rows with install command and Test |
| api-keys | Masked Anthropic key "Stored in Windows Credential Manager" with Replace/Remove; unset keys say where they go |
| approval-high | Inbox card: "Delete a folder outside this project", High risk, Deny focused, More options |
| approval-low | Inbox card: "Run tests", Low risk, plain reason, Yes / always / No |
| cockpit-accounts-menu | "+" menu second step: Default login, Work (Max), Personal, plus the Isolated toggle |
| cockpit-activity | Sidebar rows with activity lines ("Waiting: allow `npm test`?", "Editing src/app.ts") |
| cockpit-changes-merge | Receipt: Changes plus the "Merge into main?" confirm; Last turn below (light theme) |
| cockpit-changes | Receipt: Changes with an expanded diff, Merge/Discard, Last turn and receipt facts |
| cockpit-close-running | "Close Claude Code? It is still running", Keep running is the default |
| cockpit-digest | "3 agents worked while you were away" digest with Needs you list |
| cockpit-history-restore | History tab with the restore sheet (warns that another pane shares the folder) |
| cockpit-history | History timeline with turn labels, file lists and +/- counts |
| cockpit-inbox | Inbox with one approval card and the sidebar (Preview globe, Set up agents, Project memory) |
| cockpit-last-turn | Receipt tab: Changes, one-sentence Last turn, run facts |
| cockpit-light | Light theme Inbox |
| cockpit-memory | Project memory view: shared text, files agents read (CLAUDE.md, AGENTS.md, Cursor rule), share toggle |
| cockpit-new-menu | "+" menu: agents with ready hint and the Isolated (worktree) toggle |
| cockpit-palette-toolbelt | Palette filtered to "Toolbelt…" |
| cockpit-palette | Palette: panes, new agent entries |
| cockpit-pane-account | Pane header with the account label "Work (Max)" next to the project |
| cockpit-pane | Pane: tabs Terminal/Transcript/Receipt/History, activity line, approval bar, live terminal |
| cockpit-questions | Inbox: two agent questions (choices, free text) first, then the approval card; badge 3 |
| cockpit-receipt | Receipt tab with Changes, Last turn and facts |
| cockpit-rename | Pane title being renamed inline; auto titles in the sidebar; Preview chip |
| cockpit-titles | Auto-titled panes ("auth refactor", "fix flaky docs build") with agent and project beneath |
| live-preview | Pane header Preview chip (:5173) and the sidebar globe |
| loop-stuck | Stuck card ("same error 4 times", evidence, hint/interrupt/ask another/dismiss) above the approval bar |
| panic-sheet | "Stopped 1 agent" report after Stop all agents |
| settings-accounts | Settings > Agents > Accounts: logins per agent, Open login pane, Add account |
| settings-checkpoints | Settings > General scrolled to loop guard, Checkpoints, Shared project memory, Away digest |
| settings-mobile | Settings > Mobile PWA: Public address (Cloudflare tunnel, Online, Test connection) |
| settings-toolbelt-preview | Toolbelt "Apply to all agents" preview with per-agent adds/updates/removes and a masked diff |
| settings-toolbelt | Settings > MCP & bridge: Toolbelt server list, runtime status |
| settings | Settings > Agents default with Accounts and Definitions |

Harness changes made for the merged cockpit (dev-only code under `packages/app/src/dev`, `scripts/evidence`):
the virtual-time budget is 15 s, the harness lands CSS animations on their final frame (menus and sheets were caught
half faded), `get_mcp_status` is mocked (the Toolbelt page showed a red error from the null mock), and the checkpoints
scenario scrolls to its settings.

## Known issues

- `bridge::tests::definitions::approve_requires_app_session` still fails (pre-existing).
- Nothing here was run in the real app: no launch, no real phone, no Cloudflare, no push service, no real agent CLIs.
  Features verified by tests and mocked screenshots only. See each `docs/features/*.md` for its own limits.
- Ask-user questions do not trigger a Web Push yet (follow-up noted on `feat/ask-user`); the phone polls `/questions`.
- Push host allowlist suffixes for Apple/WNS are unverified; add with `PUPPET_MASTER_PUSH_HOSTS` if a subscription is refused.
- Several branches each ship their own settings blocks; the General tab is now long.
- Per-feature docs quote the catalog pin of their own branch, not the integrated one.
- The harness is mock-only; the animation freeze and the 15 s budget never ship (dev entry only).

## How to try it

1. Quit the installed Puppet Master first (single-instance lock, shared bridge ports).
2. `npm --prefix D:/pm-integration run dev`. This runs `tauri dev` from the worktree (first Rust build takes a while).
3. What to click:
   - **Security fix** (`fix/tunnel-auth`): nothing to click. After installing, restart the tunnel and re-pair devices if
     the URL may have leaked. `docs/security/2026-10-14-tunnel-auth-bypass.md`.
   - **Set up agents** (sidebar bottom, or Inbox on first run): check, Test, Open to log in, Copy install command.
   - **Approval cards**: have an agent ask to run a command; the Inbox/pane shows plain words and a risk level.
   - **Preview**: run a dev server in a pane; a globe appears on the row and a Preview chip in the header.
   - **Stuck warning / panic**: General settings has the toggles; palette "Stop all agents"; tray item.
   - **API keys**: Settings > API keys: set one, see it masked; check Credential Manager `com.puppetmaster.app`.
     Closing a running pane asks first.
   - **Accounts**: Settings > Agents > Accounts, add one, then "+" > agent > pick the login.
   - **Worktree isolation**: "+" > tick Isolated (worktree) > start an agent. Receipt tab > Changes > Merge / Discard.
     MCP: `run_agent(isolation:"worktree")`, `review_worktree`.
   - **ask_user**: an agent calls the `ask_user` MCP tool; the question appears first in the Inbox.
   - **Checkpoints**: History tab on a pane; Restore; palette "Undo last turn". Settings > General for the toggle.
   - **Tunnel + push**: Settings > Mobile PWA > Public address; enable notifications on the phone PWA.
   - **Ambient**: pane titles in the sidebar; click the title in a pane to rename; tray icon.
   - **Toolbelt**: Settings > MCP & bridge > Toolbelt (Import, Add server, Apply to all agents shows a preview first).
   - **Project memory**: sidebar "Project memory"; edit, Save, Sync now.
   - **Last turn and away digest**: Receipt tab "Last turn"; leave the desktop idle for the away time (General settings),
     come back to the digest.
   - **Terminal**: the lossless handoff and reclaim fixes are in the terminal; try taking a pane back from the phone.

---

# Round 3

Backup: `backup/integration-round3-start` (`5bdcfd3`).

## Merged branches

| # | Branch | Head | Merge commit |
|---|---|---|---|
| 16 | `fix/flaky-recovery-test` | `cbe2da5` | `11ac882` |
| 17 | `feat/settings-redesign` | `cfb1c06` | `f5968f8` |
| 18 | `feat/phone-restyle` | `a274958` | `48ff6bf` |
| 19 | `feat/ask-bar` (on top of loop-guard) | `b3e99d6` | `812a96f` |

Follow-up: `fix(cockpit): focusTerminal timer tolerates a torn-down document` (see below).

## Conflicts and how they were resolved

- **fix/flaky-recovery-test**: one conflict, the CRLF normalisation in `tui/declarative/tests/screen.rs` (both sides had the
  same fix). Kept one copy. `cargo test --lib` is now **0 failures** (1396 passed at this point).
- **feat/settings-redesign**: merged with no conflicts. Git carried the earlier branches' settings work into the new files;
  I checked it by hand: loop guard, away digest, checkpoints, shared memory, confirm-close and animated tray (General),
  Accounts and Preload (Agents), masked API keys and OpenCode keys (Keys), Toolbelt and runtime status (Bridge & MCP),
  Public address, notifications and pairing (Phone) are all in `components/settings/sections` and `blocks`.
  `settings/tabs` is gone, `saveSettings` is gone (`patchSettings` only), and `settings_patch` is registered.
- **feat/phone-restyle**: merged with no conflicts (phone harness route, sizes per scenario, screenshot script changes).
- **feat/ask-bar**: conflicts in `Cargo.toml`, `Cargo.lock`, `commands/mod.rs`, `lib.rs`, `Inbox.tsx`, `dev/harness.tsx`,
  `dev/mock-tauri.ts`, `dev/types.ts`, plus a modify/delete on the deleted `settings/tabs/GeneralTab.tsx`.
  - Kept both sides everywhere. The harness `route` is now `cockpit | settings | phone | askbar`.
  - `Cargo.toml`: a naive "keep both" put `arboard` under the Linux target table (compile error); it now sits in
    `[dependencies]`. `Cargo.lock` was re-resolved by cargo.
  - Inbox order: questions, ask notices, approval cards, stuck cards.
  - Settings: dropped `tabs/GeneralTab.tsx`. The Ask bar controls are `blocks/AskBarSettings.tsx` (ported to the new
    `SettingRow` / `SettingToggle`; it saves through its own `askbar_settings_*` commands and flashes the row tick) and one
    `RowDef` (`ask-bar`, group "Ask bar") in `sections/GeneralSection.tsx`: enable, shortcut, default agent, "Attach
    clipboard by default" (off). Its tests pass unchanged.
  - The `askbar` window is in `capabilities/default.json`. Its harness route and both scenarios are kept.
- **Flaky vitest error found and fixed.** About 1 run in 8, vitest reported `ReferenceError: document is not defined`
  (an unhandled error that makes `npm test` exit 1). Cause: loop-guard's `focusTerminal()` timer in `StuckCard.tsx` fired
  after jsdom was torn down. It now returns early when there is no `document`. 20 consecutive runs since: clean.

## MCP catalog

Unchanged: 68 tools, `68-e4687e50f1687299` (the pin test passes). None of the four branches touches the catalog.

## Results

```
npm run build --workspace=@puppet-master/shared        -> tsc -p tsconfig.json (no errors)
npx tsc --noEmit -p packages/app                       -> exit 0, no output
npm test --workspace=@puppet-master/app                -> Test Files 99 passed (99); Tests 854 passed (854); no errors
cargo test --lib   (src-tauri)                         -> test result: ok. 1432 passed; 0 failed; 2 ignored
cargo test --bins  (src-tauri)                         -> ok. 33 passed (puppet-master-mcp); ok. 0 passed (app)
npm run build --workspace=@puppet-master/app           -> built (vite, PWA stage, Rust MCP release, bundles);
                                                          tracked files under src-tauri/resources reverted, not committed
```

All 149 `#[tauri::command]` functions are registered in `lib.rs`.

## Screenshots (round 3: `D:\pm-night\evidence\integration-r3`, 50 PNGs, all opened and checked)

Run: `node scripts/evidence/screenshot.mjs --out D:\pm-night\evidence\integration-r3` (headless; phone scenarios at 390x844).
The 30 cockpit shots from round 2 are byte-identical to round 2 (same table above). New or changed:

| File | Shows |
|---|---|
| settings-general | Settings > General: Project, Appearance (animated tray), While agents work (confirm close, stuck warning, shortcut, away digest), Ask bar |
| settings-general-more | General scrolled: Undo and memory (checkpoints, keep N, shared memory), Storage, About, Uninstall |
| settings-checkpoints | General scrolled to the Ask bar rows (enable, shortcut, agent, clipboard off) and the groups around them |
| settings-light | General in the light theme |
| settings | Agents section: Accounts (logins per agent), Preload agents, agents to keep warm |
| settings-accounts | Agents section with accounts listed |
| settings-models | Orchestrator & Models: default orchestrator, OpenCode, API model and custom models |
| api-keys | Keys: masked Anthropic key in Credential Manager, OpenCode A/B keys, key automation, Remove all keys |
| settings-confirm | Keys with the in-app "Remove all keys?" confirm sheet |
| settings-search | Search "key": matching rows from General and Keys |
| settings-toolbelt | Bridge & MCP: Toolbelt servers, Bridge endpoint, Runtime status |
| settings-toolbelt-preview | Toolbelt "Apply to all agents" preview with per-agent changes |
| settings-bridge-more | Bridge & MCP scrolled: Install MCP runtime, Claude Desktop and Codex config, Uninstall MCP |
| settings-mobile | Phone: Public address (Cloudflare tunnel, Online), Notifications, typing delay, pairing |
| settings-later | "Coming later": nine not-yet-built items marked Not available yet |
| askbar-empty | The Ask bar window: agent chip, "Ask anything...", + Clipboard |
| askbar-answer | Ask bar with an answer, code block, Copy / Continue in a pane / Close |
| phone-agents-list | Phone: Agents list with the question card and Needs you / Working / Idle groups |
| phone-agents | Phone: question card and the "While you were away" digest |
| phone-menu | Phone menu sheet: connection, notifications, terminal options, Stop all agents |
| phone-confirm | Phone "Stop all agents?" confirm over the menu |
| phone-new-agent | Phone "Open an agent on your PC" sheet (Cursor Agent: Not installed) |
| phone-terminal | Phone terminal with the "Looks stuck" strip, the key panel and the command box |

## Known issues (additions)

- Nothing from round 3 was run in the real app either: Settings writes (`settings_patch`), the Ask bar window and global
  shortcut, and the phone restyle on a real phone are verified by tests and mocked screenshots only.
- The Ask bar adds the `arboard` crate (MIT or Apache-2.0) for the clipboard.
- The General page is long (Project, Appearance, While agents work, Ask bar, Undo and memory, Storage, About). Search finds rows.

## How to try it (additions)

- **Settings** (sidebar bottom): six sections (General, Agents, Orchestrator & Models, Keys, Bridge & MCP, Phone), a search
  box, and "Coming later". Every control saves as you change it and shows a small "Saved" tick; nothing needs a Save button.
  Try: flip a toggle, type "key" in search, and in Keys press "Remove..." to see the confirm sheet.
- **Ask bar**: Settings > General > Ask bar: turn it on, pick a shortcut (default Ctrl+Alt+Space) and an agent, leave
  "Attach clipboard by default" off. Then press the shortcut from any window, type a question, press Enter.
  "Continue in a pane" opens the answer in a normal pane. The app must be running (tray).
- **Phone**: pair as before (Settings > Phone) and open the PWA. New look: the Agents list with questions and the away
  digest on top, a bottom-sheet menu (notifications, terminal options, Stop all agents), an "Open an agent on your PC"
  sheet, and a "Looks stuck" strip on a stuck agent's terminal.
- **Tests**: `cargo test --lib` should now end with 0 failures.

---

# Round 4

Backup: `backup/integration-round4-start` (`18c3d0f`).

## Merged branches

| # | Branch | Head | Merge commit |
|---|---|---|---|
| 20 | `feat/followups` (question push, toolbelt secrets in the keyring, phone questions over SSE, `/agents/types` path redaction) | `1796721` | `fb600c1` |
| 21 | `feat/phone-lossless` (stream offsets on terminal, pane-resize and `/screen`; lossless phone handoff; `onGap` nudge) | `6ed77e4` | `df26258` |
| 22 | `feat/second-opinion` | `6206fa1` | `9448f0d` |

All three merged with **no conflicts**. I still checked the named hot-spots: `PwaApp.tsx` has both the questions SSE
(`questionsRev`, `ev.type === 'questions'`) and the `onGap` nudge; the checkpoint engine/runtime and `Changes.tsx` /
`PaneView.tsx` carry the second-opinion additions next to the earlier Changes and Last turn sections.

## MCP catalog

Unchanged: 68 tools, `68-e4687e50f1687299` (the pin test passes).

## Results (once, after all three merges)

```
npm run build --workspace=@puppet-master/shared        -> tsc -p tsconfig.json (no errors)
npx tsc --noEmit -p packages/app                       -> exit 0, no output
npm test --workspace=@puppet-master/app                -> Test Files 106 passed (106); Tests 920 passed (920); no errors
cargo test --lib   (src-tauri)                         -> test result: ok. 1476 passed; 0 failed; 2 ignored
cargo test --bins  (src-tauri)                         -> ok. 33 passed (puppet-master-mcp); ok. 0 passed (app)
npm run build --workspace=@puppet-master/app           -> built; tracked files under src-tauri/resources reverted, not committed
```

All 154 `#[tauri::command]` functions are registered in `lib.rs`.

## Screenshots (round 4: `D:\pm-night\evidence\integration-r4`, 52 PNGs, all scenarios)

Run: `node scripts/evidence/screenshot.mjs --out D:\pm-night\evidence\integration-r4`. 41 are byte-identical to round 3.
Changed or new, all opened and checked:

| File | Shows |
|---|---|
| second-opinion-changes | Receipt: Changes with review notes pinned to diff lines, "Send to Claude Code" per note, and the Second opinion panel ("Some concerns", Codex reviewed, an elsewhere note, Ask again / Dismiss) |
| second-opinion-picker | "Who should review?" sheet: Codex (ready) or Cursor Agent, optional "Also allow Claude Code", Start review |
| cockpit-changes | Receipt with Changes diff, Merge/Discard, a Second opinion button, Last turn |
| cockpit-changes-merge | Merge confirm over the same Receipt (light theme) |
| cockpit-receipt | Receipt: Changes, Second opinion button, Last turn, receipt facts |
| cockpit-last-turn | Receipt with the new Second opinion row above Last turn |
| cockpit-rename | Pane title being renamed |
| phone-agents-list | Phone Agents list with the question card (now fed over SSE) |
| phone-terminal | Phone terminal: "Looks stuck" strip, key panel, command box |
| settings-toolbelt | Bridge & MCP: Toolbelt copy now says secrets are kept in "your system's secure storage" |
| settings-toolbelt-preview | Toolbelt apply preview (secret values still masked in the diff) |

## Known issues (additions)

- Not run in the real app: question pushes, the keyring-backed toolbelt secrets, the lossless phone handoff and the second
  opinion reviewer need a real phone, Credential Manager and real agent CLIs.
- Second opinion asks another agent to read the worktree; its quality depends on the agent you pick.

## How to try it (additions)

- **Second opinion**: open a pane that ran in a worktree, Receipt tab > "Second opinion" > pick a reviewer (the reviewer can
  read but not change anything) > Start review. Notes appear on the diff lines; "Send to <agent>" hands a note back.
- **Questions on the phone**: an `ask_user` question reaches the phone over the live stream, and (with push set up) as a
  notification.
- **Toolbelt secrets**: Settings > Bridge & MCP > Toolbelt: tokens and passwords you add are stored in Windows Credential
  Manager, not in the settings file.
- **Phone terminal**: scroll and reconnect on the phone; output should no longer repeat or drop lines across a handoff.

---

# Round 5

Backup: `backup/integration-round5-start` (`f0e4760`).

## Merged branch

| # | Branch | Head | Merge commit |
|---|---|---|---|
| 23 | `feat/review-fixes` (11 commits: push Allow gated by risk, neutral ask_user with a countdown, palette undo through the restore sheet plus an Undo toast, runtime-only `.puppet-master` git exclude, one "Needs you" model in `lib/attention.ts`, sidebar identity, Second opinion above Merge, `--label-3` contrast tokens) | `99bda53` | `5215975` |

No conflicts. The worktree `D:\pm-review-fixes` was not touched (only the branch ref was merged).

## Flaky test found and fixed

`operations_contract_tests::recovery::startup_recovery_releases_locks_held_by_interrupted_operations` failed once in 17 full
`cargo test --lib` runs. Same class as the earlier recovery flake: startup recovery scans the whole process-wide operation
index, so it could see projects of tests running (and deleting their dirs) at the same time. Fix (`3eb5308`): the contract
tests' `Project` now sets a private, thread-scoped operation index (via a `#[cfg(test)]` `operations::set_test_index_dir`
hook), as `0d9f20c` did for the `operations::tests` recovery tests. 10 consecutive full runs since: all green.

## MCP catalog

Unchanged: 68 tools, `68-e4687e50f1687299`.

## Results

```
npm run build --workspace=@puppet-master/shared        -> tsc -p tsconfig.json (no errors)
npx tsc --noEmit -p packages/app                       -> exit 0, no output
npm test --workspace=@puppet-master/app                -> Test Files 111 passed (111); Tests 969 passed (969); no errors
cargo test --lib   (src-tauri)                         -> test result: ok. 1496 passed; 0 failed; 2 ignored  (10 runs in a row)
cargo test --bins  (src-tauri)                         -> ok. 33 passed (puppet-master-mcp); ok. 0 passed (app)
npm run build --workspace=@puppet-master/app           -> built; tracked files under src-tauri/resources reverted, not committed
```

## Screenshots (round 5: `D:\pm-night\evidence\integration-r5`, 53 PNGs, every scenario)

Run: `node scripts/evidence/screenshot.mjs --out D:\pm-night\evidence\integration-r5`. Almost every PNG differs from
round 4 because the `--label-3` tokens (dim text) got darker/lighter for contrast and the sidebar gained identity lines.
I opened the new one and a broad set of the changed ones (inbox, questions, approvals, Changes with and without second
opinion, palette, history restore, stuck card, digest, agent doctor, light theme, Ask bar, settings General and Phone, and all
six phone shots). No regressions.

| File | Shows |
|---|---|
| review-fixes-restore-toast | History after a restore: toast "Files restored to Before turn 2." with an Undo button |
| cockpit-inbox | "2 things need you": approval card and a "PowerShell hit an error" card (one attention model), sidebar "Needs you 2" with identity lines ("Error - puppet-master", "Waiting: allow `npm test`?") |
| cockpit-questions | Inbox with two agent questions (neutral wording, "stops waiting in 9 min" countdown), the approval and the error card; "4 things need you" |
| cockpit-changes | Receipt: Changes, Second opinion button above Merge/Discard, Last turn |
| second-opinion-changes | After a review: notes pinned to lines, Second opinion panel above the buttons, the primary button becomes "Merge anyway" |
| cockpit-palette | Palette with the sidebar "Needs you 2" and identity lines |
| phone-agents-list / phone-agents | Phone: question card with the "stops waiting in 60 min" countdown, "Needs you" rows |
| settings-*, askbar-*, phone-menu/-confirm/-new-agent/-terminal, cockpit-* (others) | Same screens as round 4 with the new contrast tokens |

## Known issues (additions)

- Not run in the real app: push Allow gating by risk, the Undo toast, and the runtime-only git exclude need real agents,
  a phone and real git repositories.
- `Cargo.toml` of `src-tauri` shows as modified in `git status` after builds (line endings only, no content change).

## How to try it (additions)

- **Needs you**: the sidebar group, the Inbox heading and the count now come from one model; approvals, agent questions
  and errors all appear in it.
- **Questions**: an `ask_user` question shows who is asking and how long the agent will wait ("stops waiting in 9 min").
- **Undo**: palette "Undo last turn" opens the restore sheet; after a restore an Undo toast appears for a few seconds.
- **Second opinion**: now sits above Merge. After a review with concerns, Merge turns into "Merge anyway".
- **Git**: `.puppet-master` runtime data is excluded from git, but `memory.md` and `agents/` stay shareable.

---

# Round 6 (live fixes)

Worktree `D:\pm-integration-live`, branch `integration/live`, cut from `93c8f2f` (the same point as `integration/night` at the end
of round 5). Backup: `backup/integration-live-start`. `D:\pm-integration` (the user's live app) was not touched. Rust was built
in its own target dir (`D:\pm-cargo-target-live`).

## Merged branches

| # | Branch | Head | Merge commit |
|---|---|---|---|
| 1 | `fix/live-standby` (per-agent standby backoff and cap, `agents_blocked` in StandbyStatus) | `f2885e0` | `a94f36d` |
| 2 | `fix/live-ui` (turn flicker and checkpoint numbering, side-panel crop, view precedence, mdc header, settings errors, `settle_status`) | `0ce53f6` | `2d64e31` |
| 3 | `fix/live-claude` (structural dialog detection, workspace_trust as a startup gate, `file_edit` risk) | `f5360c9` | `617742d` |
| 4 | `fix/live-codex` (`startup_gate`, `SubmitSequence`, `ComposerState`, read-only skip-git flag) | `52d63f8` | `6fab397` |
| 5 | `fix/live-hunt` ("Changes" tab for worktree panes, live status read in `checkpoint_pane_facts`, `set_active_and_persist`, registry refresh counter) | `f22f022` | `4201bd7` |

Follow-up commits: `d07ad02` (one startup-gate model), `ce0b1ea` (picker scenario).

## Conflicts

Merges 1 to 4 were textually clean. `fix/live-hunt` had two conflicts, both in `checkpoints/`:

- `runtime.rs`: the `Env` trait and `LiveEnv` kept both `turn_hint` (live-ui, one numbering for History and turn summaries)
  and `run_open` (live-hunt, names an open run precisely).
- `tests.rs`: kept both sets of tests.

`PaneView.tsx` merged cleanly. Tabs for a worktree pane: Terminal, Changes (with a count), Transcript, Receipt, History.
The Changes tab holds the diff, Second opinion, Merge and Discard. Receipt shows Second opinion only for a pane WITHOUT a
worktree, so it is never duplicated. `usePaneRegistry.ts` and `project_path` / `settings_patch` merged cleanly.

## The startup-gate reconciliation (the real work of this round)

Claude's branch made a folder-trust dialog a prompt kind that parks the run; Codex's branch added a `TuiProfile::startup_gate`
hook with dismissible and non-dismissible gates. They merged textually but ran side by side: a Codex trust or sign-in screen
parked the run with no card and no decline error, and Claude's profile did not report its gate through the hook. Now one model
(documented in `tui/README.md`, "Startup gates"; commit `d07ad02`):

- `TuiProfile::startup_gate(screen)` returns `StartupGate { id, summary, dismiss: Option<keys> }`.
- Dismissible notice (Codex update menu, `dismiss = Esc`): closed with the profile's keys before the task is typed, never a card.
  If it ignored its keys the run waits for the user.
- Only the user may pass it (Claude and Codex folder trust, Codex sign-in or onboarding): it becomes a prompt card that parks
  the run: kind `workspace_trust`, or `startup_screen` for other screens. The profile's own answerable card (Claude, Cursor:
  trust / deny with keys) wins; Codex gets a card with no choices (its keys are unknown, the user answers in the pane).
- Declining (the agent exits with the gate showing) fails the run: `WORKSPACE_TRUST_DECLINED` for trust,
  `STARTUP_SCREEN_NOT_COMPLETED` for a sign-in or setup screen (new). Neither retries, neither falls back to headless.
- Completion never happens while a gate shows (`completion.rs`), and `dispatch_blocker` never types over one.
- Claude now also reports its trust dialog from `startup_gate`, so completion and delivery see it too.
- New risk entry for `startup_screen`. `StartupGate::prompt_kind()` and `StartupGate::WORKSPACE_TRUST` are the only places
  that map gate ids to prompt kinds.

Both branches' tests stayed as they were and pass. New tests in `bridge/startup_gate_tests.rs` run Claude and Codex through the
same functions: every non-dismissible gate is a card and parks a run for any pane status; a dismissible gate is never a card;
declining Codex trust gives `WORKSPACE_TRUST_DECLINED`; a finished sign-in releases the run and an unfinished one gives
`STARTUP_SCREEN_NOT_COMPLETED`.

## MCP catalog

Unchanged: 68 tools, `68-e4687e50f1687299`.

## Results (final, after all five merges)

```
npm run build --workspace=@puppet-master/shared        -> tsc -p tsconfig.json (no errors)
npx tsc --noEmit -p packages/app                       -> exit 0, no output
npm test --workspace=@puppet-master/app                -> Test Files 115 passed (115); Tests 985 passed (985); no errors
cargo test --lib   (src-tauri)                         -> test result: ok. 1572 passed; 0 failed; 4 ignored  (3 runs in a row)
cargo test --bins  (src-tauri)                         -> ok. 33 passed (puppet-master-mcp); ok. 0 passed (app)
npm run build --workspace=@puppet-master/app           -> built; tracked files under src-tauri/resources reverted, not committed
```

The 4 ignored tests are the live-CLI tests (they spawn a real Codex or Claude), not run here.

## Screenshots (`D:\pm-night\evidence\integration-live`, 54 PNGs, every scenario)

Run: `node scripts/evidence/screenshot.mjs --out D:\pm-night\evidence\integration-live`. I opened the new and changed ones.

| File | Shows |
|---|---|
| cockpit-view-doctor-then-memory | New (live-ui, view precedence): opening Set up agents and then Project memory shows Project memory, not the stale doctor view |
| cockpit-changes | Worktree pane: tabs Terminal, Changes - 3, Transcript, Receipt, History; Changes holds the diff, Second opinion, Merge and Discard |
| second-opinion-changes | Changes tab after a review: notes pinned to lines, Second opinion panel, "Merge anyway" |
| second-opinion-picker | "Who should review?" sheet opened from the Changes tab (scenario fixed to click Changes, not Receipt) |
| cockpit-changes-merge | Merge confirm over the Changes tab (light theme) |
| cockpit-receipt / cockpit-last-turn | Receipt: Last turn and run facts only (Changes and Second opinion moved out) |
| cockpit-pane, cockpit-pane-account, loop-stuck, cockpit-history*, cockpit-palette, review-fixes-restore-toast | Same screens with the new tab bar |
| phone-agents-list, phone-agents, askbar-answer | Same screens, small layout/timing changes |

## Known issues (additions)

- Not run against the real CLIs in this worktree: the two live-Codex tests and the Claude live repro script
  (`scripts/evidence/live-claude-dialogs.mjs`) need the user's logins and spawn real panes.
- The Codex trust and sign-in screens are hand-written fixtures (no live capture), and a Codex trust card has no Trust / Don't
  trust buttons (the keys are not known); the user answers in the pane.
- Not in this round: Codex reply capture (a visible Codex turn's "result" is read from the whole screen), noted in
  `docs/features/fix-live-codex.md`.

## How to try it (additions)

- Run `npm --prefix D:/pm-integration-live run dev` (quit the other app first; it is a separate worktree from the one you use now).
- Start a visible Claude or Codex run in a folder the agent has not trusted: the Inbox shows a trust card and the run waits;
  answering (or trusting in the pane) lets it continue; declining ends it with a clear message.
- Worktree panes have a Changes tab (with a count): diff, Second opinion, Merge, Discard.
- Settings errors, standby (an agent that keeps failing to warm up backs off instead of retrying forever), and the side-panel
  crop of Claude's diff panel are fixed; see the three `docs/features/fix-live-*.md` files.
