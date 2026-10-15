# Settings redesign: six sections, autosave, partial patches

Branch `feat/settings-redesign` (worktree `D:\pm-settings-redesign`), cut from `integration/night` (`5bdcfd3`).
Backup before the refactor: `backup/feat-settings-redesign-0506`. Nothing was pushed, nothing was launched
(screenshots are headless Edge through the harness).

## 1. What and why

**Personas.** A (developers) want every knob and a search box. B (vibecoders) want sensible defaults and plain words.
C (non-technical) want to never see the word "save", never lose a setting, and never be able to destroy something by
accident.

**Story.** "I open Settings, flip a switch, and it is saved. If I close and reopen the window it shows what is really on
disk, even if I changed something from the cockpit in between. I find a setting by typing what it does. Dangerous
things sit at the bottom of their page behind a confirm sheet."

**What changed for the user**

| Before | Now |
|---|---|
| 22 tabs, 10 of them red "Planned" mock tabs | **6 sections**: General, Agents, Orchestrator & Models, Keys, Bridge & MCP, Phone. One quiet "Coming later" row at the bottom of the sidebar opens a plain read-only list. No badges. |
| Two save models (a footer Save button plus controls that save at once). No unsaved-changes guard. "Saved" never cleared. | **Everything autosaves.** No Save, Done or "Saved" footer. A quiet per-row "Saved" tick fades in and out. |
| The whole settings object was read, edited and written back, so a window holding an old copy wiped newer values (and `ui_mode`) | **Partial patches merged in Rust.** A control sends only the keys it changed. |
| Settings loaded once; a hidden-not-destroyed window showed stale state on reopen | Reloads on show, on window focus and on every `settings://changed`. |
| Search filtered the tab names | **Row search** (label + hint + keywords), grouped under each section |
| Native confirm dialogs for two destructive actions, none for the rest | In-app **confirm sheet** for every destructive action, in a last group of its own |
| Clamp-while-typing (typing "1" became 50) | Numbers clamp **on blur** |
| Red, green and amber tones in several blocks | Monochrome per `mockups/ui-proposal/STYLE.md` |

**Agent-agnostic.** Nothing here names an agent. Agent lists (Preload, Accounts, Definitions) come from the runtime agent
registry; the settings UI never matches an agent string.

## 2. Design

### Rust

| File | What |
|---|---|
| `src-tauri/src/settings_patch.rs` (new) | The `settings_patch(patch)` command. `merge_patch` (RFC 7386: objects merge, `null` removes a key, anything else replaces), `sanitize_patch` (drops `ui_mode`, which `set_ui_mode` owns, and the three provider keys when a credential store holds them), `write_atomic` (sibling temp file, then rename), `patch_file`, and `apply_core` (sanitize, merge under one process-wide lock, then mirror and emit). |
| `src-tauri/src/settings_patch/tests.rs` (new) | 11 tests (below). |
| `src-tauri/src/settings_store.rs` | `mirror_public_patch` (the bridge/mobile mirror without any window event; `patch_public_settings` now calls it). `away_minutes` joined `PATCHABLE_KEYS` and the defaults: **the Rust digest reads it from the public settings, where it was never stored, so the "Away digest after" setting never took effect** (found while porting the control). |
| `src-tauri/src/app_windows.rs` | Showing the already-open Settings window emits `settings://show` with the requested tab (the window used to just show, ignoring the tab). |
| `src-tauri/src/lib.rs` | `mod settings_patch;` and the command in the handler list. |

The command writes through the store plugin (`app.store("settings.json")`), because the frontend's `LazyStore` reads that
same in-memory store; editing the file behind its back would leave the plugin with a stale copy that its next autosave
writes over ours. The file is then written with `write_atomic` from the store's entries. The plugin's own debounced
autosave rewrites the same bytes a moment later (not atomic, but identical). If the store plugin is unusable it falls
back to a direct read-merge-write of the file. `settings://changed` carries the patch with secret keys removed; the
existing consumers (`useSidebarSettings`) only read `orchestrator_backend` and then reload, so the payload is compatible.

### Frontend

