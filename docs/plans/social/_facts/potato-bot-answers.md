# Potato bot NEEDS-FACT answers

Read-only lookup of every NEEDS-FACT line in plans 10-15. Each answer quotes what was seen in code at the Source path:line. Items marked NOT FOUND list what was searched.

# NEEDS-FACT answers, part 1

Base for short names: `C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\` = APP_SRC. `C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\potato-brain\src\` = BRAIN_SRC.

### 10-potato-bot-overview.md / NEEDS-FACT the default value site of potato_bot_enabled
Answer: Defaults are in `default_public_settings()` (a `json!({...})` object): `"potato_bot_enabled": false` (line 121) and `"potato_sandbox_enabled": false` (line 123), each with a comment saying off until chosen. `potato_bot_enabled` is also the kill switch (comment at 120). The keys are also listed in two key arrays at lines 16-17 (DEV_LABS_KEYS, a dev-build override list) and 84-85 (the public settings key list; array name not read). Dev-labs test at line 280 asserts `with_dev_labs(&stored, false)["potato_bot_enabled"] == false`. The "public settings struct" is not a struct: settings are a `serde_json::Value` (`default_public_settings() -> Value`). Read path: `potato/live.rs:31` `LABS_KEY = "potato_bot_enabled"`, `live.rs:33` `SANDBOX_LABS_KEY = "potato_sandbox_enabled"`.
Source: APP_SRC\settings_store.rs:13-17, 84-85, 91-132 (defaults 121, 123), 280; APP_SRC\potato\live.rs:31, 33

### 10-potato-bot-overview.md / NEEDS-FACT rules::Facts, Origin, inbox item JSON shape
Answer:
- `rules::Facts<'a>` (rules.rs:61-71): `surface: &'a str` (`"app"` or outside surface name), `sender: Sender` (App | PairedOwner | Other), `channel: Channel` (NotAChannel | Approved(Who) | Pending | Unknown), `mentioned: bool`, `replied_to_bot: bool`, `from_bot: bool`, `age_ms: i64`, `text_is_empty: bool`.
- `Origin<'a>` (service.rs:171-174): `Person` | `Routine { title: &'a str, context: Option<&'a str> }`. No `Outside` variant exists.
- Facts built in `send_as` (service.rs:718-727): surface "app", Sender::App, Channel::NotAChannel, mentioned true, replied_to_bot false, from_bot false, age_ms 0, text_is_empty.
- Inbox item (protocol.rs:313-326, built by `item()` at 290-297): `{"v":1,"type":"send","id","conversationId","surface","from":{"id","name","trust"},"text","tools"}` plus optional `"context"`; then `with_capabilities` (protocol.rs:340-345) adds `"capabilities":{"sandbox":bool}`. trust words: `owner_pc | owner_remote | others` (protocol.rs:213-218); tools words: `full | no_memory` (protocol.rs:234-239). Other item types: `pause` (`reason`), `persona` (`pack`), `helper_update` (`requestId`,`status`,`summary`), `sandbox_result` (`callId`,`ok`,`result`), `sandbox_result` error (`error:{code,message,...}`).
- Brain-side reader (BRAIN_SRC\channels\potato.ts:553-573 `onSend`) reads `id`, `conversationId`, `text`, `from.id`, `from.name`, `from.trust`, `tools`, `context`, `capabilities.sandbox`.
- Brain events (protocol.rs:116-186 `parse_event`): every event `"v":1` and `"type"`. Known types: `hello` (protocol, version, provider, packs, surfaces), `typing` (conversationId, on), `reply` (conversationId, inReplyTo, segments[], kaomoji, tone), `mood` (mood, intensity, energy, hunger), `usage` (model, promptTokens, completionTokens, cacheHitTokens, cacheMissTokens), `helper_request` (requestId, conversationId, task, project), `error` (code, conversationId, inReplyTo), `sandbox_call` (callId, conversationId, tool, args, image). Unknown types become `Event::Unknown`.
Source: APP_SRC\potato\rules.rs:61-71; APP_SRC\potato\service.rs:171-174, 718-727, 801-813; APP_SRC\potato\protocol.rs:116-186, 290-297, 313-326, 340-345, 204-240; BRAIN_SRC\channels\potato.ts:553-573

### 10-potato-bot-overview.md / NEEDS-FACT Host methods for Sandbox state, store.rs helpers, keeper::backoff_ms, script_path release name
Answer:
- `trait Host` (service.rs:62-77) has NO sandbox methods. Its methods: `now_ms`, `local_day(ms)`, `bridge_url()`, `profile(bot)`, `host_env()`, `secret(name)`, `resources()`, `changed(bot)`, `reply_pushed(...)`. The names `sandbox_labs_on()` and `sandbox_installed()` used in the plan (file 11 lines 967-968) do not exist.
- Sandbox state lives in `Service`: `sandbox_on: AtomicBool` (service.rs:366), `set_sandbox_enabled(on)` (service.rs:404-406), `sandbox_offered()` (service.rs:409-411) = `brains.enabled() && sandbox_on && port.is_some()`. Port set by a setter at service.rs ~400 (`*self.port.lock() = port`).
- Free functions: `potato::live::sandbox_labs_on(public: &Value) -> bool` (live.rs:65-67), `potato::sandbox_host::labs_on(public)` (sandbox_host.rs:57-60, needs both LABS_KEY and LABS_KEY-for-sandbox true), `sandbox_host::enabled()` (sandbox_host.rs:62). Sandbox installed check: `sandbox_host::sync_into(on, manager, hub)` (sandbox_host.rs:133) and `wire_into(hub, sandbox: Option<&Arc<dyn Sandbox>>) -> Option<Port>` (sandbox_host.rs:111).
- store.rs helpers (potato/store.rs): `bots_path`, `load_bots`, `require_bot(root,bot)`, `save_bots`, `add_bot(root,id,talk_persona,now_ms)`, `set_talk_persona`, `remove_bot`, `settings_path`, `load_settings(root)`, `save_settings(root,&Settings)`, `app_conversation(bot) -> String`, `surface_of(conversation_id)`, `app_bot_of(conversation_id)`, `conversation_path`, `clean_text(text, home)`, `read_messages(root, conv, limit, home)`, `append_message(root, conv, message, home)`, `load_reads`, `mark_read(root,bot,at_ms)`, `brain_dir`, `vault_dir`, `persona_dir`, `share_dir`, `state_dir`, `ensure_brain_dirs`, `seed_persona(root,bot,resource_persona)`, `forget_talk_memory`.
- `keeper::backoff_ms` (potato/keeper.rs:24): `pub fn backoff_ms(crashes_so_far: usize) -> i64` (shift capped at 16; `(BACKOFF_FIRST_MS << shift).min(BACKOFF_MAX_MS)`). Not the same as `companion/keeper.rs:35`, which returns `Option<i64>`.
- script_path release name (sidecar.rs:123-133): `script_path(resources: Option<&Path>) -> Option<PathBuf>`. Dev: env `PM_POTATO_BRAIN` only under feature `dev-fake-tui` (125-128). Release: `resources/brain.mjs`, with `without_verbatim_prefix`, returned only if `is_file()`.
Source: APP_SRC\potato\service.rs:62-77, 366, 404-411; APP_SRC\potato\live.rs:65-67; APP_SRC\potato\sandbox_host.rs:57-62, 111, 133; APP_SRC\potato\store.rs:101-633 (fn list); APP_SRC\potato\keeper.rs:24-27; APP_SRC\potato\sidecar.rs:123-133

### 10-potato-bot-overview.md / NEEDS-FACT bridge path guard lines, brain/link token header, phone session type, route module file
Answer:
- Bridge guard: `bridge/http.rs:121-129` calls `crate::potato::auth::gate(brains, method, segments, headers_raw, peer_loopback, peer)`; Verdict `Pass` falls through, `Brain(bot_id)` returns `Caller::Brain{bot_id}`, `Refuse(status, body)` returns the error. Brain routes: `is_brain_target` (http.rs:370-375) uses `potato::auth::is_brain_path(segments)` = `segments[0]=="potato" && segments[1]=="brain"` (auth.rs:137-140). Brain-route OPTIONS and errors get no CORS (http.rs:206-207, 215).
- Token header: `X-PM-Potato-Token` (`potato/auth.rs:43`, `TOKEN_HEADER`). Error code `POTATO_SCOPE` (auth.rs:44). Brain side sends it in potato.ts:232 `{ "X-PM-Potato-Token": this.opts.token, ... }`. Env var for the brain token: `PM_POTATO_TOKEN` (env.rs:143).
- Phone session type: `enum Audience { App, Device(String) }` (potato/sandbox/screen.rs:142-147), wire form `"app"` or `"dev:<pairing id>"` (screen.rs:150-161). Only the app or a named paired phone can use a link. Phone control needs yes at PC: `ScreenError::PhoneNeedsDesktopYes` (screen.rs:208-209) and `allow_phone_control(device_id)` (screen.rs:441).
- Route module files: `bridge/routes/potato_brain.rs` (`/potato/brain/inbox` long poll and `/potato/brain/events` POST; other paths under /potato/brain/ are 404) and `bridge/routes/potato_screen.rs` (`/potato/sandbox/screen/<sealed>/...` viewer and `POST /potato/sandbox/screen-link`).
Source: APP_SRC\bridge\http.rs:121-129, 206-215, 370-375; APP_SRC\potato\auth.rs:43-44, 137-140; BRAIN_SRC\channels\potato.ts:232; APP_SRC\potato\env.rs:143; APP_SRC\potato\sandbox\screen.rs:142-161, 208-209, 441; APP_SRC\bridge\routes\potato_brain.rs:1-10; APP_SRC\bridge\routes\potato_screen.rs:1-13

### 10-potato-bot-overview.md / NEEDS-FACT trait Sandbox methods for guest commands and file copy; screen connection direction; distro packages
Answer:
- `trait Sandbox` (potato/sandbox/mod.rs:140-180) has NO guest-command or file-copy methods. Its methods: `status`, `install(options, progress)`, `start`, `stop`, `snapshot(name)`, `reset(snapshot)`, `remove`, `observe`, `act(actions)`, `browser(args)`, `screen_target()`, `person_holds(until_ms)`, `installed_desktops`, `active_desktop`, `add_desktop(word, progress)`, `switch_desktop(word)`. Guest commands are on the concrete WSL type: `WslSandbox`-level `exec(name, user: GuestUser, argv, stdin, timeout)` (sandbox/wsl.rs:578-590) and `exec_streaming(...)` (wsl.rs:592). Guest runs as `-d <name> -u <user> --exec`. File copy: only `--import <name> <dir> <tar>` and `--export` (wsl.rs:349-356, 565); no copy-into-guest method was found on the trait.
- Screen connection direction: `ScreenTarget { view_port: u16, generation: u64, passwords: Option<Arc<ScreenSecrets>> }` (sandbox/mod.rs:127-137). Doc: "Where the screen can be reached from this computer (loopback ports of the forwards WSL makes)." noVNC view port; the app connects to 127.0.0.1 forwards that WSL sets up. `screen_target()` at wsl.rs:559-565 uses `lifecycle.view_port()`.
- Distro packages / desktops (sandbox/install.rs:236-271): `Desktop` = Light | Xfce | Zorin | Mint. recipe_id: Light->"light", Xfce|Zorin->"xfce", Mint->"mint"; Zorin also installs recipe "zorin-look" (install.rs:247-252). Sessions: Light->fluxbox, Xfce|Zorin->xfce4, Mint->cinnamon (install.rs:255-261). Wizard words: light, ubuntu (=Xfce), zorin, mint (install.rs:264-277). Apt package names as such were not found in this file (only the install-step name "Installing packages" and the header note about systemd/apt at install.rs:18).
Source: APP_SRC\potato\sandbox\mod.rs:127-180; APP_SRC\potato\sandbox\wsl.rs:349-356, 559-565, 578-592; APP_SRC\potato\sandbox\install.rs:18, 236-277

