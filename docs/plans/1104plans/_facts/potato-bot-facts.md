# Potato Bot facts (read-only survey)

Repo: `C:\Users\Ren-pc\Desktop\work2\puppet-master`, branch `main`, read on 2026-11-04. Nothing was built, run or changed.
Paths are relative to the repo root unless they start with `D:/`. Line numbers were read from the files as they are now.
Grep hits inside `.claude/worktrees/` were ignored (stale copies). Items that could not be verified are listed in section 11.

---

## 1. Where Potato Bot lives

### 1a. Rust: `packages/app/src-tauri/src/potato/` (the core, one folder)

Module doc: `potato/mod.rs` (declares `pub mod` for every file below; tests are `*_tests.rs` files).

| File | Purpose | Key exported names |
|---|---|---|
| `potato/mod.rs` | Folder map; declares modules | `pub mod` list only |
| `potato/service.rs` | The engine: sends, bots, state, brain events, delivery | `trait Host` (l.62), `struct BotProfile` (80), `enum Blocked` (89), `enum BrainState` (101), `struct PotatoState` (160), `struct SendOutcome` (209), `struct Service` (355), `Service::new`, `Service::send` (686), `send_as` (692), `start_bot` (838), `stop_bot` (860), `stop_all` (879), `set_enabled` (937), `state` (1020), `tick` (1317), `inbox_poll` (1388), `ingest` (1406), `on_event` (1421), `usage_report` (1640), `SECRET_DEEPSEEK` (const, l.49) |
| `potato/live.rs` | The one running Service and its thread; Labs gate; DeepSeek check | `LABS_KEY` (l.31), `SANDBOX_LABS_KEY` (l.33), `CHANGED_EVENT = "potato://changed"` (l.34), `service()`, `bridge_base()`, `labs_on`, `sandbox_labs_on`, `install_sandbox`, `apply_settings`, `stop_all`, `pause_all`, `start(app, registry)` (l.285) |
| `potato/env.rs` | Brain child environment (allowlist built from nothing) | `enum Brain { DeepSeek, Go }`, `KEY_NAME = "DEEPSEEK_API_KEY"` (l.25), `struct Inputs`, `build(&Inputs) -> BrainEnv` (l.118), `HostEnv::from_process` |
| `potato/sidecar.rs` | The Node process: find node, start hidden, stop tree | `trait Child`, `trait Launcher`, `struct SpawnSpec`, `script_path`, `fake_brain_active`, `probe_node`, `RealLauncher` (l.182) |
| `potato/keeper.rs` | Pure state machine of one brain process (lazy start, idle stop, crash backoff) | `backoff_ms`, keeper struct with `want_run`, `touch`, `spawned`, `hello`, `exited`, `step` |
| `potato/store.rs` | Files under `<app data>/potato/` (bots, settings, conversations) | `store::load_settings`, `store::app_conversation`, `store::state_dir`, `store::share_dir`, `store::clean_text` |
| `potato/protocol.rs` | Channel protocol v1: `Event` enum, limits, sandbox answers | `enum Event` (l.81: `Hello`, `Typing`, `Reply`, `Mood`, `Usage`, `HelperRequest`, `Error`, `SandboxCall`, unknown), `sandbox_result_ok`, `sandbox_result_err`, `HelperStatus`, `V` |
| `potato/auth.rs` | Who may call `/potato/brain/*` | auth checks (see file) |
| `potato/rules.rs` | The one pure decision for a message | `enum Why` (l.101), `enum Decision` (115), `tool_access(trust)` (134), `decide(&Facts, &Config, &Caps)` (138) |
| `potato/helpers.rs` | Start / Change folder / Not now; caps; helper watch | `struct Folder` (57), `struct CompanionAgent` (64), `struct Started` (74), `struct Running` (115), `struct HelperState` (145), `pick_folder`, `unique_names`, `folder_problem` (175) |
| `potato/hold.rs` | Pause all bots (panic / hold) | `hold::is_held`, `Hold` type |
| `potato/bot_settings.rs` | Per-bot brain, instructions with history, tool switches, export, delete | `Service::set_talk_brain`, `set_instructions`, `restore_instructions`, `set_tools`, `export_markdown`, `delete_bot` (service.rs 514-600) |
| `potato/cost.rs`, `reply_cost.rs`, `usage.rs` | Price tokens, daily limit, 30-day usage (read only) | `cost::day_of`, `cost::budget`, `cost::limit_nano`, `cost::dollars`, `usage_report` |
| `potato/fallback.rs` | Brains to try after the talk brain | `fallback::chain(primary, list, has_deepseek_key)` (l.27) |
| `potato/log.rs` | 200-line log ring | log ring functions |
| `potato/helpers.rs` + `open_work.rs`, `memory_notes.rs`, `notify.rs` | Bot panel Advanced data | open work list, memory notes, notify settings |
| `potato/routines.rs` | Scheduled prompts (`routines.json`), 30 s tick | `add_pending`, `PendingRun` |
| `potato/artifacts.rs` | Files a bot made or received, versions, previews | artifact store functions |
| `potato/attachments.rs` | Files attached to a message | attachment saving |
| `potato/voice.rs` | Read-aloud settings (system voice choice, speed, which bots speak) | voice settings types |
| `potato/trash.rs` | Recently deleted bots/items | trash store |
| `potato/wire_names.rs` | Names the brain package fixes (`DEEPSEEK`, `GO` constants used by env.rs) | `wire_names::DEEPSEEK`, `wire_names::GO` |
| `potato/resource_dirs.rs` | Where bundled `potato-brain` and `potato-sandbox` folders are | resource path resolution |
| `potato/sandbox/` | The bot's Linux computer (WSL 2 distro): see section 6 | `mod.rs`: `trait Sandbox` (l.140), `WslSandbox` (210), `FakeSandbox` (582); `manager.rs`, `detect.rs`, `install.rs`, `lifecycle.rs`, `control.rs`, `lease.rs`, `screen.rs`, `screen_live.rs`, `browser.rs`, `wsl.rs`, `live.rs`, `secrets.rs` |
| `potato/sandbox_calls.rs` | Dispatcher for the 6 sandbox tools | `TOOL_NAMES: [&str; 6]` (l.50), `Port` (used by live.rs) |
| `potato/sandbox_host.rs` | The app's one sandbox, idle tick, Labs gate, install/start/stop | `sync`, idle tick (see file) |
| `potato/window_guard.rs` | Blocks visible WSLg remote-desktop windows | `parse_report` (l.66), `visible_windows` (73), `struct Msrdc` (78), `PowerShell` (48) |
| `potato/attachments.rs`, `potato/notify.rs` | see above | |

Bridge route for the brain: `packages/app/src-tauri/src/bridge/routes/potato_brain.rs` (`respond(service, bot, method, segments, query, body)` l.23; `GET /potato/brain/inbox`, `POST /potato/brain/events`). Path guard: `bridge/http.rs` l.123 and l.370.

