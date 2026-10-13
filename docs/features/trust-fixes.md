# Trust fixes: API keys where the UI says, and no silent kill on close

Branch `feat/trust-fixes`. Two bugs that make users doubt the product.

## 1. What and why

Personas: A developers, B vibecoders, C non-technical people.

- **Fix 1.** Settings > API keys said "secure OS storage" while provider keys sat in plain `settings.json`
  (UI-INVENTORY bug 2). Keys now live in the OS credential store. The UI shows `•••• abcd · Stored in
  Windows Credential Manager` with Replace and Remove. If no credential store exists the UI says so plainly and
  never claims security.
- **Fix 2.** Closing a pop-out pane window killed a working agent with no question. Now a working pane
  (`running` or `waiting_input`) holds the close and shows a sheet: "<Agent> is still working. Close and stop
  it?" with Stop and close / Keep open. Same for tray "Quit Puppet Master". New setting
  `confirm_close_running_pane` (default on); the cockpit's own Close reuses it.

Agent-agnostic: nothing here names an agent. The sheet label comes from the agent registry (`agentLabel`), the
decision depends only on the generic pane status.

## 2. Design

### Secrets (Rust)
- `src-tauri/src/secrets.rs`: `SecretBackend` trait; `KeyringBackend` (real store); `MemoryBackend` (tests, and the
  process-wide backend under `cfg(test)` so tests never touch a real credential store). `mask()` returns
  set + last 4 characters (only for keys of 12+ chars, otherwise no characters). `read_secret()` is the Rust reader.
  `migrate_settings_file()` and `migrate_on_startup()` (called in `lib.rs` setup before the settings load).
- `src-tauri/src/commands/secrets.rs`: `secret_set`, `secret_get_masked`, `secret_delete`, `secret_storage_info`,
  and `secret_get_for_llm`. All restricted to the three provider names.
- `opencode/keys.rs`: the A/B profile credentials are written to the credential store as `opencode_profile_<id>`
  and blanked in `opencode-key-profiles.json` (only after a read-back match); hydrated on load.
  `migrate_profiles_to_secret_store()` runs at startup. Caveat: the active key is also written to OpenCode's own
  `auth.json` (OpenCode needs it there); the UI copy says so.
- Migration (`settings.json`): for each provider key in the file, write to the store, read back, compare, then remove
  the field from the file (atomic tmp + rename). Idempotent. Store unavailable or read-back mismatch: the file is left
  untouched and the UI warns.

### Who reads the raw key (documented exception)
The sidebar's API orchestrator runs its LLM tool loop in the webview (`lib/llm.ts`, `useOrchestratorSession.ts`,
`useRemoteOrchestratorMessages.ts`), so it needs the key for request headers. Moving that loop into Rust is a
separate, large change. Instead `secret_get_for_llm(name)` is the single narrow command that returns a raw key; it is
restricted to the provider keys, and TS reads through `resolveApiKey()` in `lib/secrets.ts`
(store first, then the file value when there is no store). Nothing else in the UI can fetch a key. No Rust code read
provider keys before; Rust readers must use `secrets::read_secret`.

### UI
- `components/settings/ApiKeyField.tsx` (used by `ApiTab`): states are secure+set (masked, Replace, Remove),
  secure+unset (input, "Not set. It will be stored in <store>."), insecure (warning + keys edited in the plain file,
  labelled "Saved in a plain settings file (not secure)").
- `saveSettings` drops provider keys when the store is secure so a stale snapshot never rewrites them to the file.
- Advanced > Export now uses `redactSettings()` (any field named like api key / token / secret / password is `***`
  or omitted), not a hand-picked list.
- OpenCode A/B copy rewritten to be true.

### Close confirm
- `pane_windows.rs`: pure `close_decision(status, setting)` and `quit_decision(statuses, setting)`;
  `on_close_requested` now returns `bool` (prevent close) and emits `pane-window://confirm-close` to that window;
  `close_confirmed` (command `confirm_pane_window_close`) kills the pane and destroys the window.
- `components/ClosePaneConfirm.tsx` + `styles/close-confirm.css`, mounted once in `TerminalApp` for pop-outs.
- Tray: `pane_windows::request_quit` uses `tauri-plugin-dialog` (already a dependency): native dialog
  "N agents are still working. Quit and stop them?" with Stop and quit / Keep open.