### 10-potato-bot-overview.md / NEEDS-FACT Tag Your Agent token name, group run start point, poller reuse
Answer:
- Token name: the Telegram bot token credential key is `channels/<profile or "default">/telegram_bot_token` (`key_name_for(profile)`, token.rs:28-30; tests at 153-154). Tag-bot switch setting key: `tag_bot` (setup_tag.rs:100, 134).
- Group run start point: `GroupEngine::on_group_message` (channels/engine/group.rs:139-201) checks tag/mention, group config, who (OnlyMe checks paired user), limits (inflight, per-person, daily), then pushes `Effect::Ask(GroupJob{...})` at group.rs:200. `GroupJob` (group.rs:50) is run later by `answer_job(host, job)` (group.rs:236), which calls `host.ask_group(&ask)` (group.rs:241) off the poll thread (host.rs:76-78).
- Poller reuse: `telegram/poller.rs:97-104` `spawn(api, engine, host, generation: Arc<AtomicU64>, link)` runs while `generation` still equals its value at spawn; "a bump stops it" (poller.rs:95-96). A newer poller makes older ones stop (poller.rs:172 "True until a newer poller has been started"). `spawn_with(..., Timing)` is for tests (poller.rs:107-108). Spawn is not described as reuse; each start creates a new poller and bumps the generation.
Source: APP_SRC\channels\token.rs:28-30, 153-154; APP_SRC\channels\setup_tag.rs:100, 134; APP_SRC\channels\engine\group.rs:50, 139-201, 236-247; APP_SRC\channels\host.rs:76-78; APP_SRC\channels\telegram\poller.rs:95-108, 161-172

### 10-potato-bot-overview.md / NEEDS-FACT persona values today, pack files and sources, release checklist location, Review Desk entry point
Answer:
- Persona values: `pub const PACKS: [&str; 2] = ["fish", "potato"]` (potato/store.rs:78); `valid_pack` (store.rs:80-81). Bot talk persona field doc: "`fish` or `potato`" (store.rs:89). Default for new bots was not found in the lines read.
- Pack files: `packages/potato-brain/persona/packs/fish/{core.md,rules.md}` and `packages/potato-brain/persona/packs/potato/{core.md,rules.md}`, plus `packages/potato-brain/persona/protocol.md`. Source note: `packages/potato-brain/NOTICE.md` says the Fish pack is a fan character, a distilled retelling; checking it may ship is "item 1 of the release checklist"; NOTICE.md is based on fishd by the Puppet Master founder, MIT relicensed.
- Release checklist location: NOT FOUND. Searched the whole repo (excluding node_modules) for "release checklist", "Release checklist", "release-checklist", "RELEASE_CHECK" and the docs folder for release/checklist filenames. Only hits: plan files 10, 13 and 15 and packages/potato-brain/NOTICE.md (which only mentions it).
- Review Desk entry point: `CockpitApp.tsx` imports `ReviewDesk` from `./components/cockpit/review/ReviewDesk` (line 25) and renders `<ReviewDesk` (line 479). Line 114 comment: `reviewFirst` opens it on its first row ("Review the riskiest item" in the palette). Helper modules: `src/lib/review-desk.ts`, `src/lib/review-desk-pending.ts`.
Source: APP_SRC\potato\store.rs:78-81, 89; C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\potato-brain\persona\packs\fish\core.md (dir listing); C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\potato-brain\NOTICE.md:5-7; C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src\CockpitApp.tsx:25, 114, 479

### 11-potato-bot-discord.md / NEEDS-FACT the exact Facts field names (line 889)
Answer: same struct as in the overview item. `rules::Facts` fields: `surface`, `sender`, `channel`, `mentioned`, `replied_to_bot`, `from_bot`, `age_ms`, `text_is_empty` (rules.rs:61-71). In `send_as`, `surface: "app"` (service.rs:719). NOTE: the plan's `Outside(o)` does not exist; `Origin` has only `Person` and `Routine{title, context}` (service.rs:171-174). Surface/trust for outside surfaces is not built in code yet (rules_tests uses surface "outer" as data only).
Source: APP_SRC\potato\rules.rs:61-71; APP_SRC\potato\service.rs:171-174, 718-727

### 11-potato-bot-discord.md / NEEDS-FACT where runTurn adds notes; pass the same option used for trust === "others" (line 892)
Answer: `runTurn` is `private async runTurn(t: {...})` at BRAIN_SRC\channels\potato.ts:624-670. It passes to `agent.turn(...)`: `where`, `tools: t.tools`, `sandbox` (only when `t.sandbox`), `owner: t.trust !== "others"` (line 653), `group`, `speaker`, `context`, `spontaneous/reason`, and `note`. The owner note is added in `core/loop.ts:904-906` only when `opts.owner` is true. The self-notes block is added at loop.ts:908 only when `memoryOpen`. `tools: "no_memory"` disables vault and notes (loop.ts:77, 898-899). There is NO `sandboxOnly` option anywhere in BRAIN_SRC (searched). The existing switch for "others" is `owner: false` plus `tools: "no_memory"`.
Source: BRAIN_SRC\channels\potato.ts:624-670 (653), 541, 567; BRAIN_SRC\core\loop.ts:77, 898-908

### 11-potato-bot-discord.md / NEEDS-FACT the Host method name (line 967-968, sandbox_labs_on and sandbox_installed)
Answer: NOT FOUND as Host methods. `trait Host` (service.rs:62-77) has no `sandbox_labs_on` or `sandbox_installed`. Searched potato/ for `sandbox_labs_on`, `sandbox_installed`, `sandbox_on`. Real equivalents: `Service::sandbox_offered()` (service.rs:409-411), `Service::set_sandbox_enabled(on)` (service.rs:404-406), the free fn `potato::live::sandbox_labs_on(&Value)` (live.rs:65-67), and `sandbox_host::labs_on(&Value)` (sandbox_host.rs:57-60). Sandbox "installed" is represented by a wired port (`port.is_some()` in sandbox_offered).
Source: APP_SRC\potato\service.rs:62-77, 404-411; APP_SRC\potato\live.rs:65-67; APP_SRC\potato\sandbox_host.rs:57-60

### 11-potato-bot-discord.md / NEEDS-FACT the file that lists command modules (line 1328)
Answer: `packages/app/src-tauri/src/commands/mod.rs` lists the command modules with `pub mod x;` lines (about 81 declarations, e.g. `pub mod attachments;` style). Verified by grep count.
Source: APP_SRC\commands\mod.rs (81 `pub mod` / `mod` lines)

### 11-potato-bot-discord.md / NEEDS-FACT SecretMask fields (line 1403, secrets.rs l.158)
Answer: `pub struct SecretMask { pub set: bool, pub last4: Option<String> }` (secrets.rs:158-162). `last4` is only set when the secret has at least 12 characters (`mask()` at 164-173). Doc: "Last 4 characters, only for keys long enough that 4 characters reveal little."
Source: APP_SRC\secrets.rs:158-173

### 11-potato-bot-discord.md / NEEDS-FACT the header that carries PM_POTATO_TOKEN (line 1917)
Answer: The header is `X-PM-Potato-Token` (`potato/auth.rs:43`, `TOKEN_HEADER`). The env var `PM_POTATO_TOKEN` is the brain's secret (pm.ts:35 reads it; env.rs:143 sets it). Brain sends `"X-PM-Potato-Token": this.opts.token` (potato.ts:232). Note: the plan says "copy exactly what potato.ts sends ... but with linkToken" (no link-token header exists in code; NOT FOUND for any `linkToken` / link token header).
Source: APP_SRC\potato\auth.rs:43; BRAIN_SRC\pm.ts:35; BRAIN_SRC\channels\potato.ts:232; APP_SRC\potato\env.rs:143

### 11-potato-bot-discord.md / NEEDS-FACT the 2036 apply_settings and SpawnSpec for discord.ts (line 2036)
Answer: `potato/live.rs:86` `pub fn apply_settings(public: &Value)` exists and computes `let sandbox = sandbox_labs_on(public)` (live.rs:89) and `let on = labs_on(public)` (live.rs:95). NOT FOUND for `discord_labs_on`, `link_keeper`, `discord.ts`, or any Discord code: searched all of packages/ (excluding node_modules) for `link_keeper`, `discord_labs_on`, `discord.ts`; app src-tauri has only `discord_bot_token` in `secrets.rs:31` (`SIDECAR_SECRETS`) and 3 test-file mentions. `SpawnSpec` exists but is not the potato one: `acp/conn.rs:42` and `companion/host.rs:12`. Brain script: `script_path` returns only `brain.mjs` in release (sidecar.rs:131), so there is no second script.
Source: APP_SRC\potato\live.rs:86-95; APP_SRC\secrets.rs:31; APP_SRC\acp\conn.rs:42; APP_SRC\companion\host.rs:12; APP_SRC\potato\sidecar.rs:124-133

### 11-potato-bot-discord.md / NEEDS-FACT the exact signature of keeper::backoff_ms (line 2092)
Answer: `pub fn backoff_ms(crashes_so_far: usize) -> i64` (potato/keeper.rs:24). Returns `i64` (not Option). Body: `let shift = crashes_so_far.min(16) as u32; (BACKOFF_FIRST_MS << shift).min(BACKOFF_MAX_MS)`. Note: `companion/keeper.rs:35` has a different `backoff_ms(crashes_so_far: usize) -> Option<i64>` (table lookup); the plan must pick the potato one.
Source: APP_SRC\potato\keeper.rs:24-27; APP_SRC\companion\keeper.rs:35-37

### 11-potato-bot-discord.md / NEEDS-FACT the app's helper for opening an external URL (line 2338)
Answer: The app helper is `tauri.openExternalUrl(url: string)` in `packages/app/src/lib/tauri.ts:365-366`: `safeInvoke<void>('open_external_url', { url }, undefined, true)`. Used by `hooks/useAppUpdateCheck.ts:52`. `@tauri-apps/plugin-opener` is NOT used (searched packages/app/src for `plugin-opener`, `openExternal`, `shell.open`, `opener`: no import). `pwa/push-core.ts:130 openUrl` is a URL builder, not the opener.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src\lib\tauri.ts:365-366; C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src\hooks\useAppUpdateCheck.ts:52

### 11-potato-bot-discord.md / NEEDS-FACT the brain item id field name (line 2475)
Answer: field name is `id`. The brain reads `m["id"]` in `onSend` (potato.ts:554) and Rust writes `"id": s.id` in `send_item` (protocol.rs:315). The send item is the one from `send_as` (service.rs:801-813), where `id` is `saved.id`. Duplicate-check: `handled` list keyed by id (potato.ts:561-562).
Source: BRAIN_SRC\channels\potato.ts:554-562; APP_SRC\potato\protocol.rs:313-326; APP_SRC\potato\service.rs:801-813

### 11-potato-bot-discord.md / NEEDS-FACT the notes test (line 2495)
Answer: same as item 892 (line 892). The notes gate is `opts.owner` (loop.ts:904) for the owner note and `memoryOpen` (loop.ts:908) for self-notes. No `sandboxOnly` option exists, so a test keyed on it cannot be written as the plan says.
Source: BRAIN_SRC\core\loop.ts:904-908; BRAIN_SRC\channels\potato.ts:653

### 11-potato-bot-discord.md / NEEDS-FACT labs_on(public) parameter type and helper (line 2510)
Answer: `pub fn labs_on(public: &Value) -> bool { public.get(LABS_KEY).and_then(Value::as_bool).unwrap_or(false) }` (potato/live.rs:60-62). "Exactly true" helper: `sandbox_labs_on(public: &Value) -> bool` (live.rs:64-67) uses the same `and_then(Value::as_bool).unwrap_or(false)` pattern, so only a JSON boolean `true` counts (test screen_tests.rs:397-400 shows `"true"` string is off).
Source: APP_SRC\potato\live.rs:60-67; APP_SRC\potato\sandbox\screen_tests.rs:397-400