```
components/SettingsPanel.tsx          shell: sidebar (search, six sections, Coming later), pages, search results,
                                      confirm sheet, "Saved" ticks, reload logic
components/settings/model.ts          RowDef, SettingsEnv, groupRows, rowMatches, searchSections
components/settings/catalog.ts        SECTIONS (six) and LATER_ITEMS (the ten old placeholders, as text)
components/settings/sections/*.tsx    one file per section: a list of rows
components/settings/controls.tsx      Switch, Popup, TextControl, NumberControl, SecretControl, ConfirmSheet
components/settings/settings-ui.tsx   row blocks: SettingRow, SettingBlock, SettingToggle, InfoRow, Btn, Note, CodeBlock
components/settings/blocks/*          the reused composite blocks (Agent definitions, Preload, loop guard, tray, memory)
components/settings/SectionView.tsx   renders rows as grouped inset lists
lib/settings.ts                       patchSettings (replaces saveSettings), mergePatch
lib/settings-sync.ts                  subscribeSettingsRefresh, subscribeSettingsTheme
lib/settings-entry.ts                 ?tab= -> page, with the 20 old tab ids mapped to their new home
styles/settings.css                   the System Settings look (scoped under .settings)
```

A **row** is data: `{ id, label, hint, keywords, group, danger, render(env) }`. The panel gives `render` a `SettingsEnv`:
the fresh `settings`, `patch(partial)`, `markSaved()`, `setError()`, `confirm()`, the project path and bridge URL, and the
update-check state. A row never touches storage itself.

**When each control saves**

| Control | Saves | Notes |
|---|---|---|
| Switch, popup (select) | immediately on change | one key |
| Text | after a 600 ms pause, and on blur / Enter | `TextControl`; a pending edit also saves when the control unmounts (leaving a section or closing the window does not lose text) |
| Number | on blur / Enter, clamped | typing "1" on the way to "120" saves nothing; the field then shows what was actually saved |
| Secret (keys, tokens) | on blur / Enter only, then clears | `SecretControl`, `ApiKeyField`; never on a timer, never echoed |
| Controls with their own command (loop guard, tray icon, project memory, accounts, toolbelt, preload, definitions, MCP install, tunnel, OpenCode keys) | as before | they call `env.markSaved()` where a row exists, so the tick shows |

The public address (tunnel) keeps its explicit **Save and restart** button on purpose: saving restarts a network tunnel,
so it must not fire per keystroke.

**Stale-state rules in the panel.** The panel shows the optimistic value at once; while any save is in flight a reload is
deferred (so a change event cannot flicker a switch back), and one reload runs when the last save lands. A failed save
shows `Not saved: <reason>` on that row and reloads the real value.

**Fresh state.** `subscribeSettingsRefresh` reloads on `settings://changed`, `settings://show` (also navigates to the
requested tab) and on window focus / the page becoming visible. A theme saved from any window now also applies in the
cockpit and pane windows at once (`subscribeSettingsTheme`; this fixes UI-INVENTORY bug 10 for theme).

**Callers of the old whole-object save.** `saveSettings` is deleted so a whole snapshot cannot be written again.
The theme toggle, `useSidebarSettings`, `useProjectPath`, the workspace sidebar drag, the remote provider sync and
`App.tsx` now call `patchSettings` with only the keys they changed.

**Search.** Every word of the query must appear in a row's label, hint or keywords (case-insensitive). Results are rows
(live controls) grouped under their section in nav order; destructive rows stay in their own list.

**Destructive rows** (always `danger: true`, last list of their page, always `env.confirm()` first, Cancel has focus):
General > Uninstall Puppet Master, Keys > Remove all keys, Bridge & MCP > Uninstall MCP, Phone > Revoke all devices.
`catalog.test.ts` pins this exact set.

**OSS.** No new dependency. RFC 7386 JSON merge patch is implemented directly (about 20 lines). No code adapted from any
other project.

## 3. Control checklist (nothing lost)

Built by reading every control of every old tab file (kept for reference at `5bdcfd3:packages/app/src/components/settings/tabs/`).
"Test" names the test file that renders it and exercises the save.

