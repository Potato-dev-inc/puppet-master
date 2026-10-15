# Toolbelt: configure MCP servers once

Branch `feat/toolbelt`. Evidence screenshots: `D:\pm-night\evidence\toolbelt\`.

## 1. What and why

**Story.** A developer wants a browser, a database and a docs MCP server in every agent. Today that means editing
Claude Code, Codex, OpenCode and Cursor configs by hand, each in its own format and place. With the toolbelt they list
each server once in Settings > MCP & bridge > Toolbelt, press **Apply to all agents**, review what will change in each
agent, and Apply.

- Personas: A (developer) mainly. B and C get the same list and plain copy ("Add an MCP server once...").
- Agent-agnostic: generic code never matches an agent name. Each agent declares where it keeps MCP servers and how an
  entry looks. Code agents carry the declaration in their profile (`TuiProfile::mcp_host`); declarative agents carry
  it in their TOML (`[mcp_host]`), so a user-made agent definition becomes a toolbelt target with no code.

## 2. Design

### Declaration (generalises the existing `[mcp_host]`)

`McpHostTable` (`tui/declarative/schema.rs`) already had `config_path`, `format`, `key_path`, `entry`. Two optional fields
were added (both covered by the approval fingerprint and validated):

| Field | Meaning |
|---|---|
| `project_config_path` | The agent's per-project config, relative to the project (for "This project only" servers). |
| `entry_http` | Entry template for web-address servers, placeholders `{url}` and `{headers}` (whole value). No `entry_http` means "this agent can't take URL servers". |

New trait method `TuiProfile::mcp_host() -> Option<McpHostTable>` (default `None`; the declarative profile returns its
table). Built-in declarations: Claude Code (`~/.claude.json`, `mcpServers`), Codex (`~/.codex/config.toml`,
`mcp_servers`), Cursor Agent (`~/.cursor/mcp.json`), OpenCode and OpenCode (API) (`~/.config/opencode/opencode.json`,
`mcp`, array `command`). Shells have none and are not listed. An agent without a declaration (for example Kilo, today)
is shown as "Not supported" with a reason.

### Files

| Path | Role |
|---|---|
| `src-tauri/src/toolbelt/store.rs` | `toolbelt.json` in app data: `{ id, name, transport, command?, args?, env?, url?, headers?, enabled, scope }`, validation, secret masking |
| `toolbelt/edit.rs` | Format-preserving edit of ONE table: JSON by text splicing (key order and indentation survive), TOML and YAML by line ranges, in place |
| `toolbelt/sync.rs` | The engine: plan, diff, `.bak`, atomic write; one report row per config file (agents sharing a file are one row) |
| `toolbelt/import.rs` | Runs the agent's own entry template backwards to read its servers |
| `toolbelt/service.rs` | JSON front shared by commands and routes |
| `commands/toolbelt.rs` | Tauri commands `toolbelt_list/add/update/remove/sync_preview/sync_apply/import_preview/import_apply` |
| `bridge/routes/toolbelt.rs` | `GET/POST /toolbelt`, `/toolbelt/update`, `/remove`, `/sync/preview`, `/sync/apply`, `/import/preview`, `/import/apply`. App session only: MCP sessions and remote devices get 403 |
| `mcp_install/declarative.rs` | `HostLaunch` gained `url`/`headers`; `substitute`, `edit_*` made `pub(crate)` |
| `src/lib/toolbelt.ts`, `components/settings/toolbelt/*` | UI: list, add/edit form, switches, Apply sheet, Import sheet |
| `components/cockpit/Palette.tsx` | Palette entry "Toolbelt..." opens Settings on the MCP tab |
| `dev/scenarios/settings-toolbelt*.ts`, `cockpit-palette-toolbelt.ts`, `dev/fixtures.ts`, `dev/mock-tauri.ts` | harness scenarios and mocks |

### Rules the engine follows

- **Ownership.** PM only writes keys named `pm-toolbelt-<id>`. It reads the parent table, and adds, updates or removes
  those keys only. Everything else in the file is left byte for byte (tested: apply, then remove everything, gives back
  the original file exactly, for JSON and TOML).
- **Removal.** Delete a server (or switch it off) and the next apply removes it from every agent.
- **Backup.** The first time a file changes, `<file>.bak` is copied. Later writes keep that `.bak` (it is the file as it
  was before Puppet Master first touched it). A config that did not exist gets no `.bak`.
- **Idempotent.** A second sync reports `unchanged` and writes nothing.
- **Dry run.** `sync_preview` returns the same plan with `dry_run: true`; nothing is written. Each row has the changed
  lines only, with secret values masked.
- **Safe refusals.** Invalid JSON or TOML is left untouched and reported as an error row. Paths must stay under home or
  the project (the existing HST-3 rule). An agent that is not installed and has no config file is skipped. A web-address
  server is skipped (with a note) for agents with no `entry_http`.
- **Puppet Master's own entry.** Optional checkbox in the sheet ("Also register Puppet Master itself"). It writes under the
  key the agent declares (`...puppet-master`) and is never removed by the toolbelt.
- **Scope.** `global` servers go to `config_path`; `project` servers go to `project_config_path` and only when a project
  is selected.

### Secrets (plain statement)

This branch has no OS keyring helper, so secrets are not in the keyring. Values of env vars and headers whose names look
secret (key, token, secret, password, auth, bearer, cookie, credential) are stored **as typed in `toolbelt.json` in the
app data folder**, are never sent to the UI (lists show `********`, and saving with `********` keeps the stored value),
and are masked in diffs and in import previews (import reads the real values server side). Agent config files need the
real value, so sync writes it there in plain text. The UI says so next to the list. Follow-up: move values to the OS
keyring and keep only references in `toolbelt.json`.

### OSS used

No new dependencies. `serde_json`, `toml` (already in the crate). The YAML path reuses the in-repo flow-mapping writer.

## 3. Evidence

All run in `D:\pm-toolbelt` with `CARGO_BUILD_JOBS=2`.

**Rust.** `cargo test --lib` in `packages/app/src-tauri`:

```
test result: FAILED. 1027 passed; 1 failed; 2 ignored
test bridge::tests::definitions::approve_requires_app_session ... FAILED     (known, pre-existing)
```

New tests (`cargo test --lib -- toolbelt`: 19 passed). All use temp config files. Hosts: JSON-style (`mcpServers`,
Claude-shaped), TOML-style (`mcp_servers`, Codex-shaped), array-command JSON (OpenCode-shaped) and YAML:

- `toolbelt::tests::sync_adds_entries_in_json_and_toml_and_keeps_the_users_bytes` (adds, key order kept, apply-then-remove
  restores the original bytes exactly)
- `second_sync_is_idempotent` (statuses `unchanged`, bytes equal, no `.bak`)
- `update_changes_only_the_managed_entry_and_removal_follows_the_store` (update, then disable removes)
- `first_write_makes_a_bak_and_later_writes_keep_it`
- `dry_run_writes_nothing_and_shows_a_diff_without_secrets` (also: the real value does reach the file on apply)
- `array_command_and_yaml_hosts_work` (array `command`, `remote` type, YAML in place, URL server skipped with a note,
  remove restores the YAML byte for byte)
- `project_scope_goes_to_the_project_file_only`, `uninstalled_agent_without_a_config_is_skipped_and_pm_entry_is_optional`,
  `invalid_config_is_left_untouched`
- `import_reads_servers_back_through_the_hosts_templates` (two agents, same server merged, PM and toolbelt-owned entries
  not offered, secrets masked in preview, real values stored on apply, `already` after)
- `store_validates_masks_and_keeps_secrets_on_edit`, `service_actions_round_trip`, `edit_json_*`, `edit_toml_*`
- `builtin_agents_declare_hosts_and_shells_do_not` (also: the two OpenCode profiles share one file row)
- `mcp_install::tests::toolbelt_declaration::*` (new `[mcp_host]` fields validate; a declarative agent is a host)
- Bridge: `bridge::tests::toolbelt_routes::routes_map_to_actions_and_only_the_local_app_may_call_them` is the in-process
  request to response transcript. Eight routes map to their actions and return 200. MCP sessions and remote devices get
  `403 AUTHORIZATION_DENIED` before any action runs. A failed action returns `400` with a plain message. A second test
  shows the MCP session gate refuses `/toolbelt/...` too.

**Fails before, passes after.** `builtin_agents_declare_hosts_and_shells_do_not` with Claude's `mcp_host` override
removed:

```
test toolbelt::tests::builtin_agents_declare_hosts_and_shells_do_not ... FAILED
claude should declare an MCP host: ["codex", "opencode", "opencode_native", "cursor_agent"]
```

With the override restored it passes (above).

**Frontend.** `npm test --workspace=@puppet-master/app`: `Test Files 48 passed (48)`, `Tests 385 passed (385)`. New:
`components/settings/toolbelt/ToolbeltGroup.test.tsx` (12 tests: list, empty state, enable switch, add command server,
add web address server and refusal message, remove, preview sheet contents including skipped and unsupported agents and
the file-changes link, apply, nothing-to-change disables Apply, include-Puppet-Master re-preview, import sheet, helpers).

`npx tsc --noEmit -p packages/app`: exit 0.

**Screenshots** (`node scripts/evidence/screenshot.mjs --out D:/pm-night/evidence/toolbelt settings-toolbelt settings-toolbelt-preview cockpit-palette-toolbelt`):

- `settings-toolbelt.png`: Toolbelt group with four servers, switches, scope, secrets note
- `settings-toolbelt-preview.png`: Apply sheet with per-agent changes and an expanded, masked diff
- `cockpit-palette-toolbelt.png`: palette filtered to "Toolbelt..."

(The red "Cannot read properties of null (reading 'repairResults')" box in the first screenshot is the pre-existing
runtime-status block hitting an unmocked command in the harness; it is not part of this feature.)

## 4. Limits and follow-ups

- Secrets are in `toolbelt.json` in plain text (no keyring helper on this branch); see above.
- The `.bak` is taken once (before the first write), not before every write.
- YAML hosts: entries are written as one-line flow mappings (existing writer); YAML import reads flow mappings only. No
  built-in agent uses YAML today.
- Agents that already hold a server by hand with the same name keep it; the toolbelt writes its own `pm-toolbelt-` key,
  so an agent may then see the server twice. Import, then delete the hand-added one, avoids that.
- Codex `bearer_token_env_var`, OAuth-style remote auth and per-tool allowlists are not modelled.
- The project path comes from the Settings project; "This project only" servers need one.
- Built-in declarations assume current config locations of each agent (user-level files). If an agent changes its format,
  the fix is one TOML string in its profile.
- Not live-tested against the real agents' config files (temp files only; the app was not launched).

## 5. File overlap with other branches

Shared files touched, all append-only or one-line:

- `tui/profile.rs` (new default trait method), `tui/{claude,codex,cursor,opencode}/mod.rs` (a const and one method each)
- `tui/declarative/{schema,fingerprint,validate,profile}.rs` (two optional `[mcp_host]` fields, one trait override)
- `mcp_install/declarative.rs` (`HostLaunch` fields, visibility, `{url}`/`{headers}`), `mcp_install/tests.rs`
- `lib.rs` (`mod toolbelt;` plus eight command registrations), `commands/mod.rs`, `bridge/routes/mod.rs`, `bridge/tests.rs` (append)
- `components/settings/tabs/McpTab.tsx` (import plus one mounted element), `components/cockpit/Palette.tsx` (one item)
- `dev/fixtures.ts`, `dev/mock-tauri.ts` (appended cases)

Likely overlaps: any branch that edits `TuiProfile` (account-profiles, worktree-isolation) or the agent definition schema
(`schema.rs`, `fingerprint.rs`), `McpTab.tsx`, `Palette.tsx` (any branch adding a palette action), `lib.rs` and
`commands/mod.rs` registration lists. The MCP tool catalog is not touched.