### 11-potato-bot-discord.md / NEEDS-FACT how *_tests.rs files are attached (line 2511)
Answer: `#[cfg(test)] mod <name>_tests;` in `potato/mod.rs`, e.g. lines 75-86: `#[cfg(test)] mod auth_tests;`, `#[cfg(test)] mod bot_settings_tests;`, `#[cfg(test)] mod cost_tests;`, `#[cfg(test)] mod reply_cost_tests;`. A few are declared without `cfg(test)` (lines 76-78: `mod attachments_tests; mod artifacts_tests; mod voice_tests;`).
Source: APP_SRC\potato\mod.rs:75-86

### 11-potato-bot-discord.md / NEEDS-FACT Facts fields and enum Origin (line 2512)
Answer: same as item 889 for `Facts`. `enum Origin<'a> { Person, Routine { title: &'a str, context: Option<&'a str> } }` (service.rs:171-174). No `Outside` variant. Sender/Channel enums in rules.rs:31-58 (Sender: App, PairedOwner, Other; Channel: NotAChannel, Approved(Who), Pending, Unknown).
Source: APP_SRC\potato\rules.rs:31-71; APP_SRC\potato\service.rs:171-174

### 11-potato-bot-discord.md / NEEDS-FACT the JSON shape of an inbox item built by push_to_brain (line 2513)
Answer: `push_to_brain(&self, bot: &str, item: Value) -> bool` (service.rs:1080-1090) does not build the shape; it pushes the given `Value` into `session.inbox` unchanged. Shape comes from `protocol::item()` (protocol.rs:290-297): `{"v":1,"type":<kind>, ...fields}`. For a send: `id`, `conversationId`, `surface`, `from:{id,name,trust}`, `text`, `tools`, optional `context`, `capabilities:{sandbox}` (protocol.rs:313-345). Brain reader: potato.ts:553-573 (`onSend`). `kind` field is `type`, not `kind`.
Source: APP_SRC\potato\service.rs:1080-1090; APP_SRC\potato\protocol.rs:290-345; BRAIN_SRC\channels\potato.ts:553-573

### 11-potato-bot-discord.md / NEEDS-FACT how Service knows Sandbox installed and Labs on (line 2514)
Answer: Labs on: `Service::set_sandbox_enabled(on)` stores `sandbox_on` (service.rs:404-406); `Service::sandbox_offered()` = brains enabled && sandbox_on && port is set (service.rs:409-411). Installed: no separate flag; it is the wired port set by `sandbox_host` (`wire_into` at sandbox_host.rs:111, `sync_into` at 133). `Host` (service.rs:62-77) has no sandbox methods. Searched sandbox_host.rs for "installed": only comments at line 72.
Source: APP_SRC\potato\service.rs:62-77, 366, 404-411; APP_SRC\potato\sandbox_host.rs:57-62, 111, 133

### 11-potato-bot-discord.md / NEEDS-FACT where runTurn adds the owner's memory notes and vault per bot or per conversation (line 2515)
Answer: Owner note: `runTurn` (potato.ts:624-670) passes `owner: t.trust !== "others"`; `core/loop.ts:904-906` adds the owner note only when `opts.owner`. Vault: ONE shared vault for all bots (`agents.ts:37-45`: "One shared vault, one shared mood, one shared cache log, but a separate conversation window per room"; `new VaultMemory(config.vaultRoot)` at agents.ts:45). Conversation windows are per room (conversationId). Vault root is `FISHD_VAULT` env or "vault" (BRAIN_SRC\config.ts:24). Rust side has per-bot dirs `vault_dir(root, bot)` (store.rs:536), which is for the app's copy of the bot's files.
Source: BRAIN_SRC\channels\potato.ts:624-670; BRAIN_SRC\core\loop.ts:904-906; BRAIN_SRC\agents.ts:37-45; BRAIN_SRC\config.ts:24-30; APP_SRC\potato\store.rs:528-540

### 11-potato-bot-discord.md / NEEDS-FACT the HTTP client crate and call style in ask_model (line 2516)
Answer: `ureq` (blocking). `ask_model(key: &str) -> Result<(), String>` (live.rs:266): `ureq::AgentBuilder::new().timeout_connect(8 s).timeout_read(20 s).redirects(0).build()`; `agent.post("https://api.deepseek.com/chat/completions").set("Authorization", "Bearer {key}").send_json(body)` with body `{model:"deepseek-chat", messages:[{role:"user",content:"hi"}], max_tokens:1}`. Status 200 is ok; 401, 402, 429, other status and transport errors map to fixed messages (live.rs:273-280).
Source: APP_SRC\potato\live.rs:266-280

### 11-potato-bot-discord.md / NEEDS-FACT the file that declares Tauri command modules (line 2517)
Answer: `packages/app/src-tauri/src/commands/mod.rs` (81 `pub mod` lines; see line 1328 answer).
Source: APP_SRC\commands\mod.rs

### 11-potato-bot-discord.md / NEEDS-FACT whether secret_set allows discord_bot_token and SecretMask names (line 2518)
Answer: Yes. `SIDECAR_SECRETS = ["deepseek_api_key", "discord_bot_token"]` (secrets.rs:31). `is_ui_secret(name)` = `is_provider_secret(name) || is_sidecar_secret(name)` (secrets.rs:37-40), so `discord_bot_token` is UI-settable. `SecretMask` fields: `set: bool`, `last4: Option<String>` (secrets.rs:158-162). Note the test at secrets.rs ~364 lists it.
Source: APP_SRC\secrets.rs:31-40, 158-162, 364

### 11-potato-bot-discord.md / NEEDS-FACT the header that carries PM_POTATO_TOKEN on brain requests (line 2519)
Answer: same as item 1917 (`X-PM-Potato-Token`, potato/auth.rs:43; sent by potato.ts:232).
Source: APP_SRC\potato\auth.rs:43; BRAIN_SRC\channels\potato.ts:232

### 11-potato-bot-discord.md / NEEDS-FACT what bridge/http.rs l.123 and l.370 check (line 2520)
Answer: l.123-129: `potato::auth::gate(...)` decides; a brain is served `/potato/brain/*` only (404 for non-brains, 403 POTATO_SCOPE elsewhere). l.370-375 `is_brain_target(target)` = segments start `potato`,`brain` via `auth::is_brain_path` (auth.rs:137-140). Used at http.rs:206, 215, 587, 599 to suppress CORS and return 404 for OPTIONS. To add `/potato/discord/` the same way, the brain-only check would need a parallel `is_discord_path` and a branch in `gate`; no such path exists today. `gate_with` at http.rs:111-139 also calls `routes::potato_screen::gate` for the screen viewer after the Potato gate.
Source: APP_SRC\bridge\http.rs:111-139, 206-215, 370-375, 587-600; APP_SRC\potato\auth.rs:137-140

### 11-potato-bot-discord.md / NEEDS-FACT how script_path names the brain script in dev and release (line 2521)
Answer: Release: `script_path(resources)` returns `resources/brain.mjs` (sidecar.rs:131), only if it is a file. Dev: `PM_POTATO_BRAIN` env replaces it, but only in a build with feature `dev-fake-tui` (sidecar.rs:125-128). `src/pm.ts` is the brain's entry in packages/potato-brain (pm.ts reads PM_POTATO_TOKEN, pm.ts:35), not the path the app runs in release. resource_dirs.rs was not read in full (name of the bundled brain file there not checked).
Source: APP_SRC\potato\sidecar.rs:123-133; BRAIN_SRC\pm.ts:13, 35

### 11-potato-bot-discord.md / NEEDS-FACT the signature of keeper::backoff_ms (line 2522)
Answer: same as item 2092 (`potato/keeper.rs:24`, `pub fn backoff_ms(crashes_so_far: usize) -> i64`).
Source: APP_SRC\potato\keeper.rs:24-27

### 11-potato-bot-discord.md / NEEDS-FACT the app's helper for opening an external URL (line 2523)
Answer: same as item 2338 (`tauri.openExternalUrl` in packages/app/src/lib/tauri.ts:365-366). No `plugin-opener` import found.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src\lib\tauri.ts:365-366

### 11-potato-bot-discord.md / NEEDS-FACT how Potato sub-pages are registered (line 2524)
Answer: `makePotatoSubPageRows(transport, usage = tauriUsageTransport): RowDef[]` (PotatoSubPages.tsx:80-96) returns two rows: "Models and fallbacks" (`render: () => <ModelsRow transport=.../>`, line 88) and "Usage" (`render: () => <UsageRow usage=.../>`, line 96). Each row opens a `SubPageSheet` (line 16, a full-window layer). PotatoSection.tsx:16 imports `makePotatoSubPageRows` and includes these rows in Settings.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src\components\settings\potato\PotatoSubPages.tsx:16, 42-96; C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src\components\settings\sections\PotatoSection.tsx:16

## Count
- File 10-potato-bot-overview.md: 7 items, 7 answered (some with partial NOT FOUND: release checklist location; Host sandbox methods; trait Sandbox guest/file-copy methods; Tag Your Agent is read from channels/ telegram code, not a named plan component).
- File 11-potato-bot-discord.md: 27 items, all answered from code (several with NOT FOUND sub-points: Host sandbox methods, sandboxOnly, discord link/keeper code, link-token header).

## Plan lines that look wrong or contradictory to the code
- 11-potato-bot-discord.md line 889 and 2512: `Outside(o)` does not exist; `Origin` has only `Person` and `Routine`.
- 11-potato-bot-discord.md lines 967-968: `self.host.sandbox_labs_on()` / `sandbox_installed()` are not on `trait Host`.
- 11-potato-bot-discord.md lines 892, 2495: a `sandboxOnly` option does not exist in packages/potato-brain.
- 11-potato-bot-discord.md line 2036 and following: no Discord code exists in packages/app/src-tauri or packages/potato-brain yet (only the `discord_bot_token` secret name).
- 11-potato-bot-discord.md line 1917 / 2519: "linkToken" header does not exist in code.
- 11-potato-bot-discord.md line 2092 / 2522: plan's keeper::backoff_ms is the potato one (returns i64), not companion/keeper.rs (returns Option).
# NEEDS-FACT answers, part 2

Base: C:\Users\Ren-pc\Desktop\work2\puppet-master

## Plan 12-potato-bot-app-groups.md

### 12-potato-bot-app-groups.md / NEEDS-FACT `typing` (NEEDS-FACT). Copy them from `potato.ts`.
Answer: Brain-side emits (potato.ts). Typing: `{ type: "typing", conversationId, on: boolean }` (emit at potato.ts:645 and :668). Reply: `{ type: "reply", conversationId, inReplyTo, segments, kaomoji? (only when set), tone }` (potato.ts:673-680). Mood and usage follow in the same report() call (potato.ts:681-695). Error event: `{ type: "error", code, conversationId, inReplyTo }` (potato.ts:638). Rust parser agrees: protocol.rs:129 (`typing`: "conversationId", "on"), protocol.rs:130-140 (`reply`: "conversationId", "inReplyTo", segments). Every event carries `v: 1` (protocol.rs:5).
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\potato-brain\src\channels\potato.ts:645