### Old General tab
| Control | New home | Saves | Test |
|---|---|---|---|
| Settings file (path) | General > About > Settings file | read-only | GeneralSection |
| Export settings JSON (was on Advanced) | same row, "Export…" (keys redacted) | file download | GeneralSection |
| Default project folder (+ Browse) | General > Project | debounce / blur / Browse -> `onProjectPathChange` (typing now works; it did nothing before) | GeneralSection |
| Theme | General > Appearance > Theme | `{theme}` | GeneralSection |
| Loop guard: warn when stuck, sensitivity, quiet for too long, global stop-all shortcut | General > While agents work (row `loop-guard`) | own command `loop_settings_set` | GeneralSection |
| Checkpoints, Keep last N | General > Undo and memory | `{checkpoints_enabled}`, `{checkpoints_keep}` (blur, clamped) | GeneralSection |
| Shared project memory | General > Undo and memory | own command | GeneralSection |
| Away digest after (minutes) | General > While agents work | `{away_minutes}` (blur, clamped) | GeneralSection |
| App version, Check for updates, Download & install | General > About > Version | update check in `SettingsApp` | GeneralSection |
| Auto-save workspace, Semantic index, Auto session summaries, Reduce motion | dropped (planned only) | | catalog.test (no "Planned" wording) |