Commands (Tauri): `packages/app/src-tauri/src/commands/potato.rs` (`potato_state` l.35, `potato_messages` l.40, `potato_send` l.46, `potato_bot_add` 52, `potato_set_persona` 57, `potato_bot_start` 63, `potato_bot_stop` 69, `potato_mark_read` 78, `potato_forget_talk_memory` 84, `potato_settings_get` 89, `potato_settings_set` 94, `potato_log` 100, `potato_helper_folders` 106, `potato_helper_start` 113, `potato_helper_decline` 119, `potato_check_key` 126, `potato_install_node` 132). Other command files: `commands/potato_panel.rs`, `potato_bot_settings.rs`, `potato_routines.rs`, `potato_artifacts.rs`, `potato_attach.rs`, `potato_hold.rs`, `potato_usage.rs`, `potato_trash.rs`, `potato_screen.rs`, `potato_sandbox.rs`. All are registered in `packages/app/src-tauri/src/lib.rs` (e.g. `commands::potato_send` at l.703, `potato_send_files` at l.704, `potato_state` l.698, `potato_settings_set` l.712, `potato_check_key` l.718).

Secrets: `packages/app/src-tauri/src/secrets.rs` and `commands/secrets.rs` (section 3).
Other Rust areas that touch Potato Bot: `packages/app/src-tauri/src/channels/` (Telegram, section 5), `packages/app/src-tauri/src/companion/` (companions = the bot's face and agent), `packages/app/src-tauri/src/one_brain/`, `packages/app/src-tauri/src/settings_store.rs` (Labs keys, section 4).

### 1b. TypeScript: `packages/potato-brain/` (the brain, a separate Node program)

`package.json`: name `@puppet-master/potato-brain`, `"type": "module"`, engines node `>=22.18.0`. Scripts: `start` = `node src/pm.ts`, `chat` = `node src/bin.ts`, `test` = `node --test test/*.test.ts`, `typecheck` = `tsc --noEmit`. Deps: only dev deps (typescript, @types/node).

`src/` tree, one level deep, with purpose:
- `src/pm.ts`: entry that Puppet Master starts. Reads `PM_POTATO_URL` and `PM_POTATO_TOKEN` (must be loopback http), builds `PotatoChannel`, calls `channel.start()`. Exit codes: 0 after shutdown, 2 missing env, 3 app gone, 1 start error.
- `src/bin.ts`: developer terminal chat (`runCli`), `--check`. Not used by the app.
- `src/config.ts`: `interface Config`, `CONFIG` (read from env on first import).
- `src/agents.ts`: `makeAgentFactory(opts)` (l.42) builds `new Agent({...})` per conversation; `notesDirUnder`.
- `src/env.ts`: `loadEnv`, `reloadEnv`, `envNumber`, `envFlag`.
- `src/types.ts`, `src/version.ts`: shared types; `BRAIN_VERSION`.
- `src/channels/potato.ts`: `PotatoChannel` class (l.159), `PROTOCOL_VERSION = 1` (l.39), `LIMITS`, `safeId`, `capSegments`. The protocol client: `start`, `stop`, `pollLoop` (271), `emit` (319), `flushLoop` (340), `flushNow` (399), `onSend` (553), `agentFor` (471), `runTurn` (624).
- `src/channels/cli.ts`: terminal channel for `bin.ts`.
- `src/core/loop.ts`: `class Agent` (l.342), `TurnResult`, `ExtraTool`, `ToolLevel`, `formatNow` (l.102), `TurnOptions`, `AgentOptions`; the turn loop.
- `src/core/provider.ts`: `makeProvider()` (l.89), `StubProvider`; re-exports `OpenAICompatProvider` from `openai.ts`.
- `src/core/openai.ts`: OpenAI-compatible HTTP provider.
- `src/core/opencode-auth.ts`: OpenCode Go sign-in reader.
- `src/core/models.ts`: model table (vision support etc.).
- `src/core/sandbox-tools.ts`: the 6 computer tools: `SANDBOX_TOOL_NAMES` (l.36), `isSandboxTool`, `sandboxToolSpecs` (159), `formatReply` (213), `sandboxTools(opts)` (259), `pruneSandboxMessages` (299), `CALL_TIMEOUT_MS = 90_000` (56), `MAX_RESULT_CHARS` (58).
- `src/core/helper-tool.ts`: `helperTool`, `ASK_HELPER_SPEC`, `cleanProject`, `TASK_MAX = 600`.
- `src/core/assemble.ts`, `budget.ts`, `cache.ts`, `hash.ts`, `errors.ts`: prompt assembly, token budget, prefix cache, hashing, error types.
- `src/persona/`: `persona.ts` (`loadPersona`, `moodStore`, `selfStore`, `saveMood`, `loadSelf`), `packs.ts` (`listPacks`), `mood.ts`, `express.ts` (tone/segments), `kaomoji.ts`, `self.ts`, `notes.ts`, `people.ts`, `share.ts`, `dsh-import.ts`.
- `src/memory/`: `vault.ts`, `vaultMemory.ts` (`VaultMemory`), `index.ts` (`MemoryIndex`), `diary.ts`, `checkpoint.ts`, `migrate.ts`, `tokenize.ts`, `types.ts`.
- `src/lifted/`: code ported from Rakazo (`PORTED.md` lists it): `tool-loop.ts`, `silent-reply.ts`, `redact-secrets.ts`, `screen-lease.ts`, `context-pruning.ts`, `prompt-data.ts`, `tool-schemas.ts`.
- `src/rooms/`: DOES NOT EXIST.

Tests: `packages/potato-brain/test/` (see section 9).

### 1c. React: Potato Bot pages, DMs and settings

Page and thread (`packages/app/src/components/cockpit/potato/`):
- `PotatoPage.tsx`: `PotatoPage(props: PotatoPageProps)` (l.~50). Three regions: bot list, thread, bot panel. Rendered from `packages/app/src/CockpitApp.tsx` l.515.
- `PotatoLayout.tsx`, `usePotatoLayout.ts`: region sizes.
- `BotList.tsx` (`BotList`, l.57), `BotListRow.tsx`, `BotListSearch.tsx`, `BotContextMenu.tsx`.
- `ThreadView.tsx`: `ThreadView(props: ThreadViewProps)` (l.64). Sends via `sendAsPerson` (l.~92) which calls `transport.send(bot.id, text)` (l.94).
- `Composer.tsx`: `Composer(props)` (l.127). Calls `transport.send(bot.id, text)` at l.356. Also `ComposerPickers.tsx`, `AttachChips.tsx`, `voice` (dictation hook in `Composer.tsx`).
- `MessageRow.tsx`, `Transcript.tsx`, `TypingRow.tsx`, `ThreadHeader.tsx`, `ThreadIcons.tsx`, `FirstMessage.tsx`, `ToolActivity.tsx`, `HelperCard.tsx`, `ChoiceCard.tsx`, `BrainStateCard.tsx`.
- `BotPanel.tsx`: `BotPanel(props)` (l.44). Tabs: Computer, Routines, Advanced (`RoutinesTab.tsx` and `AdvancedTab.tsx` exist; no `ComputerTab.tsx`).
- `ComputerPanel.tsx` (`ComputerPanel`, l.37), `ComputerOverlay.tsx` (`ComputerOverlay`, l.39), `ComputerPreview.tsx`, `ComputerBanner.tsx` (`ComputerBanner`, l.20; `bannerDue`), `ComputerMaintenance.tsx`, `useScreenLink.ts`, `useScreenState.ts`, `useBotComputerOff.ts`.
- `SandboxWizardHost.tsx` (`SandboxWizardHost`, l.11): mounts the settings wizard over the page.
- `ArtifactsPage.tsx`, `ArtifactPreview.tsx`, `ArtifactSandbox.tsx`, `SaveAsFile.tsx`.
- `StarterPicker.tsx`: "New bot" starter picker.
- `OpenWork.tsx`, `MemoryList.tsx`, `NotifySection.tsx`, `RoutineForm.tsx`, `InstructionHistory.tsx`, `AdvancedInstructions.tsx`, `AdvancedTools.tsx`, `AdvancedModel.tsx`, `AdvancedName.tsx`, `AdvancedSection.tsx`, `AdvancedBot.tsx`.
- `PersonaSwitch.tsx`, `MoodChip.tsx`, `SpeakButton.tsx`, `useReadAloud.ts`, `ReadAloudSection.tsx`.
- `Palette.potato.test.tsx`, `Sidebar.potato.test.tsx` (outside this folder, in `components/cockpit/`).
- Keys: `potato-keys.ts` (`usePotatoKeys`).
- `model.ts` (`pickOpenBot`, `totalUnread`).

Hooks and lib (`packages/app/src/`):
- `hooks/usePotato.ts`: `usePotatoState(transport = tauriPotatoTransport)` (l.17), `usePotatoThread(transport, botId, version, unread)` (l.64), `readOpenBot`, `writeOpenBot` (localStorage key `pm.potato.open`).
- `lib/potato.ts`: types (`PotatoState`, `PotatoMessage`, `SendOutcome`, `PotatoSettings`), `PotatoTransport` interface, `tauriPotatoTransport` (l.~158), `plainError`, `fakePotatoTransport` (in-memory twin for tests).
- `lib/potato-thread.ts`: pure thread rules (`THREAD_COPY`, day separators, unread divider).
- `lib/potato-composer.ts`: `MAX_MESSAGE`, composer rules.
- `lib/potato-artifacts.ts`, `potato-attach.ts`, `potato-botsettings.ts` (`tauriBotSettingsTransport`), `potato-routines.ts` (`tauriRoutinesTransport`), `potato-open-work.ts`, `potato-memory.ts`, `potato-notify.ts`, `potato-panel.ts` (`DEFAULT_TAB`, `PanelTab`), `potato-hold.ts`, `potato-trash.ts`, `potato-usage.ts`, `potato-speech.ts`, `potato-starters.ts`, `potato-firstrun.ts`, `potato-layout.ts`, `potato-list.ts`, `potato-computer.ts` (`COMPUTER_COPY`), `potatoScreen.ts` (`ScreenTransport`, `tauriScreenTransport`, `headerWords`), `potato-sandbox.ts` (`SandboxTransport`, `tauriSandboxTransport`), `potato-panel.ts`.

Settings (`packages/app/src/components/settings/`):
- `potato/PotatoModelsPage.tsx`, `potato/PotatoUsagePage.tsx`, `potato/PotatoSubPages.tsx`, `potato/RecentlyDeleted.tsx`, `potato/usePotatoSettings.ts`.
- `sections/PotatoSection.tsx`: the Potato Bot settings section (DeepSeek key row, Labs switch row at l.133-134). `sections/PotatoVoiceRows.tsx`.
- `catalog.ts` l.23: `{ id: 'potato', title: 'Potato Bot', ..., labsFlag: 'potato_bot_enabled' }`.
- `sandbox/SandboxWizard.tsx` (`SandboxWizard`, l.130; `stepsFor` l.40; `InstallProgress` l.105), `sandbox/SandboxRows.tsx` (`makeSandboxRows`, l.415; `RemoveSheet`, `AddOsSheet`), `sandbox/useSandbox.ts`.

---

## 2. A Potato Bot DM turn, end to end

1. UI: `packages/app/src/components/cockpit/potato/Composer.tsx` l.356 `else await transport.send(bot.id, text);` and `ThreadView.tsx` l.94 `sendAsPerson` -> `transport.send(bot.id, text)`.
2. Transport: `packages/app/src/lib/potato.ts` l.~162 `send: (botId, text) => invoke<SendOutcome>('potato_send', { botId, text })` (`invoke` from `@tauri-apps/api/core`).
3. Tauri command: `packages/app/src-tauri/src/commands/potato.rs` l.45-48 `pub async fn potato_send(bot_id: String, text: String)` -> `run(move |s| s.send(&bot_id, &text))`.
4. Engine: `packages/app/src-tauri/src/potato/service.rs` l.686 `Service::send` -> `send_as(bot, text, Origin::Person)` (l.692). Inside `send_as`: loads settings, day cost via `cost::day_of`, `cost::budget`, `preflight` (Node and key check), builds `Facts` (surface `"app"`, sender `Sender::App`), calls `rules::decide` (`potato/rules.rs` l.138). Refusals map to codes `POTATO_OFF`, `POTATO_EMPTY`, `POTATO_PAUSED`, `POTATO_RATE`, `POTATO_REFUSED`; over-limit returns `BrainState::DailyLimit` without error.
5. Save: `Service::save` (service.rs l.453) writes the person's message to the conversation (`store::app_conversation(bot)`).
6. Queue: `Service::deliver` (service.rs l.825) pushes the inbox item (`push_to_brain`, l.1080), then `rt.keeper.want_run(now)` (l.815) asks the keeper for a brain.
7. Spawn: `Service::spawn` (l.1154) -> `try_spawn` (l.1174) -> `self.launcher.spawn(&spec, sink)` (l.1230). Brain env from `potato/env.rs` `build(&Inputs)` (l.118, DEEPSEEK key added only for `Brain::DeepSeek`, l.150). Node found by `sidecar.rs` `probe_node`/`RealLauncher` (l.182); script from `script_path` (l.124).
8. Brain process: `packages/potato-brain/src/pm.ts` `main()` reads `PM_POTATO_URL` and `PM_POTATO_TOKEN`, creates `new PotatoChannel({...})`, `channel.start()`.
9. Brain pulls the item: `packages/potato-brain/src/channels/potato.ts` `pollLoop` (l.271) long-polls `GET /potato/brain/inbox`; `case "send": this.onSend(raw)` (l.421-422); `onSend` (l.553).
10. Turn: `runTurn` (l.624) -> `this.agentFor(conversationId)` (l.471, calls `opts.agentFor` = `makeAgentFactory` in `packages/potato-brain/src/agents.ts` l.42) -> `Agent.turn(...)` (`core/loop.ts`, class `Agent` l.342; the call at channel l.649). Provider from `core/provider.ts` `makeProvider()` (l.89). Tools for a sandbox turn: `sandboxTools` in `core/sandbox-tools.ts` (l.259).
11. Brain sends back: `emit(event)` (channel l.319) -> `flushLoop` (l.340) / `flushNow` (l.399) -> `POST /potato/brain/events` with `{"v":1,"events":[...]}`.
12. Bridge: `packages/app/src-tauri/src/bridge/routes/potato_brain.rs` `respond` (l.23) -> `Service::ingest(bot, events)` (service.rs l.1406) -> `on_event` (l.1421). A `Reply` event (`Event::Reply { conversation_id, in_reply_to, segments, kaomoji, tone }`, protocol.rs l.84) is saved as the bot's message; a `Usage` event feeds `cost`.
13. Change signal: `ingest` calls `self.host.changed(bot)` -> `potato/live.rs` `changed` (l.187) -> `app.emit(CHANGED_EVENT, json!({"botId": bot}))` (l.192), event name `potato://changed`.
14. UI refresh: `lib/potato.ts` `tauriPotatoTransport.onChanged` listens to `potato://changed` (`listen`). `hooks/usePotato.ts` `usePotatoThread` (l.64) re-reads through `tauriPotatoTransport.messages(botId, limit)` -> command `potato_messages` (`commands/potato.rs` l.40) -> `Service::messages` (service.rs l.465).

Sandbox calls inside a turn (only when Labs both on and a sandbox is set up): brain emits `sandbox_call` (protocol `Event::SandboxCall`, l.92) -> `Service` handler (service.rs ~l.1647) -> `potato/sandbox_calls.rs` dispatcher -> answer inbox item `sandbox_result` (`protocol::sandbox_result_ok` / `sandbox_result_err`).

---

## 3. DeepSeek key: where it is stored and read (no values)

- Store: OS credential store via the `keyring` crate. `packages/app/src-tauri/src/secrets.rs`: `pub const SERVICE: &str = "com.puppetmaster.app"` (l.19); `pub const SIDECAR_SECRETS: &[&str] = &["deepseek_api_key", "discord_bot_token"]` (l.31); `is_sidecar_secret` (l.33); `is_ui_secret` (l.38); `trait SecretBackend` (l.42: `get`, `set`, `delete`, `label`, `available`); `struct KeyringBackend` (l.53, `entry` uses `keyring::Entry::new(SERVICE, name)`); `struct MemoryBackend` (l.95, tests only); `backend()` (l.140 non-test, l.146 test); `read_secret(name) -> Option<String>` (l.152, empty counts as unset); `struct SecretMask` (l.158); `mask(value)` (l.164, "set" and last 4 chars for long keys); `get_masked` (l.175); `migrate_settings_file` (l.189); `migrate_on_startup` (l.235).
- The key name for DeepSeek is `deepseek_api_key` (constant `SECRET_DEEPSEEK` in `potato/service.rs` l.49).
- Commands: `packages/app/src-tauri/src/commands/secrets.rs`: `secret_storage_info` (l.34), `secret_set(name, value)` (l.40), `secret_get_masked(name)` (l.56), `secret_delete(name)` (l.62), `secret_get_for_llm(name)` (l.71, provider keys only; DeepSeek is not in `PROVIDER_SECRETS`).
- Write path from UI: `packages/app/src/components/settings/sections/PotatoSection.tsx` and `packages/app/src/lib/secrets.ts` reference `deepseek_api_key`. `lib/setup-checks/index.ts` also reads it.
- Read in Rust: `potato/live.rs` `fn secret(&self, name)` (l.154) -> `crate::secrets::read_secret(name)`; in e2e builds (`fake_brain_active`) it returns a stand-in value instead, the real key is not read.
- Used for the state mask: `service.rs` l.1025 `secrets::mask(self.host.secret(SECRET_DEEPSEEK).as_deref())`. Used for fallback choice: `service.rs` l.1187 and `fallback::chain(..., has_deepseek_key)`.
- Given to the brain: only as environment variable `DEEPSEEK_API_KEY` (`env.rs` `KEY_NAME` l.25; added at l.150), passed through `service.rs` l.1218 `deepseek_key: key.as_deref()`. Never an argument. The brain also redacts it: `packages/potato-brain/src/pm.ts` `secrets: () => [token, env("DEEPSEEK_API_KEY"), ...]`.
- Check: `potato/live.rs` `check_key(&self, key)` (l.255) -> `ask_model(key)` (l.266): one tiny request to `https://api.deepseek.com/chat/completions`, model `deepseek-chat`, `max_tokens: 1`. Tauri command `potato_check_key` (`commands/potato.rs` l.126).
- `discord_bot_token` is also in `SIDECAR_SECRETS`; nothing in `packages/app/src-tauri/src` reads it outside `secrets.rs` (see section 5).

---

## 4. Labs flags

### 4a. Definitions
- `potato_bot_enabled`: Rust constant `LABS_KEY` in `packages/app/src-tauri/src/potato/live.rs` l.31 (`pub const LABS_KEY: &str = "potato_bot_enabled";`). Read by `labs_on(public)` (l.60): on only when the value is exactly `true`. It is the kill switch (see the doc comment of `live.rs`).
- `potato_sandbox_enabled`: `SANDBOX_LABS_KEY` in `live.rs` l.33. Read by `sandbox_labs_on` (l.65). It does nothing unless Potato Bot is on too (doc comment l.32).
- Default value site: NOT located. `docs/features/potato-bot.md` says both are off by default.
- Settings toggles: `packages/app/src/components/settings/sections/GeneralSection.tsx` l.496-497 (`checked={env.settings.potato_bot_enabled ?? false}`, `env.patch({ potato_bot_enabled: value })`); `sections/PotatoSection.tsx` l.133-134; `sandbox/SandboxRows.tsx` l.315-316 and l.326 (`labsFlag: 'potato_sandbox_enabled'`).
- Settings types: `packages/app/src/components/settings/model.ts` l.79 (`labsFlag?: 'potato_sandbox_enabled' | 'acp_chat_enabled' | 'model_bridge_enabled'`, a row), l.90 (`labsFlag?: 'potato_bot_enabled'`, a section), l.97 `visibleSections(sections, settings)` which takes `potato_bot_enabled`, `potato_sandbox_enabled`, `acp_chat_enabled`, `model_bridge_enabled`.
- Section: `packages/app/src/components/settings/catalog.ts` l.23 (`labsFlag: 'potato_bot_enabled'`).
- Labs label map for setup checks: `packages/app/src/lib/setup-checks/labs.ts` l.8-9 (`potato_bot_enabled: 'Potato Bot'`, `potato_sandbox_enabled: "Fish's computer"`); also `setup-checks/index.ts` l.46.
- Page gate in the cockpit: `packages/app/src/CockpitApp.tsx` l.134-136 (`potatoOn = potato.state?.enabled === true`), render l.515 only when `view === 'potato' && potato.state && potatoOn`.
- Rust gate in the brain path: `service.rs` `enabled()` (l.432) and `set_enabled` (l.937); `live.rs` `apply_settings(public)` (l.86).

### 4b. DEV_LABS_KEYS (development override)
- Rust, the source: `packages/app/src-tauri/src/settings_store.rs` l.13-21: `pub const DEV_LABS_KEYS: &[&str] = &[ "missions_enabled", "night_shift_enabled", "potato_bot_enabled", "potato_sandbox_enabled", "acp_chat_enabled", "desk_enabled", "model_bridge_enabled", ];` (the two Potato keys are at l.16 and l.17).
- Rule: `potato/mod.rs` doc comments and `settings_store.rs`: in a debug build, unless `PM_DEV_LABS=0`, every key in `DEV_LABS_KEYS` reads as `true` (applied at read time by `with_dev_labs`, l.~45-60; never saved). `dev_labs_gate(debug_build, env, build_allows)` and `dev_labs_override_active()` are at `settings_store.rs` l.~26-40.
- Tests: `settings_store.rs` l.~324 and l.~333 assert `DEV_LABS_KEYS.contains(&"desk_enabled")` and `"model_bridge_enabled"`.
- TypeScript mirror: `packages/app/src/lib/dev-labs.ts` l.8-16: `export const DEV_LABS_KEYS = [ 'missions_enabled', 'night_shift_enabled', 'potato_bot_enabled' (l.11), 'potato_sandbox_enabled' (l.12), 'acp_chat_enabled', 'desk_enabled', 'model_bridge_enabled', ] as const;`. The comment says it "Mirrors Rust `settings_store::DEV_LABS_KEYS`".
- Other users of the TS list: `packages/app/src/components/settings/sections/AgentsSection.tsx` l.87 and `lib/setup-checks/labs.ts` (`DEV_LABS_KEYS.filter`).

### 4c. Exact lines to edit to add a new Labs flag `<new_flag>_enabled`
1. `packages/app/src-tauri/src/settings_store.rs` l.13-21: add `"<new_flag>_enabled",` to `DEV_LABS_KEYS` (only if dev builds should show it on).
2. `packages/app/src/lib/dev-labs.ts` l.9-15: add `'<new_flag>_enabled',` (the comment says the two lists must match).
3. `packages/app/src/lib/setup-checks/labs.ts` l.8-9 area: add a label to the `LABEL` map.
4. `packages/app/src/components/settings/model.ts` l.79 (row flag union) and/or l.90 (section flag type), and the `visibleSections` settings type on l.97.
5. The toggle row (copy the `GeneralSection.tsx` l.496-497 pattern) and, for a section, a `catalog.ts` entry with `labsFlag`.
6. Rust gate: a `pub const` like `live.rs` l.31/33 and a `*_on(public)` reader like `labs_on` l.60.
7. Note: `settings_store` also needs the key in the public settings the window reads (not checked in detail).

---

## 5. Channels and adapters

### 5a. Telegram
- Branch `feat/tag-your-agent`: NOT present locally or on origin any more (`git branch -a` lists no such ref; `git log feat/tag-your-agent` fails).
- It IS merged into `main`: merge commit `856c0acc` "Merge pull request #198 from Potato-dev-inc/feat/tag-your-agent". Commit `2a3fa39a` "Tag Your Agent: the group path of the Telegram channel (default off)" is an ancestor of `main` (checked with `git merge-base --is-ancestor`). Also `719a8c5c` "Tag Your Agent: settings block, API wrappers and tests".
- Files that PR #198 adds or changes (`git diff --name-only 856c0acc^1 856c0acc`, 33 files, +3122 / -20):
  - `docs/FEATURES.md`, `docs/REPO_MAP.md`, `docs/features/tag-your-agent.md`
  - `packages/app/src-tauri/src/channels/`: `config.rs`, `engine.rs`, `engine/group.rs`, `engine/group_tests.rs`, `engine_tests.rs`, `fake_bot.rs`, `host.rs`, `host_live.rs`, `mod.rs`, `setup_tag.rs`, `setup_tag_tests.rs`, `tag_config.rs`, `tag_log.rs`, `tag_prompt.rs`, `telegram/api.rs`, `telegram/api/wire.rs`, `telegram/fixtures/group_mention.json`, `telegram/fixtures/group_reply_to_bot.json`, `telegram/poller.rs`, `telegram/poller/tests.rs`, `test_host.rs`
  - `packages/app/src-tauri/src/commands/channels.rs`, `packages/app/src-tauri/src/lib.rs`
  - `packages/app/src/components/settings/blocks/TelegramTagBot.tsx` (+ `.test.tsx`), `packages/app/src/components/settings/sections/PhoneSection.tsx` (+ `.test.tsx`), `packages/app/src/lib/telegram-api.ts` (+ `.test.ts`), `packages/app/src/styles/settings.css`
- Telegram code is in `packages/app/src-tauri/src/channels/` (NOT in `potato/`). Potato Bot itself has no Telegram code path. `docs/features/potato-bot.md` l.91: "A chat app (Discord, Telegram) is a later phase."
- Telegram group path is default off (commit subject).

### 5b. Discord
- No Discord channel code in the repo. A grep for `discord` (case-insensitive) in `packages/potato-brain/src`, `packages/app/src` and `packages/app/src-tauri/src` (non-test) finds only `packages/app/src-tauri/src/secrets.rs` (the `discord_bot_token` name). `potato/env_tests.rs` mentions it in a test. Docs mention it: `docs/features/potato-bot.md`, `docs/features/telegram.md`, `docs/REPO_MAP.md`.
- The Discord design (fishd) is only in `D:/pm-research/INV-FISHD-DISCORD.md` (summary in section 7).

---

## 6. Sandbox (the bot's Linux computer)

### 6a. The six tools (Fish's sandbox tools)
Names: `computer_observe`, `computer_act`, `browser_navigate`, `browser_snapshot`, `browser_act`, `ask_person_for_screen`.
- Rust: `packages/app/src-tauri/src/potato/sandbox_calls.rs` l.50 `pub const TOOL_NAMES: [&str; 6]`; `Tool::...` mapping l.75-80.
- Brain: `packages/potato-brain/src/core/sandbox-tools.ts` l.36-43 `SANDBOX_TOOL_NAMES`.
- Limits (per `docs/features/potato-bot.md`): up to 24 actions per computer or browser call; 500 actions per bot per day (`sandboxActionsPerDay`, 10 to 5000); screen lease 15 minutes; `ask_person_for_screen` needs-you card for 30 minutes.

### 6b. Computer panel (React)
- `packages/app/src/components/cockpit/potato/ComputerPanel.tsx` (`ComputerPanel`, l.37; header comment says no colour), `ComputerOverlay.tsx` (full window), `ComputerPreview.tsx`, `ComputerBanner.tsx`, `ComputerMaintenance.tsx`, `useScreenLink.ts` (exports `KEEP_EVERY_MS`, `SILENT_VIEWER_MS`), `useScreenState.ts`.
- Mounted from `BotPanel.tsx` (its Computer tab). A source file `ComputerTab.tsx` was NOT found; only `ComputerTab.test.tsx` exists.

### 6c. Setup wizard
- `packages/app/src/components/settings/sandbox/SandboxWizard.tsx`: `SandboxWizard({ transport, onClose })` (l.130), `type Step = 'linux' | 'look' | 'use' | 'coding' | 'daily' | 'summary' | 'progress'` (l.34), `stepsFor(draft)` (l.40), `InstallProgress` (l.105).
- Host over the page: `packages/app/src/components/cockpit/potato/SandboxWizardHost.tsx` (`SandboxWizardHost`, l.11).
- Settings rows: `packages/app/src/components/settings/sandbox/SandboxRows.tsx` (`makeSandboxRows`, l.415; `RemoveSheet`, `AddOsSheet`).
- Client: `packages/app/src/lib/potato-sandbox.ts` (`SandboxTransport`, `tauriSandboxTransport`), `useSandbox.ts`, CSS `packages/app/src/styles/potato-sandbox.css`.
- Commands: `packages/app/src-tauri/src/commands/potato_sandbox.rs` (`potato_sandbox_status` l.40, `_menu`, `_detect`, `_install` l.75, `_continue_install`, `_add_os`, `_switch_os`, `_cancel_install`, `_dismiss_install`, `_start` l.113, `_stop`, `_reset`, `_remove(typed)`, `_set_idle(minutes)`, `_linux_ask`, `_linux_enable(token)`). Events: `potato://sandbox-install`, `potato://sandbox-changed`.
- Manager: `potato/sandbox/manager.rs`, detection: `potato/sandbox/detect.rs`, host: `potato/sandbox_host.rs`.

### 6d. Window guard
- `packages/app/src-tauri/src/potato/window_guard.rs`: checks after every start that no visible window owned by `msrdc`/`msrdcw` (WSLg) exists. `visible_windows(probe)` (l.73), `Msrdc` (l.78, the production check, asks PowerShell hidden with an encoded command, no script file). Doc says if it cannot tell, it reports "visible" (start stops). Called from `Lifecycle::ensure_ready` per the file comment (`LifecycleError::VisibleWindow`).

### 6e. Doc: NOT done yet (from docs)
- `docs/features/potato-sandbox.md` l.8: "A bot using it, and watching or taking over its screen, come in later pieces." (This sentence conflicts with `docs/features/potato-bot.md`, which describes the bot using the tools and the take-over; treat as stale.)
- `docs/features/potato-sandbox.md` l.132: "No sign-in copying yet. Copying an agent's login into the sandbox is a later choice; today you sign in inside the sandbox's Chrome."
- `docs/features/potato-sandbox.md` l.138-139 area: the install reads guest files from app resources or the source tree (a stated requirement).
- `docs/features/potato-bot.md` l.126: a paired phone can watch (view). Control from a phone needs a yes on this PC first; "that yes is not built yet" (a phone gets `PHONE_NEEDS_DESKTOP_YES`).
- `docs/features/potato-bot.md` l.113: "Not yet: files the person attaches in chat (the `attach_file` brain tool), the date filter, and a maximise button."
- `docs/features/potato-bot.md` l.91: chat apps are a later phase.

---

## 7. Rooms, groups, fishd gate chain

- `packages/potato-brain/src/rooms/`: DOES NOT EXIST. No `room` files in `packages/potato`/`packages/app/src-tauri/src/potato` (grep found none for group chats in the Potato folder).
- Group code that exists is Telegram Tag Your Agent (`packages/app/src-tauri/src/channels/engine/group.rs`, section 5a), and `packages/app/src-tauri/src/companion/` has peer code (`companion/peer.rs`, not Potato Bot DMs).
- Fishd modules (from `D:/pm-research/INV-FISHD.md` section 6 and `INV-FISHD-DISCORD.md`; these are the inventory's file names in `C:\Users\Ren-pc\Desktop\work2\Agent\`, NOT re-read by me in that repo):
  - Addressing: `src/channels/addressing.ts` (`aimedElsewhere`, addressed check). Status in PM: no.
  - Burst buffer: `src/channels/burst.ts` (last 10 minutes, up to 12 lines). PM: no.
  - Gate chain: `src/channels/gate.ts` (`makeGate()`, line ~446; modes `heuristic`, `jev`, `llm`, `chain`; `FISHD_GATE` env chooses; default heuristic). `src/channels/ambient.ts` (`ambientChime()` line ~37, arm delay, settle, cooldown). PM: no.
  - Silence budget and cooldown: `src/channels/ambient.ts` and `src/live.ts` (cap of unprompted replies, example 12 in 30 minutes; per-channel cooldown). PM: no.
  - Spontaneous timers and hush hours: `src/channels/spontaneous.ts`. PM: no.
  - Scope and rate: `src/channels/scope.ts` (`TURN_LIMIT` 6 per 5 minutes for non-owners).
- Discord scoping details: see the Discord summary below and `D:/pm-research/INV-FISHD-DISCORD.md` sections 1-12.

### Discord inventory summary (`D:/pm-research/INV-FISHD-DISCORD.md`, 10 lines)
1. Library discord.js ^14.27.0, Node >= 22.18; entry `startDiscord()` in fishd `src/channels/discord.ts`.
2. Gateway login with intents Guilds, GuildMessages, MessageContent (privileged), DirectMessages and typing/reaction intents.
3. DMs keyed `dm:<userId>` (one memory per person); guild channels keyed by channel id; threads behave as separate channels (inferred).
4. Filters: `DISCORD_GUILD_IDS`, `DISCORD_ONLY_CHANNELS` (hard boundary), `DISCORD_CHANNEL_IDS` (always answer).
5. Order in `onMessage`: drop DMs/non-guild oddities, drop all bots (including ones that @ her), `admit()` (scope, DM rate bucket for non-owners).
6. Hard address = DM, @mention or reply, her name (`DISCORD_NAMES`), always-channel, or `DISCORD_REPLY_ALL=1`; a plain @ of someone else is not hard-addressed (`addressing.ts`).
7. Ambient (off by default, `DISCORD_AMBIENT=1`): burst buffer then gate chain jev -> openrouter models -> main provider -> heuristic.
8. Bias: x1.6 when she asked to be left alone, x settle bias, x2.5 when aimed at someone else.
9. Silence budget, arm delay and settle period decide when the queued burst is sent.
10. Owner-only commands and `/mood`, `/stats`, `/recall`, `/diary`, `/config`, `/tune`, `/ignore`, `/ping`, `/help`; other bots ignored; memory per room with roster.

---

## 8. Rakazo parity (`D:/pm-research/RAKAZO-PORT-MAP.md`, `D:/pm-research/RAKAZO-UI-MAP.md`)

Status column = what the repo shows now. "grep" = a repo search for the feature's words found nothing relevant. Source doc status is in brackets where it differs. Only the rows listed were checked; the rest of the port map was not re-verified.

| Item (port map section) | Status in repo | Evidence / note |
|---|---|---|
| Persistent bots (2a) | done | `potato/store.rs`, `potato/service.rs` |
| Bot settings: instruction history, tool switches, export, delete (2a) | partial | `potato/bot_settings.rs`; no duplicate/archive found |
| DMs / chat UI: composer, reactions, quote, markdown (2a) | partial | `Composer.tsx`, `ThreadView.tsx`, `MessageRow.tsx`; no reactions found |
| Group chats, 2-10 bots (2a) | missing | no `groups` module in `potato/`; Telegram group path only (`channels/engine/group.rs`) |
| Bot-to-bot messaging (2a) | missing | grep: none |
| Handoff in a group (2a) | missing | grep: none |
| Subagents / helpers (2a) | done (partial scope) | `potato/helpers.rs`, `HelperCard.tsx`; helpers start local agents |
| Markdown memory (2b) | done | `potato-brain/src/memory/vault.ts`, `vaultMemory.ts` |
| Semantic memory providers (2b) | missing | grep: none |
| History compaction (2b) | partial | `potato-brain/src/memory/checkpoint.ts`; no compaction module |
| Search past chats (2b) | partial | `memory_search` in brain; app-wide `chat_index` (port map) |
| Stale-screen pruning (2b) | done | `pruneSandboxMessages` in `core/sandbox-tools.ts` l.299 |
| Tool-loop guard (2b) | done | `potato-brain/src/lifted/tool-loop.ts` |
| Time anchor (2b) | done | `formatNow` `core/loop.ts` l.102 |
| Open work list (2b) | done | `potato/open_work.rs`, `OpenWork.tsx` |
| Model choice, fallbacks (2b) | partial | `potato/fallback.rs`; DeepSeek and OpenCode Go brains |
| Usage and cost (2b) | done | `potato/cost.rs`, `potato/usage.rs`, `PotatoUsagePage.tsx` |
| Settings from chat (2b) | missing | grep: none |
| Lazy tool catalog (2b) | missing | grep: none |
| Scheduled routines (2c) | done (partial scope) | `potato/routines.rs`, `RoutinesTab.tsx`, `lib/potato-routines.ts` |
| Change-watch routines (2c) | missing | grep: none |
| Webhook / Git / Slack triggers (2c) | missing | grep: none |
| Stuck-work reminders (2c) | missing | grep: none for Potato |
| Speak replies (2d) | partial | `potato/voice.rs` (settings), `useReadAloud.ts`; webview speech |
| Dictation / hold-to-talk (2d) | partial | dictation in `Composer.tsx` |
| Voice calls (2d) | missing | grep: none |
| Computer provider interface (2e) | done | `potato/sandbox/mod.rs` `trait Sandbox` l.140 |
| Team vs private computer (2e) | missing | grep: none |
| Lazy start, idle stop, reset (2e) | done | `sandbox/lifecycle.rs`, `sandbox_host.rs`, `commands/potato_sandbox.rs` |
| Observe / act tools (2e) | done | `sandbox_calls.rs`, `sandbox-tools.ts` |
| Browser page tools (2e) | done | `potato/sandbox/browser.rs`, `sandbox-tools.ts` |
| Control lease / take-over (2e) | done | `potato/sandbox/lease.rs` |
| Screen proxy and sealed links (2e) | partial | `bridge/routes/potato_screen.rs`, `potato/sandbox/screen.rs` |
| noVNC viewer page (2e) | done | `screen.rs` l.715 allows `embed.html`, `clipboard-bridge.js`, `mobile-keyboard.js` and `novnc/` |
| Clipboard bridge (2e) | done | `clipboard-bridge.js` allowed in `screen.rs` l.715 |
| Egress restriction (2e) | missing | grep: none |
| Shell guard / desktop command guard (2f) | missing | grep: none |
| Artifacts (2g/2h) | done | `potato/artifacts.rs`, `ArtifactsPage.tsx`, `ArtifactSandbox.tsx` |
| Teach a task (2e) | missing | grep: none |
| Use my own computer / local helpers (2f) | done | helpers start local agents |
| Approval modes and lists (2f) | partial | `rules::tool_access`, helper Start card |
| Outside = stricter (2f) | done | `Trust = "owner_pc" \| "owner_remote" \| "others"` in `potato-brain/src/channels/potato.ts` l.61 |
| Secrets vault (2f) | partial | OS store `secrets.rs`; `lifted/redact-secrets.ts` |
| SSRF-safe fetch (2g) | missing for Potato | grep: none for Potato (push code has its own) |
| Site trust (2g) | missing | grep: none |
| Web search and fetch (2g) | missing | grep: none |
| MCP client (2g) | missing | grep: none |
| Skills (2g) | partial | `companion/skills.rs` |
| Plugins / Agent Store (2g) | partial | agent packs (not Potato) |
| Import an agent (2g) | partial | `potato-brain/src/persona/dsh-import.ts` |
| Tickets board (2g) | missing | grep: none |
| Media and plots (2g) | missing | grep: none |
| Chat apps (2h) | partial | Telegram only, `channels/telegram/` (not Potato) |
| Messaging commands (2h) | missing | grep: none |
| Mobile app (2h) | partial | phone pairing (not Potato-specific) |
| Notifications (2h) | partial | `potato/notify.rs` |
| Onboarding (2h) | partial | `potato-firstrun.ts`, `firstrun` components |
| Animated avatars (2h) | partial | companion avatars |
| Tool activity disclosure (2h) | partial | `ToolActivity.tsx` |
| Account export/import (2h) | partial | `bot_settings.rs` export |
| i18n and RTL (2h) | not checked | |
| Evals (2h) | partial | fake brain tests |
| Settings-from-chat undo cards (UI map row 'Settings from chat') | missing | grep: none (same as the row above) |
| Avatar studio (UI map 1.23) | not checked | |

UI map (`D:/pm-research/RAKAZO-UI-MAP.md`): section 3 has a 50-row table (screen to our component, with a status column of have / partial / missing / skip). Its status column was not counted or re-verified row by row. Section 5 is the phased build plan (waves G0-G4). Decisions block at l.505 (lead, 2026-11-01).

---

## 9. Test commands that exist

Run from the folder shown.

- `packages/app` (vitest; `package.json` scripts: `"test": "vitest run"`, `"test:watch": "vitest"`):
  - `npx vitest run src/components/cockpit/potato` (all Potato Bot page tests)
  - `npx vitest run src/components/settings/potato` (Potato settings pages)
  - `npx vitest run src/components/settings/sandbox src/lib/potato-sandbox.test.ts` (quoted from docs/features/potato-sandbox.md)
  - `npx vitest run src/lib/potato` (matches `potato.test.ts`, `potato-*.test.ts`)
  - `npx vitest run src/components/cockpit/potato/PotatoPage.test.tsx` (single file)
  - `npx vitest run src/styles/cockpit-potato.css.test.ts`
- `packages/potato-brain` (`package.json` `"test": "node --test test/*.test.ts"`):
  - `node --test test/potato-channel.test.ts` (protocol client)
  - `node --test test/sandbox-channel.test.ts test/sandbox-tools.test.ts` (sandbox tools)
  - `node --test test/*.test.ts` (all)
  - Other files: `test/integration.test.ts`, `test/real-brain.test.ts`, `test/fake-brain.mjs`, `test/lifted-*.test.ts`, `test/memory.test.ts`, `test/persona` tests (see folder).
- `packages/app/src-tauri` (Cargo; lib crate `puppet_master_app_lib`, `Cargo.toml` l.11):
  - `cargo test --lib potato` (documented in `docs/features/potato-bot.md` and `potato-sandbox.md`)
  - `cargo test --lib potato::sandbox` (sandbox subtree, by name prefix)
  - `cargo test --lib settings_store` (Labs keys tests, l.324/333)
  - `cargo test --lib secrets` (secret tests)
  - Test files are `*_tests.rs` next to each module (e.g. `potato/service_tests.rs`, `potato/sandbox/wsl_tests.rs`, `bridge/routes/potato_screen_tests.rs`).
  - If run: use `CARGO_TARGET_DIR=D:/pm-night/target-facts-a` (instruction). Not run here.

---

## 10. UI conventions

### 10a. Class-name prefixes
- Potato Bot page: `ck-` prefix plus `potato`: e.g. `ck-potato-thread`, `ck-potato-tabpanel`, `ck-potato-group`, `ck-row`, `ck-row-title`, `ck-row-main`, `ck-row-glyph`, `ck-sub`, `ck-scroll`, `ck-icon-btn` (found in `ThreadView.tsx`, `BotList.tsx`, `BotPanel.tsx`).
- Cockpit root: `.cockpit` (`styles/cockpit.css`); settings root `.settings` (55 selector hits in CSS).
- Sandbox UI: `sbx-` prefix (`sbx-hint`, `sbx-buttons`, `sbx-summary`, `sbx-choices` in `settings/sandbox/*.tsx`).
- CSS files: `packages/app/src/styles/cockpit-potato.css` and `cockpit-potato-{advanced,artifacts,cards,composer,firstrun,list,overlay,panel,routines,settings,starters,thread,trash,ui,voice}.css`; `potato-sandbox.css`; `potato-settings-pages.css`.
- Potato token use: `var(--label...)`, `var(--sep...)`, `var(--ease...)`, `var(--focus...)`.

### 10b. Colour and design rules
- `docs/design/theme.md`: tokens live in five roots; the table's first four are `styles/cockpit.css` `.cockpit`, `styles/phone.css` `.ph`, `styles/askbar.css` `.askbar`, `styles/point-box.css` `.point-box` (see the file for the fifth). Dark surfaces are charcoal; one accent, Potato orange, used in 1-5% of places. "No red, green or blue; no gradients; no coloured badges" (rule kept from `mockups/ui-proposal/STYLE.md`).
- `docs/design/cockpit-contract.md`: Rust-to-UI contract for Cockpit v1 (branch `feat/ui-monochrome`).
- `docs/design/glass-redesign.html`, `_event.html`, `_mobile.html`, `_preview.html`, `desktop-flow-demo.html`: static mock pages.

### 10c. Navigation and registration
- Sidebar row and palette entry: `packages/app/src/components/cockpit/Sidebar.tsx` and `Palette.tsx` (both reference potato; shown only when `potatoOn`).
- Page state: `packages/app/src/CockpitApp.tsx` l.134-170: `view === 'potato'` (`setView('potato')` at l.142), `openPotatoBot(id)`, `readOpenBot`/`writeOpenBot` in `hooks/usePotato.ts`. Tray event `cockpit://view` with `view: 'potato'` (l.156-158).
- Render: `CockpitApp.tsx` l.515 `<PotatoPage state version transport=tauriPotatoTransport ... />`.
- Settings section: `components/settings/catalog.ts` l.23 `BASE_SECTIONS` entry `id: 'potato'`.

### 10d. Tauri command from TypeScript (real example)
- File `packages/app/src/lib/potato.ts`:
  - l.1-2: `import { invoke } from '@tauri-apps/api/core';` and `import { listen, type UnlistenFn } from '@tauri-apps/api/event';`
  - l.~162: `send: (botId, text) => invoke<SendOutcome>('potato_send', { botId, text }),`
  - Rust side: `packages/app/src-tauri/src/commands/potato.rs` l.45-48 `#[tauri::command] pub async fn potato_send(bot_id: String, text: String)`. Tauri maps camelCase JS args to snake_case Rust args.
- Errors: Rust returns `Err(String)` with `CODE: sentence`; `lib/potato.ts` `plainError(error, fallback)` strips the code.
- Events: `listen<{ botId?: string } | null>('potato://changed', ...)` in `lib/potato.ts`.

---

## 11. Could not find / not verified

- `packages/potato-brain/src/rooms/`: does not exist.
- `feat/tag-your-agent` as a branch: gone (merged; use commit `856c0acc`).
- Discord code in the app or brain: none. Only the design note.
- Default value of `potato_bot_enabled` and `potato_sandbox_enabled` in the settings store: not located (both documented as off).
- Fishd source (`C:\Users\Ren-pc\Desktop\work2\Agent\`): NOT read by me. Fishd module names come from `D:/pm-research/INV-FISHD.md` and `INV-FISHD-DISCORD.md`.
- `potato_send_files` (lib.rs l.704) purpose not read; `attach_file` brain tool not found by name in this pass.
- Rakazo table: statuses from grep and the port map doc, not a line-by-line check.
- Neither `potato/bot_settings.rs` duplicate/archive features nor reactions in the DM composer were confirmed by reading their code.
- `packages/potato-brain/src/pm.ts` header says Puppet Master starts it with `node brain.mjs` in Rust comments; `package.json` says `node src/pm.ts`. Rust code's `script_path` (sidecar.rs l.124) was not read in full to settle which name it uses.
- Any `D:/pm-research/` file other than the four named was not opened, except for the sizes shown by `wc -l`.