### 12-potato-bot-app-groups.md / NEEDS-FACT: the `TurnOptions` field for extra system text
Answer: No field is named for "extra system text". Closest existing fields in `TurnOptions` (loop.ts:258-291): `context?: string` = "text already fenced by the host, shown before the message and kept out of the journal" (loop.ts:285-286), and `note?: string` = "one line of fact from the host, shown in the tip" (loop.ts:287-288). The host passes `context` in runTurn as `...(t.context ? { context: t.context } : {})` (potato.ts:656). Call shape: `agent.turn(t.text, { where, tools, sandbox?, owner, group, speaker?, context?, spontaneous?/reason?, note? })` (potato.ts:649-659). `Agent` class: loop.ts:342; `turn(userText, opts)`: loop.ts:839. `agentFor` is private in PotatoChannel (potato.ts:471-485), calls `opts.agentFor(conversationId, [tool, ...computer])`. Turn result is `TurnResult` (loop.ts:25); report() reads `r.segments`, `r.kaomoji`, `r.tone`, `r.mood.*`, `r.model`, `r.usage.*` (potato.ts:672-696). `runRoomTurn` / `parseRoomItem` do not exist in code yet (grep of packages/ found none).
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\potato-brain\src\core\loop.ts:285

### 12-potato-bot-app-groups.md / NEEDS-FACT (line 934): the existing item type field name
Answer: `type`. Rust builds every inbox/outbox item with `map.insert("type", kind)` in `fn item(kind, fields)` (protocol.rs:290-293). Brain dispatches inbox items on `raw["type"]` (potato.ts:418-420, `switch (raw["type"])`) and ignores others in `default` (potato.ts:449-451). Plan line 934 should say `type`.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\potato\protocol.rs:293

### 12-potato-bot-app-groups.md / NEEDS-FACT (line 966): unknown-kind test in potato-channel.test.ts
Answer: YES, the test exists: `test("unknown types, other versions and broken messages are ignored", ...)` at potato-channel.test.ts:534. (Also `unknown` pack test at :485 is unrelated.)
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\potato-brain\test\potato-channel.test.ts:534

### 12-potato-bot-app-groups.md / NEEDS-FACT `POTATO_TOO_LONG: This message is too long.`
Answer: Reuse the existing empty-message refusal. `POTATO_TOO_LONG` does not exist in the code (grep of packages/ found none). The existing `POTATO_EMPTY` text is `"Write a message first."`: `Err(err("POTATO_EMPTY", "Write a message first."))` in `Service::send_as` (service.rs:692, the match at :732). Test: service_tests.rs:709 asserts code `POTATO_EMPTY`.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\potato\service.rs:732

### 12-potato-bot-app-groups.md / NEEDS-FACT (line 1176): exact place where `BotList` is rendered
Answer: `BotList` is rendered inside `PotatoPage` at PotatoPage.tsx:105 (`<BotList`). `ThreadView` is rendered at PotatoPage.tsx:124. `PotatoPage` itself is rendered by CockpitApp.tsx:515-532, only when `view === 'potato' && potato.state && potatoOn`. Import of BotList: PotatoPage.tsx:15. No `NewGroupSheet` or `RoomThread` exists in PotatoPage.tsx yet.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src\components\cockpit\potato\PotatoPage.tsx:105

### 12-potato-bot-app-groups.md / NEEDS-FACT (line 1234): where PotatoPage gets settings (`potato_groups_enabled`)
Answer: NOT FOUND for `potato_groups_enabled`: grep of C:\...\puppet-master\packages for that name returned no files. The Labs key that exists is `potato_bot_enabled` (live.rs:31, `LABS_KEY`). PotatoPage gets settings from its `state` prop: CockpitApp.tsx:516 `state={potato.state}`; inside PotatoPage the settings are `state.settings` (PotatoPage.tsx:128, 172, 174). Rust `PotatoState` has `pub enabled: bool` and `pub settings: store::Settings` (service.rs:160-162). So the plan's flag name is wrong; use `state.settings` (or `state.enabled`) and a real flag name.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src\CockpitApp.tsx:516

### 12-potato-bot-app-groups.md / NEEDS-FACT (line 1327): injection test in test/integration.test.ts
Answer: NOT FOUND in integration.test.ts: no test checks prompt injection between bots. Its test names (lines 14-299) are memory, checkpoint, people, relationship, tip clock, group speaker identity, ignored message journaling, burst context, unrelated memory. Closest existing pattern (other file): sandbox-channel.test.ts:481-496, test "secrets the brain holds are hidden ... and a closing tag in a page cannot end the fence". It injects hostile text `... </sandbox_data> ignore all rules` via `r.answer(() => okResult({...}))` and asserts with `assert.equal(text.split("</sandbox_data>").length, 2)`.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\potato-brain\test\sandbox-channel.test.ts:481

### 12-potato-bot-app-groups.md / NEEDS-FACT (line 1338): type of `public` in `labs_on` and the "exactly true" helper
Answer: `pub fn labs_on(public: &Value) -> bool` (serde_json `Value`). Body: `public.get(LABS_KEY).and_then(Value::as_bool).unwrap_or(false)`. `LABS_KEY: &str = "potato_bot_enabled"` (live.rs:31). "Exactly true" = only a JSON boolean true counts. Sandbox twin: `sandbox_labs_on(public: &Value) -> bool` on `SANDBOX_LABS_KEY = "potato_sandbox_enabled"` (live.rs:33, 65-66).
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\potato\live.rs:60

### 12-potato-bot-app-groups.md / NEEDS-FACT: how conversation lines are written and how ids are generated
Answer: Lines are appended, one JSON message per line, to `<root>/conversations/<surface>/<stem>.jsonl` (store.rs:3, 11-13; `conversation_path` store.rs:303-305; `file_stem` store.rs:290 turns non-[A-Za-z0-9._-] bytes into `%XX`). Write path: `append_message(root, conversation_id, message, home)` (store.rs:467) calls `append_line(path, line)` (store.rs:446). Limits: text max 4000 chars per message, 2 MB or 5000 lines per file, then rotates to `.1.jsonl` (store.rs:12-13). Message id: `id: format!("m-{}", &uuid::Uuid::new_v4().simple().to_string()[..12])` (store.rs:363), i.e. `m-` plus 12 hex chars of a v4 UUID. Store `Message` ids are returned to the brain as `inReplyTo`.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\potato\store.rs:363

### 12-potato-bot-app-groups.md / NEEDS-FACT: the field names of emitted `reply`/`typing` events and inbox items
Answer: `type` is the discriminator everywhere (see item for line 934). Emitted: `reply` and `typing` use `conversationId` and `inReplyTo` (see first item). Inbound `send` item read in potato.ts:555-558 as `m["id"]`, `m["conversationId"]` (string, max 200), `text`, `from`. Rust Event parse: `text(value, "conversationId", 200)`, `text(value, "inReplyTo", 128)` (protocol.rs:139-140, 174-175). Brain ignores `typing` and `progress` when the queue is full (potato.ts:42, 321-325).
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\potato-brain\src\channels\potato.ts:555

### 12-potato-bot-app-groups.md / NEEDS-FACT (line 1341): `TurnOptions` field for extra system text and how `runTurn` reads the turn result
Answer: same as item for line 824 (no extra-system-text field; `context` is closest; turn result is `TurnResult` from `Agent.turn`, read in runTurn potato.ts:649-660 and report() potato.ts:672-696).
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\potato-brain\src\core\loop.ts:285

### 12-potato-bot-app-groups.md / NEEDS-FACT (line 1342): unknown inbox kinds test
Answer: same as item for line 966: YES, potato-channel.test.ts:534.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\potato-brain\test\potato-channel.test.ts:534

### 12-potato-bot-app-groups.md / NEEDS-FACT (line 1343): text of the existing `POTATO_EMPTY` refusal
Answer: same as item for line 1025: `"Write a message first."` (code `POTATO_EMPTY`), service.rs:732.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\potato\service.rs:732

### 12-potato-bot-app-groups.md / NEEDS-FACT (line 1344): where PotatoPage gets settings and renders BotList
Answer: same as items for line 1176 (BotList at PotatoPage.tsx:105) and line 1234 (settings via `state` prop, `state.settings`; `potato_groups_enabled` NOT FOUND). CockpitApp.tsx l.515 is the PotatoPage render site.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src\components\cockpit\potato\PotatoPage.tsx:105

### 12-potato-bot-app-groups.md / NEEDS-FACT (line 1345): whether `Composer` can take an `onSend(text)` prop
Answer: NO. `ComposerProps` (Composer.tsx:62-76) has: `bot: PotatoBotView`, `transport: PotatoTransport`, `onChanged: () => void`, `onNotify: (text: string) => void`, `onSent?: () => void`, `bots?: MentionTarget[]`, `actions?: ComposerActions`, `attach?: AttachTransport | null`. No `onSend`. Sending goes through `transport`. `ComposerBox` destructures these at Composer.tsx:131.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src\components\cockpit\potato\Composer.tsx:62

### 12-potato-bot-app-groups.md / NEEDS-FACT (line 1346): person's display name source for room lines
Answer: NOT FOUND for a person display name. Searched non-test files in packages/app/src-tauri/src/potato for owner/name/display/person. The only owner-side label is `Origin::Person`: `from_id` "owner", `from_name` "you", and the saved line is `Message::new(at, FromKind::You, None, MsgKind::Text, text)`, so no name is stored (service.rs:178-197). The shared `Speaker` struct has an optional name: `pub kind: FromKind`, `pub name: Option<String>` (store.rs:322-326).
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\potato\service.rs:180

### 12-potato-bot-app-groups.md / NEEDS-FACT (line 1347): existing prompt-injection test pattern
Answer: NOT FOUND in integration.test.ts (same as item for line 1327). Pattern in sandbox-channel.test.ts:481-496 (hostile text in a tool result, then assert the fence holds).
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\potato-brain\test\sandbox-channel.test.ts:481

## Plan 13-potato-bot-sandbox-finish.md

### 13-potato-bot-sandbox-finish.md / NEEDS-FACT: docs guard test lists feature docs (line 112)
Answer: NOT FOUND. Searched packages/app/src for test files matching `docs/features` and `features/`: only phone-handoff.test.tsx:20 mentions `docs/features/phone-lossless.md` in a comment, no test reads the docs folder. Searched scripts/: only a comment in scripts/evidence/live-claude-dialogs.mjs:1. No docs guard test found.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src\hooks\phone-handoff.test.tsx:20

### 13-potato-bot-sandbox-finish.md / NEEDS-FACT: where `PHONE_NEEDS_DESKTOP_YES` is returned today (line 124)
Answer: Constant `pub const PHONE_NEEDS_DESKTOP_YES: &str = "PHONE_NEEDS_DESKTOP_YES";` (screen.rs:52). Returned by `ScreenHub::link(&self, bot, mode, audience, base)` when mode is Control, audience is `Audience::Device(id)`, and `phone_yes` does not contain id: `return Err(ScreenError::PhoneNeedsDesktopYes);` (screen.rs:571-574). Error code mapping: `ScreenError::PhoneNeedsDesktopYes => PHONE_NEEDS_DESKTOP_YES` (screen.rs:226); message "Say yes on your PC first, then this phone can take control." (screen.rs:242); HTTP status 403 (screen.rs:253). Body: `code` field. Test asserts `(403, Some("PHONE_NEEDS_DESKTOP_YES"))` on `denied.1["code"]` (bridge/routes/potato_screen_tests.rs:604). Also screen_tests.rs:360. Note: potato_screen.rs itself does not name the code; the route forwards ScreenError.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\potato\sandbox\screen.rs:574

### 13-potato-bot-sandbox-finish.md / NEEDS-FACT: paired phone identity type (line 125)
Answer: `pub struct PairedDeviceInfo { pub id: String, pub name: String, pub public_key: String, pub paired_at: u64, pub caps: Caps, pub expires_at: u64, pub last_seen: Option<u64>, pub expired: bool, ... }` (mobile_pairing/mod.rs:89-101). Device id from a token: `device_id_for_token(&token)` (bridge/routes/digest.rs:9). Phone control gate takes `device_id: &str` (screen.rs:441 `allow_phone_control(&self, device_id: &str)`).
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\mobile_pairing\mod.rs:89

