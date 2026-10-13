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