- Setting `confirm_close_running_pane`: `shared` schema, `settings.ts` defaults, `toPublicSettings`, Rust
  `PATCHABLE_KEYS` and defaults, toggle in Settings > Sessions.
- Cockpit `PaneView` Close uses `needsCloseConfirm` (mirror of the Rust rule); it now also asks for `waiting_input`.

### OSS
- `keyring` 3.6.3 (MIT OR Apache-2.0), features `windows-native` (Windows Credential Manager), `apple-native`
  (macOS Keychain), `linux-native` (keyutils), each under a target-specific dependency. Version 4 has a different
  store-crate architecture, so 3.6 was chosen.
- The crate's own `mock` builder gives each `Entry` independent state, so it cannot test a write-then-read-back
  migration. The tests use the `MemoryBackend` behind the same trait instead.

## 3. Evidence

Commands run in `D:\pm-trust-fixes` (cargo with `CARGO_BUILD_JOBS=3`):

```
cd packages/app/src-tauri && cargo test --lib
  test result: FAILED. 1020 passed; 1 failed; 2 ignored   (the one failure is the known
  bridge::tests::definitions::approve_requires_app_session)
npm test --workspace=@puppet-master/app
  Test Files  49 passed (49)    Tests  382 passed (382)
npx tsc --noEmit -p packages/app      (clean)
```

New Rust tests (`cargo test --lib secrets`, `pane_windows`, `opencode::keys`):
- `secrets::tests::migration_moves_the_key_to_the_store_and_blanks_the_file`
- `secrets::tests::migration_is_idempotent` (second run: nothing migrated, file byte-identical)
- `secrets::tests::migration_keeps_the_key_in_the_file_when_the_store_is_unavailable`
- `secrets::tests::migration_keeps_the_key_when_the_readback_does_not_match`
- `secrets::tests::masked_get_never_returns_the_full_key`, `short_secrets_expose_no_characters`,
  `only_provider_secrets_are_ui_addressable`
- `opencode::keys::tests::profile_keys_are_kept_out_of_the_profile_file`
- `pane_windows::tests::busy_panes_ask_before_closing_and_idle_ones_do_not`, `quitting_asks_when_any_pane_is_busy`

New frontend tests: `components/settings/ApiKeyField.test.tsx` (masked field, Save/Replace/Remove, no-store copy,
export redaction) and `components/ClosePaneConfirm.test.tsx` (sheet shown on the Rust event, Keep open, Stop and
close, other pane ignored, `needsCloseConfirm`).

**Fails before, passes after** (implementation neutralised in place, then restored):
```
# close_decision made to always return Close; migration no longer removes the key from the file
test pane_windows::tests::busy_panes_ask_before_closing_and_idle_ones_do_not ... FAILED
test secrets::tests::migration_moves_the_key_to_the_store_and_blanks_the_file ... FAILED
test secrets::tests::migration_is_idempotent ... FAILED
test result: FAILED. 3 passed; 3 failed

# export redaction restricted to the old three hand-picked fields
x redactSettings (Advanced > export) > never copies a secret-looking field   (a github_token leaked)
```
Before this branch no test covered either behaviour, and the old code wrote the key to `settings.json` and killed the
pane unconditionally.

Screenshots (monochrome harness), in `D:\pm-night\evidence\trust-fixes\`:
`api-keys.png` (masked Anthropic key with Replace/Remove, unset fields with true copy),
`cockpit-close-running.png` (the cockpit confirm for a working pane).

## 4. Limits / follow-ups

- The pop-out sheet itself has no screenshot (the harness only has cockpit and settings routes); it is covered by
  component tests.
- The orchestrator LLM loop still receives the raw key in the webview via `secret_get_for_llm`. Real fix: move the
  loop into Rust.
- OpenCode's own `~/.local/share/opencode/auth.json` holds the active key in plain text by OpenCode's design.
- Existing keys that were in `opencode-key-profiles.json` migrate at next app start, not live.
- If a pane window's webview is wedged, the held X cannot show the sheet and the window stays open (the pane keeps
  running); the tray Quit still works. A forced-close fallback was not added.
- Not run against a real Windows Credential Manager in this session (no GUI launched): `KeyringBackend` is compiled
  and used at runtime only. A manual check is worth doing once: set a key, look for `com.puppetmaster.app` in
  Credential Manager, confirm `settings.json` has no key.
- The tray dialog buttons and Quit flow are untested at runtime (needs the app).