### 13-potato-bot-sandbox-finish.md / NEEDS-FACT: the struct that owns the lease at runtime (line 243)
Answer: `pub struct ScreenLease` (sandbox/lease.rs:191-192; fields `current: Option<Lease>`, `last_id`, `high_fence`, `card: Option<NeedsYou>`, `cards_made`, `parked`). It is owned by `pub struct ScreenHub` as `lease: Arc<Mutex<ScreenLease>>` (screen.rs:357-362, built at :388), exposed by `pub fn lease(&self) -> &Arc<Mutex<ScreenLease>>` (screen.rs:435). live.rs does not own it. `AllowList` and `Asks` do not exist in sandbox/ (grep found none).
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\potato\sandbox\lease.rs:192

### 13-potato-bot-sandbox-finish.md / NEEDS-FACT: cockpit root overlay slot (line 327, also 385)
Answer: NOT FOUND as a named slot. CockpitApp.tsx has no "overlay slot" component. Searched for overlay/slot in CockpitApp.tsx: only `closeOverlay` (CockpitApp.tsx:120) and overlay views rendered as view-ternary branches: StoreHost, PlansMonth, MorningPage, ReviewDesk (about lines 457-483, `onClose={closeOverlay}`). PotatoPage is rendered in the main ternary at CockpitApp.tsx:514-532. Root-level cards for the Potato page are not mounted there.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src\CockpitApp.tsx:120

### 13-potato-bot-sandbox-finish.md / NEEDS-FACT: `bundle.resources` keys (line 398, also 489)
Answer: tauri.conf.json `bundle.resources` (lines 26-37): "resources/mcp-stdio.bundle.cjs", "resources/puppet-master-cli.bundle.mjs", "resources/pwa-dist/*", "resources/pwa-dist/assets/*", "resources/bin/*", "resources/potato-brain/*", "resources/potato-brain/persona/*", "resources/potato-brain/persona/packs/fish/*", "resources/potato-brain/persona/packs/potato/*", "resources/potato-sandbox/**/*". Installed resource path: `<resource dir>/resources/<name>`, dev run may use `<resource dir>/<name>` (resource_dirs.rs:3-4, candidates at :29-31, find at :34). Windows build output path: Tauri resolves via `app.path().resolve(rel, BaseDirectory::Resource)` (resource_dirs.rs:40-42).
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\tauri.conf.json:26

### 13-potato-bot-sandbox-finish.md / NEEDS-FACT: PotatoState field style (line 405, also 1042)
Answer: `pub struct PotatoState` (service.rs:160-165), `#[serde(rename_all = "camelCase")]` on the struct (service.rs:159). Fields: `pub enabled: bool`, `pub settings: store::Settings`, `pub key: SecretMask`, `pub node: NodeView`, `pub bots: Vec<BotView>`. No `missing_files` field exists yet. Brain-state UI copy lives in BrainStateCard.tsx (see its item below).
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\potato\service.rs:160

### 13-potato-bot-sandbox-finish.md / NEEDS-FACT: REQUIRED_BRAIN / REQUIRED_SANDBOX names (line 408, also 1041)
Answer: Sandbox list exists: `const REQUIRED_FILES: [&str; 6]` in sandbox/install.rs:108-115: "guest/desktop/apply-recipe.py", "guest/desktop/pm-sbx-start-desktop.sh", "guest/egress/pm-sbx-egress.sh", "guest/egress/pm-sbx-egress-probe.sh", "guest/control/pm-sbx-control", "recipes/base.json". `GUEST_FOLDERS: [&str; 4] = ["desktop", "control", "egress", "page-browser"]` (install.rs:104). No `REQUIRED_BRAIN` constant found (grep of potato/ not run for brain list; the brain script name is in sidecar.rs, see next item).
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\potato\sandbox\install.rs:108

### 13-potato-bot-sandbox-finish.md / NEEDS-FACT: brain script name in release (line 419, 1041)
Answer: `brain.mjs`, not `src/pm.ts`. sidecar.rs:123-132: `pub fn script_path(resources)` returns `resources.join("brain.mjs")` (dev override is `PM_POTATO_BRAIN` under `dev-fake-tui`).
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\potato\sidecar.rs:131

### 13-potato-bot-sandbox-finish.md / NEEDS-FACT: every other guest file install.rs copies (line 427)
Answer: Guest files are walked from the folders in `GUEST_FOLDERS` (install.rs:104: desktop, control, egress, page-browser) by `fn collect(dir, prefix, out)` (install.rs:159) under `pub fn list(&self)` (install.rs:127); files filtered by `shipped()` which drops `*.md`, `test_*` and `*.pyc` (install.rs:117-118). `REQUIRED_FILES` (install.rs:108-115) must be present (`check`, install.rs:138). Exact per-file list is the folder walk, not a literal list.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\potato\sandbox\install.rs:104

### 13-potato-bot-sandbox-finish.md / NEEDS-FACT: where BrainStateCard copy lives (line 446, also 1042)
Answer: The copy is inline in the component: `export function BrainStateCard({ name, state, lastError, limitCents, onStart, onInstallNode }: BrainStateCardProps)` (BrainStateCard.tsx:19). Not in potato-firstrun.ts (not checked in code).
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src\components\cockpit\potato\BrainStateCard.tsx:19

