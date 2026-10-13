# Account profiles: one agent, several logins

Branch `feat/account-profiles`. Status: built and tested; auto-switch on a rate limit is designed, not built.

## 1. What and why

**Personas.** A (developers with a work and a personal subscription, or a team seat and their own), B (vibecoders
who hit a plan limit and have a second account), C (not targeted; the UI hides the feature until an agent supports it).

**Story.** "I run Claude Code under my work account for this repo and under my personal account for that one,
and I pick which when I press +." The user adds an account once (a name), clicks "Open login pane", logs in
inside that pane, and from then on picks the account per pane or per `run_agent` call.

**Agent-agnostic.** Generic code never names an agent. A profile declares *how* its CLI is pointed at another
account (`TuiProfile::account_scheme`, or `[accounts]` in a TOML definition). An agent with no declared scheme
gets no accounts UI. This generalises the OpenCode key A/B rotation, which is untouched.

**Policy note (plain).** Using several accounts may be against some vendors' terms. Puppet Master only points the
CLI at a config folder the user controls. It does not read, copy, pool or share credentials, and it does not
rotate accounts on its own. The user is responsible for what their provider allows. The Settings text says this.

## 2. Design

### Env channel (the prerequisite)
Facts verified: TOML `[env] set/unset` was parsed, validated and fingerprinted but never applied (`LaunchSpec` had
no env). Fixed:

- `LaunchSpec { env: Vec<(String,String)>, env_remove: Vec<String> }` (`tui/types.rs`), plus `LaunchSpec::new` and
  `apply_to_command`. `LaunchEnv { set, remove }` is the extra env of a launch (the account's).
- Applied in every launch path, spec env first, account env second (account wins):
  - `pty/registry/command.rs::build_launch_spec_command` (spec profiles, incl. TOML agents)
  - `build_command`, the legacy `AgentType` builders, on the direct, `cmd.exe /C` and Cursor paths and on the
    Windows PowerShell host (`windows_powershell_host_agent`): the host is a child of the `CommandBuilder`, so the
    agent it starts inherits the variables (proven with a real PowerShell, see Evidence)
  - `agent_runs/headless/process.rs::headless_command` (headless runs)
  - `standby/host.rs::standby_command` (headless standby); the visible standby spawns through `spawn_pane`
- `DeclarativeProfile` fills `env`/`env_remove` from `[env]` in both `headless_spec` and `tui_spec`.

### Account scheme (profile capability)
`TuiProfile::account_scheme() -> Option<AccountScheme { home_env, extra_env, login_hint }>`, default `None`.
TOML: `[accounts] home_env = "X_HOME"`, optional `extra_env = { K = "v" }`, `login_hint = "..."`. Validated
(env-name shape, denied env names such as `LD_PRELOAD`, 1 KiB values, no placeholders, hint <= 300 chars) and
included in the definition fingerprint **only when present**, so existing fingerprints/approvals do not change.

| Agent | `home_env` | Source | Declared? |
|---|---|---|---|
| Claude Code | `CLAUDE_CONFIG_DIR` | code.claude.com settings / env-vars docs: "CLAUDE_CONFIG_DIR" is the config root (transcripts under `projects/`) | yes |
| Codex | `CODEX_HOME` | Codex config docs: "Codex stores its local state under `CODEX_HOME` (defaults to `~/.codex`)", holds `config.toml` and file-based `auth.json` | yes |
| Cursor CLI | `CURSOR_CONFIG_DIR` | documented as the config directory for `cli-config.json`; docs do not say the login token lives there | **no** (unverified for credentials) |
| Kilo | `KILO_CONFIG_DIR` | vendor blog says it moves config **but not** `auth.json`; real isolation needs `XDG_CONFIG_HOME` + `XDG_DATA_HOME` (two vars, only one source) | **no**. Kilo is a TOML agent; its owner can declare `[accounts]` with `extra_env` once verified |
| OpenCode | none | auth is in the XDG data dir; OpenCode already has the key A/B rotation | **no** |

Caveat on Claude Code: on macOS the login may live in the keychain keyed by config dir; not tested here (Windows).

### Accounts store (`accounts/mod.rs`)
`<app data>/accounts.json` = `{ version, accounts: [{ id, agent_id, label, home }] }`, written atomically (tmp +
rename) under a process lock. `home` defaults to `<app data>/accounts/<id>`; a custom home must be absolute.
`add` requires the agent's profile to declare a scheme; labels are unique per agent (case-insensitive), <= 40
chars. `remove(id, delete_home)` never deletes a folder unless `delete_home` is set, and even then only a folder
inside the app's own `accounts/` dir (a user-chosen home is never deleted). `Store::at(root)` is the test seam.

Surfaces:
- Tauri: `accounts_list`, `accounts_add`, `accounts_rename`, `accounts_remove` (`commands/accounts.rs`)
- Bridge: `GET /accounts`, `POST /accounts`, `/accounts/rename`, `/accounts/remove` (`bridge/routes/accounts.rs`).
  Deliberately **not** in the MCP catalog: agents only pass `account`.

### Selection
- `SpawnPaneArgs.account` -> resolved in `spawn_pane` before spawning (unknown/foreign account fails the spawn);
  `PaneInfo.account` (id) and `PaneInfo.account_label` (cockpit header).
- `new_pane(agentId, accountId?)` (Tauri) -> `tray::new_pane_for_account`; the warm-standby claim uses the account key.
- `run_agent(..., account)`: `AgentRunRequest.account` -> validated (`accounts::validate_for_run`) -> stored on
  `WorkerPersist.account` (follow-up turns inherit it) -> used by the visible spawn (`bridge/dispatch.rs`) and the
  headless launch (`headless/process.rs`).
- Never cross accounts:
  - `worker_key(handle, .., name, account)`: a *named* worker is keyed `name@<account>`, so the same name under
    another account is a different worker. An explicit handle is never folded; adopting a worker with a different
    `account` is `ACCOUNT_MISMATCH` (409). A live pane adopted by `worker_id`/`pane_id` hands its account to the run.
  - `PoolKey.account` + `with_account`: warm panes and warm headless processes are keyed by account; `key_for_run`
    takes the account. A run reuses an existing idle pane only if `pane.account == run account`.
- MCP catalog: one optional `account` string on `run_agent` (`66-46d53ccddbd2b4dc` -> `66-335bc4af8cd69435`),
  isolated in its own commit (merge hot-spot with `feat/worktree-isolation`: re-pin the hash after merging both).

### UI (monochrome)
- `components/settings/accounts/AccountsGroup.tsx` (mounted by one line in `AgentsTab`): per supporting agent, the
  login hint, accounts (inline rename, "Open login pane", "Remove" which keeps the folder), "Add account".
  Hidden when the backend reports no supporting agent (browser, old backend).
- `components/cockpit/NewAgentMenu.tsx` (used by `Sidebar.tsx`'s "+"): an agent with at least one account shows
  "N logins >", then a second step: Default login / each account / Back. (The brief said "more than one account";
  with one account there are already two logins to choose between, so the step shows from one.)
- `PaneView` header: the account label next to the project name.

### OSS
No new crate. Nothing copied. Env names come from the vendors' docs, linked in the table.

## 3. Evidence

Commands run in `D:\pm-account-profiles` (shared target dir `D:/pm-cargo-account`, `CARGO_BUILD_JOBS=4`).

```
cd packages/app/src-tauri && cargo test --lib
test result: FAILED. 1029 passed; 1 failed; 2 ignored   (the one failure is the known pre-existing
                                                           bridge::tests::definitions::approve_requires_app_session)
npm test --workspace=@puppet-master/app
Test Files 49 passed (49) / Tests 383 passed (383)
npx tsc --noEmit -p packages/app           -> no output (clean)
```

New Rust tests (all pass): 
- Env channel per path
  - `pty::registry::tests::launch_spec_env_reaches_the_pty_child` (real PTY child prints the variable)
  - `launch_spec_env_remove_and_account_env_override` (unset variable not seen by the child; account env beats spec env)
  - `legacy_agent_type_builders_apply_env` (Claude/Codex/OpenCode/Cursor builders; asserts the PowerShell host on Windows)
  - `powershell_host_inherits_the_launch_env` (real `powershell.exe` on a PTY prints the value)
  - `launch_command_passes_the_account_env_for_code_profiles`
  - `agent_runs::headless::process::env_tests::*` (real child via `headless_command`: spec env, account env, removal)
  - `standby::host::env_tests::headless_standby_process_gets_spec_and_account_env`
- TOML `[env]` now applied: `tui::declarative::tests::profile::toml_env_is_carried_by_the_headless_and_pane_launch_specs`
- Account scheme / validation: `accounts_table_becomes_the_account_scheme`, `accounts_table_is_validated_and_fingerprinted`
- Account -> env: `accounts::tests::account_maps_to_the_profiles_home_env_var` (Claude `CLAUDE_CONFIG_DIR`, Codex `CODEX_HOME`,
  foreign account refused), `only_agents_whose_profile_declares_a_scheme_get_accounts`
- Store CRUD: `crud_round_trips_through_accounts_json`, `labels_are_unique_per_agent_and_validated`,
  `removing_never_deletes_the_home_unless_asked_and_only_app_owned_ones`
- Pool/worker separation: `standby::key::tests` (extended: account changes the key, blank account = none),
  `agent_runs::persist::account_key_tests::*`
- Catalog: `tool_registry::tests::catalog_version_is_pinned` (updated) and `run_agent_advertises_an_optional_account`
- HTTP: `bridge::routes::accounts::tests::accounts_routes_transcript` (in-process request -> response; run with
  `cargo test --lib accounts_routes_transcript -- --nocapture`):
  ```
  GET /accounts -> 200 {"accounts":[],"agents":[{"agent_id":"claude","home_env":"CLAUDE_CONFIG_DIR",...},{"agent_id":"codex",...}]}
  POST /accounts {claude, Work} -> 200 {"agent_id":"claude","home":"...\\accounts\\acc_xxxxxxxx","id":"acc_xxxxxxxx","label":"Work"}
  POST /accounts {claude, work} -> 400 ... already has an account named
  POST /accounts {opencode, x}  -> 400 ... opencode does not support accounts
  POST /accounts/rename -> 200 ...  POST /accounts/remove -> 200 {"home_deleted":false,"removed":true}
  ```

**Failing before, passing after.** With the application points disabled (the spec env line in
`build_launch_spec_command` and `env: self.def_env()` in the declarative profile, i.e. the pre-change behaviour), run
`cargo test --lib -- launch_spec_env_reaches toml_env_is_carried`:
```
test tui::declarative::tests::profile::toml_env_is_carried_by_the_headless_and_pane_launch_specs ... FAILED
  left: []   right: [("PM_T", "from-toml")]
test pty::registry::tests::launch_spec_env_reaches_the_pty_child ... FAILED
  left: None right: Some("spec-env-7731")
```
They pass on the branch. Also `catalog_version_is_pinned` failed with `66-335bc4af8cd69435` vs the old pin until updated.

UI component tests: `AccountsGroup.test.tsx` (6) and `NewAgentMenu.test.tsx` (4), in the 383 total.

Screenshots (headless harness, scenarios `settings-accounts`, `cockpit-accounts-menu`, `cockpit-pane-account`):
`D:\pm-night\evidence\account-profiles\*.png`. The "+" menu looks see-through in the headless shot because the glass
blur is not rendered there; it is the existing menu style.

## 4. Limits and follow-ups

- Not run against the real CLIs (no logins on this machine). Env var names come from docs; the PowerShell/PTY tests
  prove the variable reaches the child, not that each CLI honours it. macOS keychain behaviour of Claude Code untested.
- Cursor, Kilo, OpenCode have no scheme (see table); a TOML author can add `[accounts]` for Kilo-like agents.
- Accounts are not shown for `opencode_native` (it has its own key rotation).
- The visible standby claim key includes the account, but a pane spawned by hand for an account is not pre-warmed
  until a first use of that account (`note_run`).
- MCP agents cannot list accounts (not in the catalog); they need an id from the user.
- The headless `--version` probe in `finish.rs` and the model probe do not get the account env.
- No path to remove an account's folder from the UI (the `delete_home` flag exists in the command/route only).

## 5. Designed, not built: auto-switch on a rate limit

Out of scope tonight. Design:

- `TuiProfile::detect_limit(&self, screen_or_output: &str) -> Option<LimitHit { kind, reset_hint }>`, default `None`,
  lifted from the OpenCode-only phrase list in `opencode/quota.rs` (that file becomes the OpenCode profile's
  implementation; behaviour unchanged). TOML: `[tui.limits] patterns = ["regex", ...]` (validated like other
  regexes, size-limited) for declarative agents.
- Detection feeds the existing pane status/prompt pipeline (hook via `bridge/sse.rs::push_pane_status_sse`), raising
  an Inbox card "Claude Code hit its limit on Work. Continue on Personal?" with the other accounts of that agent.
- A switch is **a new session plus a context-pack handoff, not a resume**: the credentials differ, so the provider
  session cannot be resumed. Build a context pack (`context_pack.rs`) from the old pane, start a pane under the
  chosen account (`new_pane(agentId, accountId)`), deliver the pack as the first prompt.
- Always user-confirmed; never automatic in the first version, given the policy note above.