### Old Appearance, Sessions, Storage, Advanced, Developer tabs
| Control | New home | Saves | Test |
|---|---|---|---|
| Animated tray icon | General > Appearance | own command | GeneralSection |
| Sidebar width presets / custom | **dropped** (legacy orchestrator sidebar only, per the brief) | | |
| Default grid columns, Design language | dropped (planned only) | | |
| Ask before closing a working agent (Sessions) | General > While agents work | `{confirm_close_running_pane}` | GeneralSection |
| Default orchestrator (Sessions and Orchestrator showed it twice) | Orchestrator & Models > Orchestrator | `{orchestrator_backend}` | ModelsSection |
| Send initial prompt, Keep session history, Cost-aware router | dropped (planned only) | | |
| Coordination storage: scope, tasks, locks, event log, folder, Refresh | General > Storage | read-only | GeneralSection |
| "Project-local tasks and locks" switch | dropped: it had an empty `onChange` (UI-INVENTORY bug 3) | | |
| Require locks, Archive sessions | dropped (planned only) | | |
| Uninstall Puppet Master | General > last group, confirm sheet | `launch_uninstall` | GeneralSection |
| Scrollback limit, Spawn timeout, Telemetry | dropped (planned only) | | |
| Developer: Use bundled Rust MCP | Bridge & MCP > Bridge | `{developer_use_rust_mcp}` | BridgeSection |
| Developer: Bridge endpoint | Bridge & MCP > Bridge > Bridge endpoint (merged with the MCP tab's) | read-only | BridgeSection |
| Developer: Settings file (duplicate card) | merged into General > Settings file | | |
| Developer: "Bridge health response" sample | dropped: static fake JSON | | |
| Log MCP tool calls, Protocol inspector | dropped (planned only) | | |

### Old Orchestrator tab
| Control | New home | Saves | Test |
|---|---|---|---|
| Backend | (the single Default orchestrator row) | | ModelsSection |
| Librarian indexer | Orchestrator & Models > Orchestrator | `{librarian_indexer_path}`; blank is now possible (bug 14) | ModelsSection |
| OpenCode default model: provider ID, model ID | Orchestrator & Models > OpenCode (two rows) | `{opencode_model_provider}`, `{opencode_model_id}` (`null` when cleared) | ModelsSection |
| Default provider and model | Orchestrator & Models > Model for the API orchestrator | `{default_provider, default_model}`, `{default_model}` | ModelsSection |
| Custom models: list, Remove, Add | same group | `{custom_models}` (+ provider and model on Add) | ModelsSection |
| Cost-aware router, "Shared tool surface" card | dropped (planned / descriptive only) | | |

### Old API keys tab (Keys)
| Control | New home | Saves | Test |
|---|---|---|---|
| Anthropic, OpenAI, OpenRouter keys (masked, Replace, Remove, keychain note) | Keys > Provider keys | **on blur** to the keychain (`secret_set`), file fallback via `{<key>}`; per-key Remove asks first | KeysSection, ApiKeyField |
| OpenCode keys A / B, Refresh status | Keys > OpenCode keys | **on blur** (`set_opencode_key_profile`) | KeysSection |
| Key automation mode | same group | own command | KeysSection |
| Restart pane after key rotate | same group | own command | KeysSection |
| (new) Remove all keys | Keys > last group, confirm sheet | `secret_delete` x3 and `{<key>: null}` | KeysSection |
| GitHub token, Linear key, Validate keys | dropped (planned only) | | |

### Old MCP & bridge tab (Bridge & MCP)
| Control | New home | Saves | Test |
|---|---|---|---|
| Toolbelt (servers, add, edit, remove, switches, import, Apply to all agents, preview) | Bridge & MCP > `toolbelt` | own commands | BridgeSection (mount), ToolbeltGroup |
| Runtime status, Recheck & repair | `mcp-status` | `get_mcp_status` | BridgeSection |
| Bridge endpoint | `bridge-endpoint` | read-only | BridgeSection |
| Install in project / globally with results | `mcp-install` | own commands | BridgeSection |
| Claude Desktop JSON, Codex TOML snippets (Copy) | `snippet-claude-desktop`, `snippet-codex` | clipboard | BridgeSection |
| Uninstall from project / globally | `mcp-uninstall`, last group, confirm sheet (was a native dialog) | own commands | BridgeSection |
| Remote runners | dropped (planned only) | | |

### Old Mobile PWA tab (Phone)
| Control | New home | Saves | Test |
|---|---|---|---|
| Public address (mode, hostname, token, status, Save and restart, Restart tunnel, Test connection) | Phone > `tunnel` | own commands (explicit Save and restart) | TunnelSection |
| Notifications (tunnel-push) | Phone > `phone-notifications`: plain text; the toggle lives in the phone app | | PhoneSection |
| Mobile input buffer (ms) | Phone > Typing delay | `{mobile_input_delay_ms}` (blur, 0 or 50 to 1000) | PhoneSection |
| Keyboard resizes the agent, Show mobile input box | Phone > Phone app | `{mobile_keyboard_resizes_agent}`, `{mobile_input_visible}` | PhoneSection |
| Pairing: Public PWA URL, Local dev server port, QR, pairing code, Refresh QR, Paired devices, Revoke | Phone > `pairing` | `{public_pwa_url}` (pause / blur), `{dev_server_port}` (blur, 1024 to 65535) | MobilePairingPanel |
| (new) Revoke all devices | Phone > last group, confirm sheet | `revoke_paired_mobile_device` per device | PhoneSection |
| Require pairing approval | dropped (planned only) | | |

### Old Preload tab and Agents tab (Agents)
| Control | New home | Saves | Test |
|---|---|---|---|
| Preload master switch, agents to keep warm (+ count), idle timeout, status | Agents > Speed (`preload`) | own commands; idle timeout clamps on blur | PreloadSettings, AgentsSection |
| Accounts (per agent: rename, Open login pane, Remove, Add account) | Agents > Logins (`accounts`) | own commands | AccountsGroup, AgentsSection |
| Project consent banner, Definitions (Reload, Import, Add as generic agent, fixtures folder, Check, Export, Review and approve, Revoke), Drafts, Sources, Consented projects, Extra directories | Agents > `definitions` | own commands | AgentDefinitions, AgentsSection |

### The ten placeholder tabs
Security, Notifications, Backup & sync, Agent marketplace, Rules & guardrails, Cost controls, Team workspace,
Automation, Observability, Plugins: removed from the nav. They are ten lines of plain text under **Coming later**
("Not available yet"), with no controls and no badge (`SettingsPanel.test.tsx` asserts the page holds no input, switch
or button).

**"Ask bar".** The brief lists an "Ask bar" setting. This branch has no setting by that name: the controls that ask
before acting are "Ask before closing a working agent" (kept) and the Stop-all shortcut (kept). Nothing was dropped; if a
separate setting was meant, it is not on `integration/night`.

## 4. Evidence

Commands run from `D:\pm-settings-redesign` on the final commit.

### Rust
```
cd packages/app/src-tauri && CARGO_BUILD_JOBS=4 cargo test --lib
test result: FAILED. 1406 passed; 1 failed; 2 ignored  (the 1 is the known bridge::tests::definitions::approve_requires_app_session)
cargo test --lib settings_patch
test result: ok. 11 passed; 0 failed
```
New tests (`settings_patch/tests.rs`):
- `merge_patch_follows_rfc_7386`: nested merge, `null` removes, arrays replace.
- `sanitize_drops_ui_mode_and_optionally_secrets_and_rejects_non_objects`.
- `patch_file_merges_keeps_other_keys_and_creates_missing_files`, `an_unreadable_file_is_replaced_not_crashed_on`.
- **Atomic write:** `write_atomic_replaces_the_file_and_leaves_no_temp_files` (also a failed write removes its temp file).
- **Stale snapshot regression:** `two_stale_windows_do_not_clobber_each_other`. Two windows hold the same old copy and each saves one control. The control half of the test shows the old whole-snapshot model losing window A's change (`theme` goes back to `dark`); the patch half keeps both. Plus `a_stale_ui_mode_in_a_patch_never_reaches_the_file` and `concurrent_patches_from_many_threads_all_land` (16 threads, none lost).
- **Event:** `apply_core_emits_settings_changed_with_the_patch_but_never_a_key` (exactly one `settings://changed`, no `ui_mode`, no key; the bridge mirror also saw the patch), `an_empty_or_invalid_patch_emits_nothing`, `a_failed_write_reports_an_error_and_emits_nothing`.

### Frontend
```
npx tsc --noEmit -p packages/app                      exit 0
npm test --workspace=@puppet-master/app               Test Files 90 passed (90), Tests 789 passed (789)
npm run build --workspace=@puppet-master/app          tsc + vite build + Rust MCP build: ok (tracked bundle files reverted afterwards)
```
Settings tests (277 across 21 files):

| File | Tests | What it proves |
|---|---|---|
| `controls.test.tsx` | 15 | **autosave debounce** (nothing saved at 599 ms, one save after the pause), blur flush, mode "blur", unmount flush, number clamp on blur and not while typing, **secrets save on blur only and clear**, confirm sheet focus |
| `SettingsPanel.test.tsx` | 24 | six sections + one Coming later row; no Save/Done; **search** by label, hint and keyword, grouped by section, every word must match; **Coming later** is read-only (no input, switch or button); switches save one key, two controls never send each other's keys, the Saved tick shows and fades, a failed save shows an error and reloads, a change event mid-save does not flicker; reload on change / show / focus and tab navigation; confirm sheet (Cancel focused, action runs only after confirm) |
| `GeneralSection.test.tsx` | 15 | every General control renders and saves the exact patch; uninstall confirmed |
| `AgentsSection.test.tsx` | 17 | Accounts, Preload, Definitions render and call their own commands |
| `ModelsSection.test.tsx` | 14 | every control, exact patches, `null` to clear |
| `KeysSection.test.tsx` | 22 | masked keys, save on blur, OpenCode keys on blur, automation, Remove all keys behind a confirm |
| `BridgeSection.test.tsx` | 17 | status, install, snippets, Uninstall behind a confirm |
| `PhoneSection.test.tsx` + `MobilePairingPanel.test.tsx` | 11 + 6 | every control, clamps, Revoke all behind a confirm, list reloads |
| `catalog.test.ts` | 17 | six sections, unique row ids, every row has a hint, destructive rows are exactly the four named, no "Planned" wording, real-catalog search |
| `monochrome.test.ts` | 33 | no hue class in any settings source file |
| `lib/settings.test.ts` | 10 | `patchSettings` sends only changed keys, clamps, applies the theme, **two stale windows do not clobber each other**, never writes `ui_mode` |
| `lib/settings-sync.test.ts` | 4 | reload triggers; theme subscription |
| `lib/settings-entry.test.ts` | 4 | every old tab id maps to its new home |

**Fails before, passes after.** `lib/settings.test.ts` run against the previous `lib/settings.ts` (`git show 5bdcfd3:...`):
```
× patchSettings in the app > two windows holding the same old copy do not clobber each other
  → patchSettings is not a function          (10 of 10 failed; restored afterwards)
```
and the Rust `two_stale_windows_do_not_clobber_each_other` carries its own control: the old whole-snapshot model
loses the change in the same test that the patch model passes.

### Screenshots (headless, harness)
`D:\pm-night\evidence\settings-redesign\` (`node scripts/evidence/screenshot.mjs --out ...`):

| File | Shows |
|---|---|
| `settings-general.png`, `settings-general-more.png` | General top, and scrolled to storage, version, export and the Uninstall group |
| `settings.png` | Agents (Accounts, Keep agents ready, Agent definitions) |
| `settings-models.png` | Orchestrator & Models |
| `api-keys.png` | Keys, with the Remove all keys group last |
| `settings-toolbelt.png`, `settings-bridge-more.png` | Bridge & MCP top (Toolbelt, status) and bottom (install, snippets, Uninstall MCP) |
| `settings-mobile.png` | Phone (Public address, phone app, pairing) |
| `settings-search.png` | search "key": rows from several sections grouped under their section |
| `settings-later.png` | Coming later |
| `settings-confirm.png` | the confirm sheet (Keys > Remove all keys) |
| `settings-light.png` | light theme |
| `settings-checkpoints.png`, `settings-accounts.png`, `settings-toolbelt-preview.png` | the older scenarios, navigating by the new ids |

Harness scenarios now navigate by section id (`general`, `agents`, `models`, `keys`, `bridge`, `phone`, `later`); the dev
mock gained `settings_patch` (the Rust merge in miniature), `get_app_install_info` and `get_coordination_storage_info`.
The standby fixture was missing `config.preload` (the Preload block threw on it); it now has the real shape.

## 5. Overlaps with other branches (file level)

- **Any branch that adds a control to `settings/tabs/*Tab.tsx` will get a modify/delete conflict.** The tab files are
  gone: re-add the control as a `RowDef` in the right `sections/*Section.tsx` (see `GeneralSection.tsx`), saving with
  `env.patch({...})`. This is the main merge hot-spot.
- Any branch calling `saveSettings` (removed) must call `patchSettings({changedKey})`.
- `lib/settings.ts`, `lib/settings-entry.ts` (rewritten), `lib/secrets.ts` (`withoutSecrets` removed).
- `SettingsPanel.tsx`, `SettingsApp.tsx`, `App.tsx` (two lines: dropped the sidebar-width props, `patchSettings`).
- `hooks/useCockpit.ts`, `components/sidebar/useSidebarSettings.ts`, `hooks/useProjectPath.ts`, `TerminalApp.tsx`,
  `lib/orchestrator-provider-sync.ts` (+ test): `patchSettings`, and the theme subscription.
- `components/cockpit/Palette.tsx`: `openSettings('mcp')` became `openSettings('bridge')` (the old id still resolves).
- `components/MobilePairingPanel.tsx`, `settings/ApiKeyField.tsx`, `blocks/*`, `accounts/AccountsGroup.tsx`: restyled and
  autosaving.
- Rust: `settings_store.rs` (shared with `feat/checkpoints`' `PATCHABLE_KEYS` lines and the away-digest branch),
  `app_windows.rs` (one emit), `lib.rs` (two lines).
- Dev harness: `dev/mock-tauri.ts`, `dev/fixtures.ts`, `dev/scenarios/settings*.ts`, `api-keys.ts`.
- Docs: `docs/FEATURES.md`, `docs/REPO_MAP.md` (three lines each).
- The MCP catalog is untouched.

## 6. Limits and follow-ups (honest)

- **Not run in the real app.** No launch, no real window show/hide cycle. The `settings://show` emit, the store-plugin
  write path of `settings_patch` (`patch_via_store`) and the atomic write of the plugin's file are covered by unit tests
  of the pure parts (`apply_core`, `patch_file`, `write_atomic`) but the plugin glue itself has no test (the repo has no
  Tauri mock runtime). First thing to try in a real build: flip a switch, close the window, change the theme from the
  cockpit, reopen Settings.
- The store plugin's own debounced autosave still rewrites `settings.json` (same bytes, non-atomic) about 100 ms after our
  atomic write. A crash in that 100 ms window could truncate the file; the plugin offers no way to turn that off per
  write.
- Composite blocks (Agent definitions, Preload, Accounts, Toolbelt, MCP status) are single search rows with keywords, not
  one row per inner control, so search lands on the block.
- Blocks with their own commands show the "Saved" tick only where I wired `markSaved` (loop guard, tray icon, project
  memory, Preload, OpenCode keys, key fields). Accounts, Toolbelt and definitions show their own notes instead.
- Per-row error text appears under the row; there is no global toast.
- Bridge: config snippets still always show `npx` even when the Rust MCP switch is on (UI-INVENTORY bug 13, not part of
  this work). The Rust MCP switch and the install hint now agree because the switch autosaves.
- Phone > Notifications is a text row: there is no desktop-side notification setting in this build.
- `settings-search.png` and the other shots cannot show the transient "Saved" tick: the harness captures after animations
  settle (the tick is covered by `SettingsPanel.test.tsx`).
- `FEATURES.md` and `REPO_MAP.md` were updated for the new paths only; `docs/UI-INVENTORY.md` section 2 still describes the
  old window (it is a snapshot for the redesign planning).