### 13-potato-bot-sandbox-finish.md / NEEDS-FACT: release checklist location (line 490)
Answer: NOT FOUND. Glob for docs/**/*CHECKLIST* and docs/release/** returned no files.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\docs

### 13-potato-bot-sandbox-finish.md / NEEDS-FACT: exact resources path inside the Windows build output (line 499)
Answer: Installed copy: `<resource dir>/resources/<name>` (resource_dirs.rs:3-4, candidates `resources/{name}` then `{name}` at :29-31). Marker must exist inside the folder (`dir.join(marker).exists()`, :35). Exact Windows build output folder is not in code (Tauri resolves it at runtime).
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\potato\resource_dirs.rs:29

### 13-potato-bot-sandbox-finish.md / NEEDS-FACT: scripts convention in root package.json (line 568, 1049)
Answer: YES, root package.json has a `scripts` block (line 19), with entries such as `"build"`, `"test"`, `"check"`, `"check:privacy"`, `"test:changed"`. Adding a script is consistent with that convention.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\package.json:19

### 13-potato-bot-sandbox-finish.md / NEEDS-FACT: whether `check_key` needs `&self` (line 614, 1044)
Answer: YES, `&self`. Trait `pub trait Hands: Send + Sync` (helpers.rs:91) declares `fn check_key(&self, key: &str) -> std::result::Result<(), String>;` (helpers.rs:101). Impl: `impl Hands for LiveHands` (live.rs:209); `fn check_key(&self, key: &str) -> Result<(), String>` (live.rs:255) returns Ok when `sidecar::fake_brain_active()`, else calls `ask_model(key)` (live.rs:266). The `&self` on `helpers.rs:388` is `pub fn check_key(&self) -> Result<String>` on a different type. Error text does not include the key (live.rs:264-265 comment, :275-280 messages).
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\potato\live.rs:255

### 13-potato-bot-sandbox-finish.md / NEEDS-FACT: `makeProvider` options (line 625, 1045)
Answer: `export function makeProvider(): Provider` takes NO options (provider.ts:89). It reads env: `FISHD_PROVIDER` (`stub`, `opencode`, `deepseek`), `DEEPSEEK_API_KEY`, `OPENCODE_API_KEY`, `FISHD_MODEL`, `FISHD_REASONING`, `FISHD_REASONING_EFFORT` (provider.ts:68-98). Only variable names are given here; no values read.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\potato-brain\src\core\provider.ts:89

### 13-potato-bot-sandbox-finish.md / NEEDS-FACT: provider method name and request shape (line 626, 1045)
Answer: Method is `chat`, not `complete`: `export interface Provider { ... chat(req: ChatRequest): Promise<ChatResult>; }` (types.ts:102-104). `ChatRequest` at types.ts:82, `ChatResult` at types.ts:93. Example: `StubProvider.chat(req)` returns `{ text, usage, model }` (provider.ts:34-53).
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\potato-brain\src\types.ts:102

### 13-potato-bot-sandbox-finish.md / NEEDS-FACT: `trait Sandbox` methods for guest command and file copy (line 769, 1046)
Answer: `pub trait Sandbox: Send + Sync` (sandbox/mod.rs:140) has: `status`, `install(&self, options, progress)`, `start`, `stop`, `snapshot(&self, snapshot: &str)`, `reset(&self, snapshot)`, `remove`, `observe`, `act(&self, actions: &[Action])`, `browser(&self, args: &[String]) -> SbxResult<String>`, `screen_target`, `person_holds`, plus desktop methods (`installed_desktops`, `active_desktop`, `add_desktop`, `switch_desktop`) and settings methods (`cancel_install`, `resume_options`, `linux_feature`, `set_idle_minutes`, `idle_tick`). No generic "exec" or file copy method on the trait. Guest command runner is on the WSL type: `pub fn exec(&self, name: &SandboxName, user: GuestUser, argv: &[&str], stdin: Option<Vec<u8>>, timeout: Duration) -> Result<Output, WslError>` (sandbox/wsl.rs:578-585), argv only, optional stdin bytes. File copy out (`pull`) not found.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\potato\sandbox\mod.rs:140

### 13-potato-bot-sandbox-finish.md / NEEDS-FACT: file push method (line 857)
Answer: No `push_file` on `trait Sandbox`. No `/mnt/` copy path in wsl.rs (grep found none). Existing way to send content into the guest is stdin on `exec` (wsl.rs:578-585; the install archive is sent as a stream, install.rs:148 `tar_stream`). So the plan's "add push_file" is still needed, or use stdin.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\potato\sandbox\wsl.rs:578

### 13-potato-bot-sandbox-finish.md / NEEDS-FACT: Review Desk entry point (line 871, 885, 1048)
Answer: Tauri commands in packages/app/src-tauri/src/commands/review_desk.rs: `review_desk_list` (:63), `review_desk_count` (:70), `review_desk_detail` (:77), `review_desk_keep` (:88), `review_desk_throw_away` (:105), `review_desk_unmark` (:122), `review_desk_fix` (:134). No command that opens a branch by path. UI open: `openReview(first)` sets view `'review'` (CockpitApp.tsx:116-118); `ReviewDesk` imported at CockpitApp.tsx:25. The agent-side MCP tool `review_worktree` (merge.rs:3, 33) is not a Tauri command. Line 885 "open Review Desk on <free name>" has no matching call.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\commands\review_desk.rs:63

### 13-potato-bot-sandbox-finish.md / NEEDS-FACT: copy bundle from guest (pull method) (line 881)
Answer: NOT FOUND. No pull/copy-out method on trait Sandbox or in wsl.rs (searched potato/ for copy/push/pull/put/read_file/write_file names; the only matches were unrelated: artifacts `put`, `push` for log/notify, install `tar_stream`).
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\potato\sandbox\mod.rs:140

### 13-potato-bot-sandbox-finish.md / NEEDS-FACT: Review Desk open call (line 885)
Answer: same as item for line 871: no Tauri command opens a branch for review by path; the UI route is `openReview()` (CockpitApp.tsx:116-118).
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src\CockpitApp.tsx:116

### 13-potato-bot-sandbox-finish.md / NEEDS-FACT: helper card representation in `PotatoMessage` (line 956)
Answer: `export interface PotatoMessage { id; at; from: { kind: FromKind; name? }; kind: MessageKind; text; mood?; requestId?; project?; paneId?; costNano? }` (lib/potato.ts:91-105). `MessageKind = 'text' | 'helper_card' | 'helper_result' | 'note'` (lib/potato.ts:89). Helper card: `kind: 'helper_card'`, `requestId`, `project` (folder name) (lib/potato.ts:98-99). Helper result row: `kind: 'helper_result'`, `paneId` (lib/potato.ts:101-102). Rust mirror: `pub enum MsgKind { Text, HelperCard, HelperResult, Note }`, snake_case (store.rs:328-335).
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src\lib\potato.ts:91

### 13-potato-bot-sandbox-finish.md / NEEDS-FACT: Review Desk transport `openReview` / `potato_review_open` name (line 967)
Answer: NOT FOUND. No `potato_review_open` command or `openReview` transport method exists (searched packages/app/src and src-tauri/src for `potato_review`, `review_open`). Review Desk commands are `review_desk_*` (see item for line 871).
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\commands\review_desk.rs:62

### 13-potato-bot-sandbox-finish.md / NEEDS-FACT: card type union closed, `'brought_back'` (line 1007)
Answer: `brought_back` is NOT in the `MessageKind` union (lib/potato.ts:89: `'text' | 'helper_card' | 'helper_result' | 'note'`) nor in Rust `MsgKind` (store.rs:330-335: Text, HelperCard, HelperResult, Note). Adding it needs both sides.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src\lib\potato.ts:89

### 13-potato-bot-sandbox-finish.md / NEEDS-FACT: how a helper's finish is detected and how helper cards are represented (line 1047)
Answer: `fn tick` (helpers.rs:398-399) looks at every running helper; `RunState::Done { status, summary, pane_id }` calls `finish_helper` (helpers.rs:415, 440). Run state comes from the operation status in live.rs:248-251: Completed -> `HelperStatus::Finished`, Failed -> `Failed`, Cancelled -> `Stopped`, else Running. The finished row is `Message::new(now, FromKind::Note, None, MsgKind::HelperResult, finished_line(status, folder))` (helpers.rs:456; `finished_line` at :212-215 gives "Helper finished in <folder>."). Card lookup by `request_id` with `MsgKind::HelperCard` (helpers.rs:269-272). Front-end card shape: see item for line 956.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\potato\helpers.rs:440

### 13-potato-bot-sandbox-finish.md / NEEDS-FACT: whether a docs test exists (line 1043)
Answer: same as item for line 112: NOT FOUND.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src\hooks\phone-handoff.test.tsx:20

### 13-potato-bot-sandbox-finish.md / NEEDS-FACT (1036): PHONE_NEEDS_DESKTOP_YES status/body
Answer: same as item for line 124.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\potato\sandbox\screen.rs:574

### 13-potato-bot-sandbox-finish.md / NEEDS-FACT (1037): paired phone identity type
Answer: same as item for line 125.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\mobile_pairing\mod.rs:89

### 13-potato-bot-sandbox-finish.md / NEEDS-FACT (1038): runtime struct that owns the control lease
Answer: same as item for line 243 (`ScreenLease`, lease.rs:192, owned by `ScreenHub`, screen.rs:362).
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\potato\sandbox\lease.rs:192

### 13-potato-bot-sandbox-finish.md / NEEDS-FACT (1039): cockpit root overlay slot
Answer: same as item for line 327 (NOT FOUND as named).
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src\CockpitApp.tsx:120

### 13-potato-bot-sandbox-finish.md / NEEDS-FACT (1040): bundle.resources keys and resources path
Answer: same as item for line 398.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\tauri.conf.json:26

### 13-potato-bot-sandbox-finish.md / NEEDS-FACT (1041): guest files list and brain script name
Answer: same as items for lines 408, 419, 427 (REQUIRED_FILES install.rs:108-115; brain script `brain.mjs`, sidecar.rs:131).
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\potato\sandbox\install.rs:108

### 13-potato-bot-sandbox-finish.md / NEEDS-FACT (1042): BrainStateCard copy and PotatoState field style
Answer: same as items for lines 405 (PotatoState service.rs:160-165) and 446 (BrainStateCard.tsx:19).
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src\components\cockpit\potato\BrainStateCard.tsx:19

### 13-potato-bot-sandbox-finish.md / NEEDS-FACT (1043): docs test
Answer: same as item for line 112: NOT FOUND.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src\hooks\phone-handoff.test.tsx:20

### 13-potato-bot-sandbox-finish.md / NEEDS-FACT (1044): whether check_key needs &self
Answer: same as item for line 614: YES, `&self`.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\potato\live.rs:255

### 13-potato-bot-sandbox-finish.md / NEEDS-FACT (1045): makeProvider options and provider call
Answer: same as items for lines 625 and 626 (no options; method `chat(req: ChatRequest): Promise<ChatResult>`).
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\potato-brain\src\core\provider.ts:89

### 13-potato-bot-sandbox-finish.md / NEEDS-FACT (1046): trait Sandbox methods for guest command and copying files
Answer: same as item for line 769 (no generic exec or file copy on the trait; `exec` on the WSL type at wsl.rs:578; no push/pull).
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\potato\sandbox\mod.rs:140

### 13-potato-bot-sandbox-finish.md / NEEDS-FACT (1047): helper finish detection and card representation
Answer: same as item for the 1047 question above (helpers.rs:398-456; lib/potato.ts:91-105).
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\potato\helpers.rs:440

### 13-potato-bot-sandbox-finish.md / NEEDS-FACT (1048): Review Desk entry point
Answer: same as item for line 871.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src\CockpitApp.tsx:116

### 13-potato-bot-sandbox-finish.md / NEEDS-FACT (1049): scripts convention in root package.json
Answer: same as item for line 568 (YES, `scripts` at package.json:19).
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\package.json:19

---

## Plan lines that look wrong or contradict the code
- 12 line 1234 and 1344: flag `potato_groups_enabled` does not exist anywhere. The Labs key is `potato_bot_enabled` (live.rs:31).
- 12 line 824 and 1341: `TurnOptions` has no "extra system text" field; use `context` (loop.ts:285).
- 13 line 419 and 1041: brain script is `brain.mjs` (sidecar.rs:131), not `src/pm.ts`.
- 13 line 626 and 1045: the provider method is `chat`, not `complete` (types.ts:104).
- 13 lines 124 and 1036: `PHONE_NEEDS_DESKTOP_YES` is raised in screen.rs, not potato_screen.rs.
- 13 line 243 and 1038: `AllowList` / `Asks` do not exist; the lease struct is `ScreenLease` in lease.rs.
# Facts part 3: NEEDS-FACT answers for plans 14 and 15

Read-only pass. Each item is numbered by its line in the plan file. Secret values are never written here; only secret names.

## Plan 14: 14-potato-bot-phone-and-telegram.md

### 14-potato-bot-phone-and-telegram.md / NEEDS-FACT L43 (Tag Your Agent token secret name)
Answer: The Tag Your Agent bot token is stored under the key `channels/<profile>/telegram_bot_token`, where `<profile>` is the profile name or `default` when there is none. Example names: `channels/default/telegram_bot_token`. Function: `key_name_for(profile: Option<&str>) -> String` (`format!("channels/{}/telegram_bot_token", profile.unwrap_or("default"))`). This is NOT a discord name and it is not in `SIDECAR_SECRETS`.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\channels\token.rs:28-30

### 14-potato-bot-phone-and-telegram.md / NEEDS-FACT L45 (Who may talk: People, owner ids, listed ids)
Answer: NOT FOUND for a type named `People`. The closest real type is `GroupWho` (variants `OnlyMe` default and `Anyone`), set per group on `TagGroup.who`. Searched `channels/` for `People`/`people` (no struct). Owner pairing is `cfg.paired` (see group.rs gate at line ~243). Partial answer: `TagGroup { chat_id, title, who: GroupWho, agent, folder, style, ... }` in `tag_config.rs`; default `who` is `OnlyMe` (test asserts `GroupWho::OnlyMe` at tag_config.rs:190).
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\channels\tag_config.rs:27-60, 190

### 14-potato-bot-phone-and-telegram.md / NEEDS-FACT L46 (Rate limits: RateBook 6/5 min person, 20/10 min place)
Answer: NOT FOUND. No `RateBook` exists in `packages\app\src-tauri\src` (searched whole tree). The real limiter is `Bucket` (`channels/rate.rs:12`, `Bucket::new(cap: u32, window_ms: u64)` at rate.rs:21, `take(now_ms) -> bool` at rate.rs:28) plus `Limits` (rate.rs:55). Tag Your Agent uses these in `channels/engine/group.rs`:
- per person: `take_person(key, cap = cfg.tag_bot.per_person_cap, now)` on an HOUR_MS bucket (group.rs:86-96, HOUR_MS = 3_600_000 at group.rs:41)
- per day: `take_daily(cfg.tag_bot.daily_cap, now)` on a DAY_MS bucket (group.rs:98)
- at most 2 questions at once: `MAX_INFLIGHT = 2` (group.rs:44); at most 1000 person buckets: `MAX_PEOPLE` (group.rs:46)
- pending adds: `Bucket::new(10, HOUR_MS)` (group.rs:80); busy note `Bucket::new(1, 60_000)` (group.rs:81)
Defaults for the caps are `DEFAULT_PER_PERSON_CAP` / `DEFAULT_DAILY_CAP` in tag_config.rs (the values were not printed); setup_tag_tests.rs:37 asserts the view shows `(daily_cap, per_person_cap) == (30, 5)`. No "20 per 10 min per place" limit found.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\channels\engine\group.rs:41-46, 80-98, 176-179
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\channels\rate.rs:12-28

### 14-potato-bot-phone-and-telegram.md / NEEDS-FACT L48 (Replies out, plain text, no mass mentions: allowed_mentions.parse=[])
Answer: NOT FOUND for `allowed_mentions`: the string does not appear anywhere in `channels\telegram`. What is there: plain text only, no `parse_mode`. The code comment says "No parse_mode: plain text only, so nothing in a message can format or spoof the chat." Tests assert `parse_mode` is absent (api/tests.rs:164 and :215).
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\channels\telegram\api.rs:318
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\channels\telegram\api\tests.rs:164, 215

### 14-potato-bot-phone-and-telegram.md / NEEDS-FACT L93 (bridge routes/mod.rs file name: declare)
Answer: The file exists: `packages\app\src-tauri\src\bridge\routes\mod.rs`. It declares the route modules as `mod <name>;` or `pub(super) mod <name>;` at lines 5-46 (for example `pub(super) mod potato_brain;` at line 27, `pub(super) mod potato_screen;` at line 28, and `#[cfg(test)] pub(super) mod potato_screen_tests;` at lines 29-30).
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\bridge\routes\mod.rs:5-46

### 14-potato-bot-phone-and-telegram.md / NEEDS-FACT L100 (Facts surface "phone", trust owner_remote; field names)
Answer: `Origin::Phone` does NOT exist. The `Origin` enum in potato/service.rs is `pub(super) enum Origin<'a> { Person, Routine { title: &'a str, context: Option<&'a str> } }`. Trust is `enum Trust { OwnerPc, OwnerRemote, Others }` with wire strings `"owner_pc"`, `"owner_remote"`, `"others"` (protocol.rs:206-219). The per-message facts sent to the brain are `SendFacts` (protocol.rs about line 300-320) with fields including `surface: &str` (line 304), `trust: Trust`, `conversation_id`, `from_id`, `from_name`, `text`, `context`, `tools`. Rule input is `rules::Facts<'a>` with `surface: &'a str`, `sender`, `channel`, `mentioned`, `replied_to_bot`, `from_bot`, `age_ms`, `text_is_empty` (rules.rs:61-71).
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\potato\service.rs:170-174
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\potato\protocol.rs:204-219, 300-320
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\potato\rules.rs:60-71

### 14-potato-bot-phone-and-telegram.md / NEEDS-FACT L114 (PhoneSession type name after phone auth)
Answer: NOT FOUND as `PhoneSession` (searched `bridge` and `potato` for the name). The type the screen routes get is `Caller` (enum in mobile_pairing/auth.rs:152): `Local`, `Public`, `Device { device_id: String, caps: Caps }`, `Brain { bot_id: String }`. Caps are `Cap` (mobile_pairing/caps.rs:16): `Watch`, `Answer`, `Type`, `Control`. Used in potato_screen.rs:33 (`use crate::mobile_pairing::{self, Cap, Caller}`), gate at potato_screen.rs:51-65.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\mobile_pairing\auth.rs:150-161
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\mobile_pairing\caps.rs:14-25
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\bridge\routes\potato_screen.rs:33, 51-65

### 14-potato-bot-phone-and-telegram.md / NEEDS-FACT L164 (phone app entry and router folder, packages/app/src/phone/?)
Answer: There is no `packages/app/src/phone/` folder. The phone web app entry is `packages\app\src\PwaApp.tsx`. It imports `./styles/phone.css` (line 40), mobile components from `./components/mobile/` (for example `PhonePlans` line 24, `PhoneSearch` line 43, `PhoneChatView` line 53, `AcpPhoneDock` line 54), and tab logic from `./lib/mobile-agent-tabs` (line 21). Styles: `packages\app\src\styles\phone.css` and `phone-chat.css`. Phone-only lib: `packages\app\src\lib\phone-abilities.ts`.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src\PwaApp.tsx:21-54
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src\styles\phone.css

### 14-potato-bot-phone-and-telegram.md / NEEDS-FACT L169 (PhonePotato.tsx path)
Answer: No phone Potato component exists in the phone tree (no match for `Potato` under `components/mobile/`). Potato desktop UI lives in `packages\app\src\components\cockpit\potato\` (files include `BotPanel.tsx`, `AdvancedTab.tsx`, `ThreadIcons.tsx`, `PersonaSwitch.tsx`). Recommended path for a new phone component, by the existing pattern: `packages\app\src\components\mobile\`.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src\components\cockpit\potato\BotPanel.tsx
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src\PwaApp.tsx:21

### 14-potato-bot-phone-and-telegram.md / NEEDS-FACT L171 (phone router file to add a Potato tab)
Answer: NOT FOUND for a Potato tab. `PwaApp.tsx` builds tabs via `phoneLaunchEntries`, `tabLabels`, `tabOfPane`, `ALL_TAB` from `lib/mobile-agent-tabs.ts`; a grep for `Potato|potato` in that file found nothing. Sheets are switched with `sheet === 'plans'` (PwaApp.tsx:839). The router to edit is `PwaApp.tsx`.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src\PwaApp.tsx:21, 839
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src\lib\mobile-agent-tabs.ts

### 14-potato-bot-phone-and-telegram.md / NEEDS-FACT L175 (the phone fetch helper)
Answer: The phone app has no generic fetch helper with a plain name. It uses bridge transports built from the `bridge` object: `bridgeCompanionTransport(bridge)` (PwaApp.tsx:169) and `bridgePlansApi(bridge)` (PwaApp.tsx:171), passed as props (for example `<PhonePlans api={plansTransport} ...>`). Named helper for the plans API was not located in `packages\app\src` by `export function bridgePlansApi` search.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src\PwaApp.tsx:169-171, 839

### 14-potato-bot-phone-and-telegram.md / NEEDS-FACT L183 (PhoneApi wrapper name)
Answer: NOT FOUND. No `PhoneApi` identifier in `packages\app\src` (searched `PhoneApi`, `phoneFetch`, `PhoneTransport`, `PotatoTransport`). Real names: `PotatoTransport` is in `packages\app\src\lib\potato` (used by CockpitApp.tsx:46 as `tauriPotatoTransport`); the phone has no potato transport yet.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src\CockpitApp.tsx:46, 518

### 14-potato-bot-phone-and-telegram.md / NEEDS-FACT L362 (group.rs: where a group message turns into an agent run)
Answer: `channels/engine/group.rs` turns a message into a job: `GroupJob { chat_id, reply_to, group_title, asker, question, ask: GroupAsk { agent, folder, prompt } }` (group.rs:50-60, built around line 188). The run starts in `GroupState`/`Engine::answer_job(&self, host: &dyn Host, job: GroupJob) -> Vec<Effect>` (group.rs:236), which calls `host.ask_group(&ask)`. Trait method: `fn ask_group(&self, ask: &GroupAsk) -> GroupAskOutcome` (channels/host.rs:78). Live impl in `channels/host_live.rs:164-176`: resolves the agent with `crate::side_question::resolve_agent`, then `askbar::run_ask(&request, &label, Some(group_folder(ask.folder.as_deref())), ...)` with `crate::agent_runs::quick::quick_ask` as the runner.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\channels\engine\group.rs:48-60, 188-199, 236-241
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\channels\host_live.rs:164-176

### 14-potato-bot-phone-and-telegram.md / NEEDS-FACT L377 (how the run picks a place; Sandbox runner)
Answer: No place selection in `group.rs`. The job carries only `folder: folder_for(group)` (group.rs:196) and `answer_job` re-reads it (group.rs:240: `ask.folder = host.config().tag_bot.group(job.chat_id).and_then(folder_for)`). "Sandbox" does not appear in the group path. The run goes to `askbar::run_ask` with the folder (host_live.rs:173). So today the only place is a local folder; a `SandboxOnly` branch was not found. `folder_for` is in `tag_config.rs:119`. tag_config.rs:161 says a project folder must not be shown to anyone who can trigger the bot until there is a sandbox.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\channels\engine\group.rs:196, 240
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\channels\tag_config.rs:119, 161

### 14-potato-bot-phone-and-telegram.md / NEEDS-FACT L446 (can poller.rs run twice with different tokens and sinks)
Answer: Yes, by signature. `spawn(api: Arc<dyn Api>, engine: Arc<Engine>, host: Arc<dyn Host>, generation: Arc<AtomicU64>, link: SharedLink) -> Running` (poller.rs:97-105) and `spawn_with(..., timing: Timing)` (poller.rs:108-115). The token is not a parameter: the `Api` value carries it. Each call has its own `Ctx` and `link`, and the poller stops when `generation` moves past its value (doc at poller.rs:95-96). Two pollers therefore need two different `generation` Arcs, or the second start stops the first. Tests in `poller/tests.rs` were not read.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\channels\telegram\poller.rs:95-135

### 14-potato-bot-phone-and-telegram.md / NEEDS-FACT L723 (phone request auth and session type name)
Answer: Same as L114 for the type name (`Caller`, `Device { device_id, caps }`). Auth: `gate()` in potato_screen.rs:51 returns `Caller::Local` when `mobile_pairing::requires_pairing_token(headers, peer_loopback)` is false (auth.rs:137); otherwise it needs a link for a paired device that has `Cap::Watch` (potato_screen.rs:56-64). Bearer tokens are checked in `bridge/http.rs` (Authorization: Bearer, lines 144 and 274).
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\bridge\routes\potato_screen.rs:51-65
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\bridge\http.rs:144, 274

### 14-potato-bot-phone-and-telegram.md / NEEDS-FACT L724 (file that declares bridge route modules)
Answer: `packages\app\src-tauri\src\bridge\routes\mod.rs`, lines 5-46. Route folder files include `potato_brain.rs`, `potato_screen.rs`, `potato_screen_tests.rs`, `channels.rs`, `chats.rs`, `night.rs`, `panes.rs`, and others (list from Glob of `bridge/routes/*`).
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\bridge\routes\mod.rs:5-46

### 14-potato-bot-phone-and-telegram.md / NEEDS-FACT L725 (Origin::Phone and Facts field names)
Answer: Same as L100. `Origin::Phone` does not exist; `Origin` has only `Person` and `Routine`. Trust wire names: `owner_pc`, `owner_remote`, `others` (protocol.rs:206-219). `SendFacts` carries `surface` and `trust`.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\potato\service.rs:170-174
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\potato\protocol.rs:204-219

### 14-potato-bot-phone-and-telegram.md / NEEDS-FACT L726 (phone web app folder, router, authenticated fetch helper)
Answer: Same as L164 and L175. Folder: `packages\app\src` (no `phone/` folder); router: `PwaApp.tsx`; fetch: bridge transports (`bridgeCompanionTransport`, `bridgePlansApi`), PwaApp.tsx:169-171.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src\PwaApp.tsx:169-171

### 14-potato-bot-phone-and-telegram.md / NEEDS-FACT L727 (Tag Your Agent token secret name, Open facts)
Answer: Same as L43. `channels/<profile>/telegram_bot_token` (default profile: `channels/default/telegram_bot_token`). Source of the key name: `key_name_for`.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\channels\token.rs:28-30

### 14-potato-bot-phone-and-telegram.md / NEEDS-FACT L728 (where the Tag Your Agent group path starts an agent run)
Answer: Same as L362 and L377. Starts in `Engine::answer_job` (group.rs:236), which calls `host.ask_group` (host_live.rs:164), which runs `askbar::run_ask` in the folder from `folder_for(group)`. Runs locally; no Sandbox branch found.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\channels\host_live.rs:164-176

### 14-potato-bot-phone-and-telegram.md / NEEDS-FACT L729 (poller.rs run twice with different tokens and sinks)
Answer: Same as L446. Yes by signature; the token is inside the `Api` argument; `generation` must be distinct per poller.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\channels\telegram\poller.rs:97-115

### 14-potato-bot-phone-and-telegram.md / NEEDS-FACT L730 (Update/Message types in wire.rs and bot username in fixtures)
Answer: `wire.rs` has `pub fn map_updates(result: &Value) -> Option<Vec<Update>>` (line 11), `map_update` (line 15) building `Update { update_id: i64, kind: UpdateKind }`, and `map_message(m) -> Option<InMessage>` (line 26) with fields `message_id`, `chat_id`, `chat_private`, `from_id`, `from_is_bot`, `username`, `first_name`, `date_s`, `text`, `reply_to`, `chat_group`, `chat_title`, `mentions`, `reply_to_from_id`, `reply_to_from_is_bot`, `reply_to_text`. Bot username in fixtures: `test_pm_bot` (api/tests.rs:93 and fixtures/callback.json:8). The `Update`/`Message` structs themselves are in `telegram/mod` (not opened).
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\channels\telegram\api\wire.rs:11-50
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\channels\telegram\api\tests.rs:93

### 14-potato-bot-phone-and-telegram.md / NEEDS-FACT L731 (is_ui_secret allow list)
Answer: `is_ui_secret(name)` returns true for `is_provider_secret(name) || is_sidecar_secret(name)`. `SIDECAR_SECRETS = ["deepseek_api_key", "discord_bot_token"]`. Telegram token key `channels/.../telegram_bot_token` is NOT in this list (it is kept by the channels token module). Names only, no values.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\secrets.rs:31-40

## Plan 15: 15-potato-bot-personas-and-polish.md

### 15-potato-bot-personas-and-polish.md / NEEDS-FACT L19 (current values of facts 1a, 1c, not in the facts file)
Answer: NOT FOUND. I did not locate the facts file that holds facts 1a and 1c, so no values were read.
Source: NOT FOUND (not searched in this pass)

### 15-potato-bot-personas-and-polish.md / NEEDS-FACT L70 (existing release checklist, search docs/ for release and checklist)
Answer: NOT FOUND. Glob `docs/**/*{release,checklist,lint}*` in the repo returned no files.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\docs

### 15-potato-bot-personas-and-polish.md / NEEDS-FACT L163 (docs linter allow list)
Answer: NOT FOUND for a docs linter. The Glob for `docs/**/*{release,checklist,lint}*` found nothing. Not searched further in this pass (no grep of package.json scripts).
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\docs

### 15-potato-bot-personas-and-polish.md / NEEDS-FACT L217 (pack description field)
Answer: NOT FOUND. `packages\potato-brain\src\persona\packs.ts` exports only `packsRoot(personaDir): string` (line 4), `listPacks(personaDir): string[]` (line 9), `resolvePersonaDir(personaDir, pack?): string` (line 26). No description field in those exports.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\potato-brain\src\persona\packs.ts:4-26

### 15-potato-bot-personas-and-polish.md / NEEDS-FACT L255 (where the art lives in the app)
Answer: NOT FOUND. Not searched in this pass.
Source: NOT FOUND

### 15-potato-bot-personas-and-polish.md / NEEDS-FACT L268 (direction of Sandbox screen connection, host to guest or guest to host)
Answer: The bridge relays the screen socket to the guest: `GET .../websockify` is "relayed byte for byte to the guest's websockify on `127.0.0.1:<view port>`", so the host opens the TCP connection to the guest (host to guest). The header also says the control connection is checked against the lease every second. `screen.rs` itself did not show a `connect`/`listen` call in the grep.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\bridge\routes\potato_screen.rs:8-10, 13-15

### 15-potato-bot-personas-and-polish.md / NEEDS-FACT L308 (guest opens connections to host for the screen)
Answer: Same as L268. The bridge code opens the connection to the guest's websockify (host to guest). No evidence found that the guest opens connections to the host for the screen.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\bridge\routes\potato_screen.rs:8-10

### 15-potato-bot-personas-and-polish.md / NEEDS-FACT L328 (nftables present? the install recipe must add nftables)
Answer: NOT FOUND. grep for `nftables` under `packages\potato-sandbox` returned no matches. `apply-recipe.py` has a package-list check at line 174-176 (`block["apt"]` must be a list of package names) and an install stage at line 472-474, but the distro's package list was not found.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\potato-sandbox\guest\desktop\apply-recipe.py:174-176, 472-474

### 15-potato-bot-personas-and-polish.md / NEEDS-FACT L372 (does SandboxCall know the conversation/scope)
Answer: NOT FOUND. Not searched in this pass (potato/outside/engine.rs and sandbox_calls.rs were not read).
Source: NOT FOUND

### 15-potato-bot-personas-and-polish.md / NEEDS-FACT L447 (where current mood is stored, TS side and Rust side)
Answer: TS: `packages\potato-brain\src\persona\mood.ts` has `mood: Mood` (line 8, and 48), a `ISO timestamp of the last mood change` field (line 16), default `mood: "genki"` (line 77), and `advance`-by-time with re-derive (line 88). Rust: `potato/protocol.rs:85` `Event::Mood { mood: String, intensity: u64, energy: u64, hunger: u64 }`; `potato/service.rs:122` `pub struct MoodNow { mood, intensity, energy, hunger }`; `service.rs:141` `pub mood: Option<MoodNow>`; the value is set in memory at `service.rs:1494-1498` (`rt.mood = Some(MoodNow { ... })`). So the Rust side is held in memory, not saved.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\potato-brain\src\persona\mood.ts:8-16, 48, 77, 88
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\potato\service.rs:122-141, 1494-1498

### 15-potato-bot-personas-and-polish.md / NEEDS-FACT L524 (open work item fields: id, title, updated time)
Answer: `potato/open_work.rs:37-42`: `pub struct Item { pub id: String, pub text: String, pub done: bool }` with serde `camelCase`. There is no `title` and no `updated_ms` field. `id` is `ow-` plus eight lowercase hex digits (line 38).
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\potato\open_work.rs:35-42

### 15-potato-bot-personas-and-polish.md / NEEDS-FACT L580 (open work has no updated_ms: which names)
Answer: Same as L524. `updated_ms` does not exist. Item fields are `id`, `text`, `done`. No created-time field was found on `Item`.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\potato\open_work.rs:37-42

### 15-potato-bot-personas-and-polish.md / NEEDS-FACT L672 (ThreadIcons names for 4 icon buttons)
Answer: `export type ThreadIconName = 'check' | 'alert' | 'chevron' | 'arrow-down';` and `export function ThreadIcon({ name, size = 14 })`. Only these four names exist.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src\components\cockpit\potato\ThreadIcons.tsx:5-19

### 15-potato-bot-personas-and-polish.md / NEEDS-FACT L833 (url crate present)
Answer: NOT a direct dependency. `packages\app\src-tauri\Cargo.toml` has no `url = ...` line. Direct deps seen: `sha2 = "0.10"` (line 50), `ureq = { version = "2", features = ["json"] }` (line 54). The `url` crate may still be a transitive dependency; Cargo.lock was not checked.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\Cargo.toml:21-54

### 15-potato-bot-personas-and-polish.md / NEEDS-FACT L879 (HTTP client resolve override)
Answer: The client is `ureq` 2 (`ureq::AgentBuilder::new()` at `potato/live.rs:267`, in `ask_model`, lines 266-280). The grep for `resolve` in live.rs found no resolve override. ureq 2's builder is the place to check for a resolver; not confirmed in code here.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\potato\live.rs:266-280

### 15-potato-bot-personas-and-polish.md / NEEDS-FACT L1060 (existing release checklist location)
Answer: Same as L70. NOT FOUND.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\docs

### 15-potato-bot-personas-and-polish.md / NEEDS-FACT L1061 (Fish pack files and recorded sources)
Answer: NOT FOUND for the file list. `packs.ts` has `listPacks(personaDir)` (line 9), which reads the pack folder; the folder contents and `lifted/PORTED.md` were not read.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\potato-brain\src\persona\packs.ts:9-25

### 15-potato-bot-personas-and-polish.md / NEEDS-FACT L1062 (docs linter restricts new folders under docs/)
Answer: Same as L163. NOT FOUND.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\docs

### 15-potato-bot-personas-and-polish.md / NEEDS-FACT L1063 (values potato_set_persona accepts; PersonaSwitch.tsx; pack descriptions)
Answer: `#[tauri::command] pub async fn potato_set_persona(bot_id: String, pack: String) -> Result<Bot, String>` runs `s.set_persona(&bot_id, &pack)` (potato.rs:56-58). The function does not validate `pack` itself; the accepted list is in `set_persona` (not read). `PersonaSwitch.tsx` and pack descriptions: NOT read in this pass.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\commands\potato.rs:56-58

### 15-potato-bot-personas-and-polish.md / NEEDS-FACT L1064 (where mascot/avatar art lives in the app)
Answer: NOT FOUND. Not searched in this pass.
Source: NOT FOUND

### 15-potato-bot-personas-and-polish.md / NEEDS-FACT L1065 (Sandbox screen/control connection direction)
Answer: Same as L268. Host to guest: the bridge relays to the guest's websockify on `127.0.0.1:<view port>` (potato_screen.rs:8-10).
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\bridge\routes\potato_screen.rs:8-10

### 15-potato-bot-personas-and-polish.md / NEEDS-FACT L1066 (distro package list and nftables installed)
Answer: Same as L328. NOT FOUND for `nftables` in `apply-recipe.py`. The apt package list comes from each recipe block's `apt` list (apply-recipe.py:174-176).
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\potato-sandbox\guest\desktop\apply-recipe.py:174-176

### 15-potato-bot-personas-and-polish.md / NEEDS-FACT L1067 (how root commands run and stdin is piped in the guest)
Answer: `potato/sandbox/wsl.rs`: `GuestUser::Root` (as_str `"root"`, line 66-68); a request struct with `args: Vec<String>` run as argv with no shell, optional `stdin: Option<Vec<u8>>` (lines 218-240), `stdin(bytes)` builder (line 239), timeout and cancel. The header says "sends scripts over stdin, never through a shell string built from data" (line 13). Max stdin size 'the biggest script or archive sent over stdin' (line 33 constant).
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\potato\sandbox\wsl.rs:13, 33, 66-68, 218-240

### 15-potato-bot-personas-and-polish.md / NEEDS-FACT L1068 (computer_act action shape and whether dispatcher knows the turn's scope)
Answer: Partial. `potato-brain/src/core/sandbox-tools.ts`: the header says `computer_act` has `drag` instead of `down` and `up` (line 10); the six tools are `computer_observe`, `computer_act`, `browser_navigate`, ... (line 2). The browser action item has `kind` enum `click`, `fill`, `type`, `upload` and `ref` (lines 126-127); this is `browser_act`, not `computer_act`. The exact computer_act action fields (keys, typed text) were NOT read. `sandbox_calls.rs` scope knowledge: NOT FOUND (not read).
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\potato-brain\src\core\sandbox-tools.ts:2, 10, 124-130

### 15-potato-bot-personas-and-polish.md / NEEDS-FACT L1069 (where the current mood is stored on the Rust side)
Answer: Same as L447. In memory on the runtime struct: `rt.mood = Some(MoodNow { mood, intensity, energy, hunger })` (service.rs:1494-1498); field `mood: Option<MoodNow>` (service.rs:141, 272).
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\potato\service.rs:141, 272, 1494-1498

### 15-potato-bot-personas-and-polish.md / NEEDS-FACT L1070 (open work item fields: updated_ms, done)
Answer: Same as L524. `Item { id, text, done }` (open_work.rs:37-42). `updated_ms` does not exist.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\potato\open_work.rs:37-42

### 15-potato-bot-personas-and-polish.md / NEEDS-FACT L1071 (ThreadIcons.tsx icon names)
Answer: Same as L672: `'check' | 'alert' | 'chevron' | 'arrow-down'`.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src\components\cockpit\potato\ThreadIcons.tsx:5

### 15-potato-bot-personas-and-polish.md / NEEDS-FACT L1072 (crate depends on url and sha2; HTTP client resolve override)
Answer: `sha2 = "0.10"` yes (Cargo.toml:50). `url`: no direct dependency line. `ureq = { version = "2", features = ["json"] }` (Cargo.toml:54); `ask_model` uses `ureq::AgentBuilder::new()` (potato/live.rs:267). No resolve override seen.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\Cargo.toml:50, 54
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\potato\live.rs:266-280

### 15-potato-bot-personas-and-polish.md / NEEDS-FACT L1073 (how tool specs are sent each turn and how the prefix cache keys them)
Answer: Partial. Tool specs come from `sandboxToolSpecs(): ToolSpec[]` (sandbox-tools.ts:159, returns six specs). `packages\potato-brain\src\core\cache.ts` is cost accounting (`DEEPSEEK_RATES` cacheHit 0.028, cacheMiss 0.28; `CacheStat`), not prefix keying. Sending each turn and keying in `core/loop.ts` NOT read.
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\potato-brain\src\core\sandbox-tools.ts:159
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\potato-brain\src\core\cache.ts:1-17

### 15-potato-bot-personas-and-polish.md / NEEDS-FACT L1074 (routines.json schema version)
Answer: `pub const FILE_VERSION: u32 = 1;` (routines.rs:24). Files with `v > FILE_VERSION` are refused with "routines.json was written by a newer Puppet Master." (routines.rs:328). Shape: `RoutinesFile { v, routines }` (routines.rs:327, 334).
Source: C:\Users\Ren-pc\Desktop\work2\puppet-master\packages\app\src-tauri\src\potato\routines.rs:24, 327-334

## Plan lines that look wrong or contradictory to code
- Plan 14, L100 and L725: `Origin::Phone` does not exist; `Origin` has only `Person` and `Routine`.
- Plan 14, L46: `RateBook` does not exist; the code uses `Bucket` (rate.rs) with per-person hourly and daily caps (group.rs).
- Plan 14, L48: `allowed_mentions` does not appear; only "no parse_mode" is in the code.
- Plan 14, L43/L727: the Tag Your Agent token is `channels/<profile>/telegram_bot_token`, not a discord secret.
- Plan 15, L524/L580/L1070: open work `Item` has no `updated_ms` field (fields: id, text, done).
- Plan 15, L833/L1072: `url` is not a direct dependency in Cargo.toml.
