# Declarative agent definitions: SPEC

Status: draft for review. Branch base: `refactor/tui-profiles`. Companion files in this folder:
`PLAN.md` (task list), `DEFINITION-FORMAT.md` (author reference), `AGENT-CHOICE.md` (the real agent
used to prove the feature).

## 1. Goal

Let Puppet Master drive any terminal coding agent from a single, reviewable TOML file, with no Rust,
TypeScript or MCP-schema change per agent. The proof is Kilo Code (`@kilocode/cli`): one file makes
`run_agent(agent_type="kilo", ...)` and `followup_task` work headless and in a visible pane on the
live bridge.

Success looks like this from the user's side: drop `kilo.toml` into `<app data>/agents/`, approve the
exact command line shown in Settings > Agents, and `kilo` appears in `GET /agents/types`, in the
spawn menu, and in the `agent_type` argument of every MCP tool, with no rebuild.

## 2. Background and current state (verified against the tree)

All paths are under `packages/app/src-tauri/src/` unless stated.

### 2.1 What is already data-shaped

- `tui/profile.rs`: `trait TuiProfile: Sync` has about 35 methods, each with a generic default. They
  fall into:
  - Identity and capability flags: `id`, `features` (`Features{kind, dispatchable, read_only_headless,
    read_only_spawned_pane, resumable_headless}` in `tui/types.rs`), `not_dispatchable_reason`,
    `run_options` (`RunOptionSupport`), `is_installed`, `standby_support`, `capability_notes`.
  - Headless: `headless_command` (returns `LaunchSpec{program,args,cwd,read_only_enforced}`),
    `accepts_headless_model`, `extract_result`, `headless_result_ready`, `extract_provider_session`,
    `rejected_call_warning`, `detect_headless_trust`, `extract_model`, `probes_default_model`,
    `default_model`, `run_warnings`, `headless_standby_command`, `headless_standby_input`.
  - Visible pane: `idle_after`, `busy_marker`, `quiet_idle_after`, `classify_screen`, `detect_prompt`,
    `prompt_reply`, `looks_like_menu`, `clear_input_sequence`, `verifies_prompt_delivery`,
    `prompt_visible`, `standby_screen`.
  - Screen result capture: `is_chrome_line`, `is_tool_activity_line`, `strip_dialogs`, `tidy_reply`,
    `detect_footer_model`.
- Roughly 60 percent of these are pure data (flags, argv shape, durations) or declarative parsing
  (JSON field selection, regex line filters, footer model regex). The smallest real profile is
  `tui/claude/` (`mod.rs`, `headless.rs`, `screen.rs`); the biggest is `tui/cursor/` (`chrome/`,
  `screen.rs`, `approval.rs`, `launch.rs`, `headless.rs`, fixtures).
- Cursor keeps heavy procedural logic (boxed-dialog stripping, allowlist approval parsing). Those
  profiles stay code (section 4).
- `tui/mod.rs` holds `static PROFILES: &[&dyn TuiProfile]` and `profile_for`, which falls back to
  `generic::Unknown` (`Kind::Unknown`, `dispatchable=false`). `opencode_native` is
  `Kind::NativeProtocol`, a REST/server backend (`crate::opencode`), not a screen-scraped CLI.
- `tui/conformance.rs` is a set of `#[test]`s that iterate `profiles()` (unique lowercase id,
  `AgentType::parse` round-trip, `every_agent_type_has_a_profile`, shared headless policy, parsers
  reject garbage, every prompt can be answered, an empty screen is never blocked, idle window
  bounds). A declarative profile must pass the same rules automatically.
- `standby/` (warm pool): backends are discovered through `tui::standby_backends()` and the profile
  methods `standby_support`, `standby_screen`, `headless_standby_command/_input`. Today only Claude and
  Cursor Agent opt in. Config lives in `standby/config.rs` (`standby-pool.json` in `app_paths::app_data_dir()`).

### 2.2 What is closed (the work this spec removes)

1. `pty/agents.rs`: closed `enum AgentType` (`Claude, Codex, Opencode, OpencodeNative, Cmd,
   Powershell, Bash, Cursor, CursorAgent`) with `as_str`, `parse`, and three per-OS launch matches
   (`resolve_windows/macos/linux`). Mirrored by hand in `packages/shared/src/agents.ts`
   (`AgentTypeSchema = z.enum([...])`, `AGENT_META`).
2. `AgentType::parse` is a gate that rejects unknown ids: `pty/registry/spawn.rs` (`unknown agent_type`),
   `bridge/dispatch.rs::ensure_agent_dispatchable` (`INVALID_AGENT_TYPE`), `bridge/routes/panes.rs`,
   `commands/agent_models.rs`, `agent_runs/persist.rs`, `agent_contexts/inspect.rs`.
3. About 100 `"opencode_native"` string comparisons across `agent_runs/`, `bridge/`, `opencode/`,
   `pty/registry/spawn.rs` (native branch before the generic path). These are protocol-backend
   branches and are NOT part of this work, but they show why the closed enum matters: a new id must
   never reach them by accident.
4. `agent_contexts/profiles.rs`: `get_agent_context_profile(AgentType)` is an exhaustive match whose
   struct (`AgentContextProfile` in `agent_contexts/mod.rs`) holds `agent_type: AgentType` and
   `&'static` strings/slices.
5. `mcp_install/{claude,codex,opencode}.rs`: per-host config writers (for hosts that consume the
   Puppet Master MCP server, not the agents we drive). Off the critical path; see HST.
6. Frontend: `packages/app/src/lib/ansi.ts` (`TUI_AGENTS` set), `TerminalViewport.tsx`,
   `lib/standby.ts` (per-backend labels), plus `AGENT_META` in `packages/shared/src/agents.ts`.
7. Hard-coded, mutually inconsistent `agent_type` enums in the MCP catalog:
   - `tool_registry/catalog/agents.rs` (`run_agent` and siblings): `claude, codex, cursor_agent,
     opencode, opencode_native`. `quick_ask` in `packages/mcp-server/src/catalog.ts` uses `backend`
     with `claude, codex, cursor_agent`.
   - `tool_registry/catalog/operations.rs` (`delegate_work`): `claude, codex, cursor, opencode,
     opencode_native` (note `cursor`, not `cursor_agent`).
   - `tool_registry/catalog/panes.rs` (`spawn_agent`): `opencode_native, powershell`; line 44 (another
     tool): `claude, codex, opencode, opencode_native, cmd, powershell, bash, cursor`.
   `tool_registry/catalog/mod.rs::catalog_version()` hashes names, descriptions, method, path and
   `input_schema.to_string()`; `tool_registry/tests.rs::catalog_version_is_pinned` pins it
   (`66-91b6c68e1db9fece` at time of writing). The MCP shim
   (`bin/puppet_master_mcp/bridge_http.rs`) compares its own `catalog_version()` with `/health` and
   reports `SCHEMA_DRIFT`. `packages/mcp-server/src/catalog.ts` is a duplicated TypeScript catalog.
8. Cross-contamination bug: the free functions in `tui/mod.rs` (`is_chrome_line`, `strip_dialogs`,
   `tidy_reply`, `is_tool_activity_line`, `looks_like_menu`) fold or `any()` over EVERY profile in
   `PROFILES`. Callers: `agent_runs/completion/clean.rs` (lines 28, 71, 90, 268, 287, 301-302) and
   `pane_wait/check.rs:314`. Today that means Cursor's chrome rules run against a Claude screen. With
   user-authored regexes this becomes a correctness and safety problem (one definition can eat
   another agent's reply lines). Fixed first, in Phase 0.

### 2.3 Related prior decisions (memory notes)

- Generic code never matches an agent-type string; add a trait method with a generic default instead
  (`tui/README.md` "The rule"; memory `tui-profiles`).
- Every MCP-affecting change needs a rebuilt dist exe (memory `rebuild-mcp-after-changes`).
- Every handoff gets a fresh `build_id`; finished work is committed (memory `always-commit-and-new-build-id`).
- `docs/REPO_MAP.md` must be read before exploring; it and `tui/README.md` are updated at the end.

## 3. In scope

- Runtime agent registry that replaces the static `PROFILES` slice and the closed `AgentType` gate.
- `DeclarativeProfile`: a `TuiProfile` implementation built from a strict TOML schema.
- Sources: bundled (compiled in), user dir, project-local dir, optional extra dirs, each with an enable
  switch and a trust policy.
- Headless mode (argv templates + three built-in output parser kinds) and TUI mode (regex tables).
- Approval of new or changed user/project definitions, showing the exact argv.
- MCP schema unification (`agent_type` as validated free string), `GET /agents/types`,
  `UNKNOWN_AGENT_TYPE`, one `catalog_version` bump with a documented drift path.
- Settings > Agents page (list, errors, reload, approve, sources, extra dirs).
- Generic zero-config mode and a session recorder that drafts a TOML.
- Hot reload, per-project consent, declarative MCP-host writer, definition sharing (export/import a file).
- Fixture + conformance + parity tests, and the end-to-end Kilo Code proof.

## 4. Out of scope (explicit)

- Porting Claude, Codex, Cursor Agent/IDE, OpenCode, `opencode_native`, cmd/powershell/bash to TOML.
  They stay code. A hybrid `base = "<code profile id>"` that overrides data fields is OPTIONAL and
  later (Phase 3, one task marked optional).
- A plugin API for native-protocol backends (HTTP/ACP/JSON-RPC drivers). `kind = "native_protocol"`
  is rejected by the schema. (Kilo ships `kilo serve` and `kilo acp`; we do not use them.)
- Remote registries, marketplace, auto-download of definitions, signed-definition infrastructure.
- Executing arbitrary scripts from a definition (no hooks, no embedded Lua/JS/Rhai, no shell snippets).
- Changing how existing agents are spawned or classified, other than Phase 0 scoping fixes.
- A GUI definition editor (Settings lists, validates, approves; editing happens in a text editor).
- Making `read_only` enforceable for an agent whose CLI has no documented read-only policy.

## 5. Requirements (EARS)

IDs are stable; tasks in PLAN.md cite them.

### 5.1 Registry (REG)

- REG-1: The system shall resolve every `agent_type` string through one runtime registry holding
  `Arc<dyn TuiProfile>` entries keyed by lowercase id.
- REG-2: When definitions are reloaded, the system shall swap the registry snapshot atomically; a run
  that already resolved its profile shall keep the `Arc` it holds for its whole lifetime.
- REG-3: The registry shall contain the code profiles (Claude, Codex, OpenCode, OpenCodeNative,
  CursorAgent, CursorIde, Cmd, Powershell, Bash) first and in the current stable order, then
  declarative profiles sorted by id. `read_only_backends()` ordering for existing ids shall not change.
- REG-4: If a lookup id is not registered, `profile_for` shall return the generic `Unknown` profile,
  as today; callers that must reject shall use `is_registered`.
- REG-5: The system shall represent an agent id with an `AgentId` newtype (lowercase, validated,
  interned `Arc<str>`) instead of `AgentType` outside the spawn/launch layer. `AgentType` remains for
  the code-profile set only.
- REG-6: `TuiProfile::id` shall return `&str` borrowed from `&self` (not `&'static str`).
- REG-7: Where a code profile and a definition declare the same id, the code profile shall win and the
  definition shall be reported as `AGENT_DEF_RESERVED_ID`.
- REG-8: Where two definitions declare the same id, precedence shall be project-local > extra dirs
  (in listed order) > user > bundled; the loser shall be reported as shadowed, never silently dropped.
  A project-local definition shall shadow others only for runs in that project.

### 5.2 Definition format (FMT)

- FMT-1: A definition shall be one TOML file with `schema_version = 1` and the tables in section 7.1.
- FMT-2: The loader shall use `deny_unknown_fields` on every table, so a typo is an error, not a
  silent default.
- FMT-3: Argv shall be arrays of strings only. No field of a definition shall be passed to a shell.
- FMT-4: Placeholders shall be from the closed set `{task} {project} {model} {session} {reasoning}`
  and substituted per argv element (the element stays one argument).
- FMT-5: Conditional argv segments shall use `when` from the closed set `model, resume, read_only,
  not_read_only, allow_shell, trust_workspace`.
- FMT-6: A definition shall not exceed 64 KiB, 200 regexes in total, 64 argv elements per command, or
  4 KiB in any single string.
- FMT-7: An unrecognised `schema_version` shall be rejected with `AGENT_DEF_SCHEMA_UNSUPPORTED`,
  naming the supported versions.

### 5.3 Sources and trust (SRC)

- SRC-1: Source `bundled` (directory `packages/app/src-tauri/agents/*.toml`, embedded at compile time)
  shall be on by default and trusted without approval.
- SRC-2: Source `user` (`<app data>/agents/*.toml`) shall be on by default; each file needs approval
  (5.7) before it can run.
- SRC-3: Source `project` (`<project>/.puppet-master/agents/*.toml`) shall be OFF by default. When
  enabled it needs (a) per-project consent keyed by `standby::key::canonical_project` and (b) approval.
- SRC-4: Source `extra` (a list of directories) shall be OFF by default; each directory needs its own
  consent and every file needs approval.
- SRC-5: Each source shall have an independent enable switch in settings. Disabling a source shall
  unregister its agents for new runs without killing live runs.
- SRC-6: The loader shall scan only `*.toml` directly inside a source directory (non-recursive), skip
  symlinks that leave the directory, and read at most 64 files per directory.
- SRC-7: A project definition shall never be loaded for a path that is not the project of the current
  run or pane.

### 5.4 Validation (VAL)

- VAL-1: `id` shall match `^[a-z][a-z0-9_]{1,31}$`, shall not be a reserved word (`unknown`, `all`,
  `any`, `auto`, `default`, `none`, `shell`, plus every code-profile id) and shall not collide per REG-7.
- VAL-2: Every regex shall compile with the Rust `regex` crate (linear time; no backreferences or
  look-around) under `RegexBuilder::size_limit(256 KiB)`.
- VAL-3: `binary.names` shall be bare file names (no path separators, no `..`, no drive letters).
- VAL-4: `binary.names` shall not name a shell or interpreter (`sh, bash, zsh, fish, dash, cmd,
  powershell, pwsh, python, python3, node, deno, bun, ruby, perl, php, osascript, wscript, cscript,
  mshta, env`) unless `[agent] allow_interpreter = true`, which forces an approval warning and is
  rejected for the project and extra sources.
- VAL-5: Environment keys shall not include `PATH, PATHEXT, LD_PRELOAD, LD_LIBRARY_PATH, DYLD_*,
  NODE_OPTIONS, PYTHONPATH, PYTHONHOME, RUBYOPT, PERL5OPT, COMSPEC, SHELL`.
- VAL-6: A definition declaring `[headless]` shall have exactly one task delivery: one `{task}`
  placeholder in argv (or `task_flag`), or a `[headless.stdin]` block; never more than one.
- VAL-7: A file that fails validation shall not be registered; it shall appear in the definitions list
  with its error code, file, line/column when TOML gives one, and a message.
- VAL-8: Validation shall run without network access or spawning any process. (The optional
  `version_args` probe runs only after approval, on demand.)

### 5.5 TUI mode (TUI)

- TUI-1: A definition with `[tui]` shall launch its pane as resolved program + argv in the project
  directory through the existing `pty::registry` spawn path, with no `AgentType` lookup.
- TUI-2: The profile shall implement `is_chrome_line`, `is_tool_activity_line`, `strip_dialogs`,
  `tidy_reply`, `looks_like_menu`, `busy_marker`, `classify_screen`, `detect_footer_model`,
  `idle_after`, `quiet_idle_after` and `clear_input_sequence` from the `[tui.*]` tables.
- TUI-3: When `[tui.ready]` is absent the profile shall use the generic prompt heuristic (as `Unknown`
  does) and `standby_screen` shall return `NotReady`.
- TUI-4: `idle_after_ms` shall be clamped to `[1500, 5000]` and `quiet_idle_after_ms` to
  `[500, idle_after_ms]`, matching `conformance.rs::idle_windows_are_sane_and_only_animated_tuis_shorten_them`.
- TUI-5: `verifies_prompt_delivery` shall default to false; a definition may opt in only with
  `[tui.delivery] input_echo_regex` and `transcript_echo_regex`.

### 5.6 Headless mode (HDL)

- HDL-1: `headless_command` shall build argv from `[headless]` templates, resolve the program with the
  binary resolver (7.3), set `cwd` to the project, and set `read_only_enforced` from
  `features.read_only_headless && launch.read_only`.
- HDL-2: Output parsing shall use one of `plain_text`, `json_single`, `json_lines`. No other parser kind
  exists in schema version 1.
- HDL-3: `extract_result` shall return `None` for non-zero exit codes and for output the selector does
  not match (the rule in `conformance.rs::output_parsers_reject_garbage_and_failed_exits`).
- HDL-4: `extract_provider_session` shall apply `[headless.output.session]` and return the first match.
- HDL-5: `headless_result_ready` shall return true once a line/object matching `[headless.output.done]` or
  `[headless.output.error]` has been seen, so a lingering child does not hold the turn.
- HDL-6: When `[headless.output.error]` matches, the run shall fail with `AGENT_REPORTED_ERROR` carrying
  the extracted message, even when the exit code is 0.
- HDL-7: If the task is positional and `task_separator` is set, the profile shall insert that separator
  immediately before the task so it cannot be read as a flag. A definition with a positional task and
  neither `task_separator` nor `task_flag` shall be rejected.
- HDL-8: Output read by a parser shall be capped at `max_output_bytes` (default 8 MiB, hard max 64 MiB).
- HDL-9: `resumable_headless` shall be true only when a `when = "resume"` segment exists and
  `[headless.output.session]` is configured.

### 5.7 Approvals (APR)

- APR-1: A user, project or extra definition shall not be spawnable until the user approves its
  fingerprint.
- APR-2: The fingerprint shall be SHA-256 over canonical JSON of the executable-relevant parts: `binary`
  (names, shim globs, extensions), every argv template and conditional segment, `env`,
  `allow_interpreter`, `source`, the canonical project path for project definitions, and the file's
  absolute path.
- APR-3: The approval dialog shall show, per mode, the rendered command line with sample placeholders
  (`<task>`, `<project>`), the resolved binary path or "not found", env keys, and warnings (for example
  `--auto`, `--dangerously-*`, `allow_interpreter`).
- APR-4: When a file's fingerprint changes after approval, the definition shall return to
  `needs_approval`; runs already started keep their snapshot; new runs fail with `AGENT_DEF_APPROVAL_STALE`.
- APR-5: Changes that do not alter the fingerprint (label, icon, regex tables, parser selectors) shall not
  require re-approval (they cannot cause execution; see SEC-6).
- APR-6: Approvals shall be stored in `<app data>/agent-approvals.json`; deleting an entry revokes it.
- APR-7: The MCP surface shall not be able to approve a definition; the HTTP route requires the app's own
  session, never an MCP session.

### 5.8 Model detection (MOD)

- MOD-1: `[model] source` shall be one of `none`, `footer_regex` (TUI), `output_path` (headless JSON),
  `probe_command` (argv run once, regex capture).
- MOD-2: `probe_command` shall run only after approval, off the async thread (the existing
  `probes_default_model` contract), with a 5 s timeout and 4 KiB output cap.
- MOD-3: `accepts_headless_model` shall be true exactly when a `when = "model"` segment exists.
- MOD-4: When `source = "none"` the run shall report `model_unconfirmed = true` in `RunFacts` and carry
  the standard unconfirmed-model warning.

### 5.9 MCP schema and API (API)

- API-1: The `agent_type` property of `run_agent`, `delegate_work`, `spawn_agent`, list filters and every
  other tool that takes one shall be `{"type":"string","pattern":"^[a-z][a-z0-9_]{1,31}$"}` with a
  description pointing to `GET /agents/types`. The diverging enums shall be removed.
- API-2: When a request names an unregistered id the bridge shall return `UNKNOWN_AGENT_TYPE`
  (HTTP 400) whose context lists `valid` ids (dispatchable first) and a `hint`. The legacy
  `INVALID_AGENT_TYPE` code shall remain in the `delegate_work` path for one release with the same body.
- API-3: `GET /agents/types` shall return every registered agent as in 7.5.
- API-4: `quick_ask` shall accept any registered id whose profile reports `read_only_headless` and
  reject others with the existing `READ_ONLY_UNSUPPORTED`.
- API-5: `catalog_version` shall change exactly once for this feature (Phase 0). Registering or removing
  an agent shall never change it, because the schema no longer contains the list.
- API-6: `list_agents`/`inspect_agent` output shall include `agent_label`.

### 5.10 Frontend and settings (UI)

- UI-1: `packages/shared/src/agents.ts` shall keep the nine built-in ids as known presets, but
  `AgentTypeSchema` shall accept any id matching the pattern and presets shall fall back to
  `{label: id, icon: first two letters upper-case, isTui: true}`.
- UI-2: The app shall read the registry via a Tauri command / `GET /agents/types` and use it for spawn
  menus, `TUI_AGENTS` (`lib/ansi.ts`), `TerminalViewport.tsx` and `lib/standby.ts`; compiled constants are
  the offline fallback only.
- UI-3: Settings > Agents shall list definitions with source badge, state (`active`, `needs_approval`,
  `approval_stale`, `invalid`, `shadowed`, `disabled`, `not_consented`, `not_installed`), errors, file path
  (click to reveal), a Reload button, source toggles, the extra-directory list, project consent, and the
  Approve dialog of APR-3.
- UI-4: The page shall never render a definition's `label` or `description` as HTML.
- UI-5: A new agent shall appear in the spawn menu within 1 s of a successful reload without a restart.

### 5.11 Standby pool (STB)

- STB-1: `standby::config::StandbyConfig` per-backend toggles shall key on id strings, and `clamped()`
  shall drop toggles for ids whose profile does not report support (unchanged behaviour for the code ids).
- STB-2: A declarative profile shall report `StandbySupport{visible:false, headless:false}` unless
  `[standby]` says otherwise AND `[tui.ready]` (visible) or `[headless.stdin]` (headless) is present.
- STB-3: Removing or disabling a definition shall discard its standby workers on the next pool tick.
- STB-4: Reloading a definition whose fingerprint changed shall discard its standby workers (a warm
  process holds the old argv).

### 5.12 Generic zero-config mode (GEN)

- GEN-1: A user may mark a bare command as an experimental agent (`[agent] kind = "generic"`, or the
  "Add as generic agent" action). The profile shall be visible-pane only; `dispatchable = true` only when
  `experimental = true`; every `run_agent` receipt shall carry `result_inferred = true` and the existing
  screen-capture warnings.
- GEN-2: Generic mode shall use only the shared heuristics (`generic::detect_prompt`,
  `pty::status::classify_pane_state`) and shall not claim `verifies_prompt_delivery` or standby.
- GEN-3: A generic agent shall be labelled "experimental" in every UI surface and in `GET /agents/types`.

### 5.13 Recorder (REC)

- REC-1: A "record session" action on a pane shall capture sanitized screen snapshots (idle, working,
  prompt, reply) and, for a headless probe, the raw output, into the scratch dir with secrets redacted.
- REC-2: The draft generator shall output a TOML with `[tui]` regex candidates (footer, busy marker,
  prompt boxes) and, when JSON output is detected, a `[headless]` skeleton, each candidate annotated with
  the sample line that produced it.
- REC-3: The recorder shall redact environment values, token-shaped strings (`sk-`, `ghp_`, `AKIA`, long
  base64/hex runs) and the user's home path before writing to disk.
- REC-4: The draft shall be written to `<app data>/agents/drafts/<id>.toml` and never be loaded from
  there (not scanned).
- REC-5: The recorder shall be off unless the user starts it and shall not run on standby-pool panes.

### 5.14 Hot reload (HOT)

- HOT-1: A filesystem watcher (debounced 300 ms) on enabled source directories shall trigger reload; a
  manual Reload shall always work.
- HOT-2: A failed reload shall keep the previous registry snapshot and report errors; a partial failure
  shall register the files that validated.
- HOT-3: Reload shall emit `agent_definitions_changed` (SSE + Tauri event) with `added`, `removed`,
  `changed`, `errors`.
- HOT-4: Reload shall follow the standby pool's live-apply model (`f54056a`): never touch running panes.

### 5.15 MCP host install (HST)

- HST-1: A definition with `[mcp_host]` shall get a generated writer for the Puppet Master MCP entry in
  that host's config: path, format (`json`, `toml`, `yaml`), key path, entry template.
- HST-2: The writer shall touch only the one key it owns and create a `.bak` first.
- HST-3: The writer shall require approval like the rest of the definition and refuse paths outside the
  user's home or the project directory.

### 5.16 Observability and errors (OBS)

- OBS-1: Every definition load shall emit one `event_log` record (source, id, outcome, fingerprint, code).
- OBS-2: Every run snapshot shall record `agent_definition: {id, source, fingerprint}` so a result is
  traceable to the exact argv that produced it.
- OBS-3: `GET /agents/types` and Settings shall show why an agent is not dispatchable
  (`not_dispatchable_reason`) or not installed (with `install_hint`).
- OBS-4: The error codes of 7.4 shall be the only codes the loader emits.

### 5.17 Security (SEC)

- SEC-1: No definition field shall reach a shell. Spawn is program + argv through the PTY/process APIs.
- SEC-2: A project or extra source definition shall never auto-run; consent plus approval is mandatory
  and consent is per canonical project path.
- SEC-3: Bare binary names shall be resolved by scanning `PATH` explicitly, excluding the current and the
  project directory, on every OS (Windows `CreateProcess` also searches the cwd; we do not rely on it).
- SEC-4: The resolved absolute path shall be pinned in the approval record; a different path at spawn
  time shall return `AGENT_DEF_BINARY_CHANGED` and require re-approval.
- SEC-5: The child environment shall be the parent's plus `env.set` minus `env.unset`, with VAL-5 keys
  rejected at load.
- SEC-6: Regex tables are untrusted input: the engine is linear-time; FMT-6/VAL-2 bound memory; screen
  text is capped at 256 KiB per evaluation.
- SEC-7: `allow_all` choices (`broad = true` prompts) shall be offered only when the caller sets
  `allow_broad` (the `generic::prompt_reply` rule), never auto-chosen.
- SEC-8: A definition shall not set `read_only_headless = true` without both a `when = "read_only"`
  segment and a `read_only_note` explaining the CLI's policy (shown at approval).
- SEC-9: Logs and events shall not include `{task}` values; argv is logged with placeholders.

## 6. Design summary

```
sources (bundled | user | project | extra)
   -> scan + parse (toml, deny_unknown_fields)
   -> validate (ids, regex, binary, env, limits)
   -> approval gate (fingerprint vs agent-approvals.json)
   -> DeclarativeProfile { def, compiled regexes, source, fingerprint }
   -> Registry snapshot: ArcSwap<Vec<Arc<dyn TuiProfile>>> (code profiles first)
tui::profile_for(id) -> Arc<dyn TuiProfile>
```

Key choices:

- `ArcSwap` snapshot (not a lock around a map): `profile_for` has about 50 call sites outside `tui/`,
  some on the PTY reader/idle threads; a lock-free read keeps them cheap and a reload cannot block them.
- Per-agent dispatchers (Phase 0): `tui::is_chrome_line(agent_type, lower)` and the others call only
  that profile's method. Required for the correctness of user regexes.
- Strict argv templates with a closed placeholder and condition set give the expressive power that
  `claude/headless.rs` and `codex/headless.rs` need, with nothing that can compose a shell string.
- Three parser kinds cover Claude (`json_single`), Codex (`json_lines`) and Kilo (`json_lines`).
- Parity tests prove the DeclarativeProfile machinery equals a code profile on the same inputs without
  porting any code profile: a test-only TOML that mirrors Claude's headless contract is run beside
  `tui::claude` over the existing Claude fixtures.

## 7. Data

### 7.1 Definition file: full schema (version 1)

All tables use `deny_unknown_fields`. "req" = required. Durations are integer milliseconds.

#### Top level

| Field | Type | Default | Constraints |
|---|---|---|---|
| `schema_version` | int | req | must be `1` |

#### `[agent]`

| Field | Type | Default | Constraints |
|---|---|---|---|
| `id` | string | req | VAL-1 |
| `label` | string | req | 1-40 chars, no control chars |
| `icon` | string | first 2 letters of id upper-cased | 1-4 chars |
| `description` | string | `""` | <= 240 chars |
| `kind` | `"cli"` or `"generic"` | `"cli"` | `native_protocol`, `shell` rejected |
| `experimental` | bool | `false` (`true` forced for `generic`) | |
| `allow_interpreter` | bool | `false` | VAL-4; invalid for project/extra sources |
| `base` | string | none | RESERVED. Optional later (Phase 3 hybrid): a code profile id whose methods back any table left empty. Rejected in v1 with `AGENT_DEF_FIELD_RESERVED`. |

#### `[binary]`

| Field | Type | Default | Constraints |
|---|---|---|---|
| `names` | array<string> | req, 1-6 | bare names without extension, tried in order |
| `windows_extensions` | array<string> | `["exe","cmd","bat"]` | subset of `exe, cmd, bat, ps1, com` |
| `shim_native_globs` | array<string> | `[]` | relative globs, max 4, see 7.3 |
| `allow_cmd_shim` | bool | `false` | 7.3 step 4 |
| `version_args` | array<string> | `["--version"]` | used by the on-demand probe |
| `install_hint` | string | `""` | <= 200 chars, shown when not installed |
| `login_hint` | string | `""` | <= 200 chars, how to sign in; shown by the agent doctor (display only, not part of the fingerprint) |

#### `[features]`

| Field | Type | Default | Constraints |
|---|---|---|---|
| `dispatchable` | bool | `true` | forced `false` for `generic` unless `experimental` |
| `not_dispatchable_reason` | string | `""` | required when `dispatchable=false` |
| `read_only_headless` | bool | `false` | SEC-8 |
| `read_only_spawned_pane` | bool | `false` | requires `[tui] read_only_argv` |
| `resumable_headless` | bool | derived | HDL-9; an explicit value must agree |
| `read_only_note` | string | `""` | required when either read-only flag is true |

#### `[run_options]`

| Field | Type | Default | Meaning |
|---|---|---|---|
| `trust_workspace` | bool | `false` | accept the flag; needs a `when="trust_workspace"` segment |
| `allow_shell` | bool | `false` | needs a `when="allow_shell"` segment, or `shell_default_on` |
| `shell_default_on` | bool | `false` | shell already runs; the flag is an accepted no-op |
| `allowed_commands` | bool | `false` | visible-pane auto-approve of named programs; needs a `[[tui.prompt]]` with `kind="command_approval"` |

#### `[env]`

| Field | Type | Default | Constraints |
|---|---|---|---|
| `set` | table<string,string> | `{}` | VAL-5; values <= 1 KiB; no placeholders |
| `unset` | array<string> | `[]` | removed from the child env |

#### `[headless]` (optional; absent = no headless mode)

| Field | Type | Default | Constraints |
|---|---|---|---|
| `argv` | array<string> | req | base argv after the program; FMT-4 placeholders |
| `task_flag` | string | none | for example `"--prompt"`; appends `[task_flag, "{task}"]` |
| `task_separator` | string | none | for example `"--"`; inserted before a positional task (HDL-7) |
| `cwd` | `"project"` / `"inherit"` | `"project"` | |
| `max_output_bytes` | int | 8388608 | 65536..67108864 |
| `timeout_hint_ms` | int | none | advisory, shown in capabilities |
| `[[headless.optional]]` | array of tables | `[]` | `when` (closed set), `args` array<string> |
| `[headless.stdin]` | table | absent | `template` string containing only `{task}`; stdin closed after the write; also what enables the warm pool |

A positional `{task}` is implied at the end of the argv when neither `task_flag` nor `[headless.stdin]`
is given; it is appended after `task_separator`.

#### `[headless.output]`

| Field | Type | Default | Constraints |
|---|---|---|---|
| `format` | `plain_text` / `json_single` / `json_lines` | req | |
| `strip_ansi` | bool | `true` | |
| `[headless.output.result]` | selector | req (`plain_text` may omit: whole text) | below |
| `[headless.output.session]` | selector | absent | |
| `[headless.output.model]` | selector | absent | for `[model] source="output_path"` |
| `[headless.output.done]` | selector | absent | HDL-5 |
| `[headless.output.error]` | selector | absent | HDL-6 |
| `[headless.output.rejected]` | selector + `warning_code` | absent | feeds `rejected_call_warning` |

Selector (the same table everywhere):

| Field | Type | Default | Meaning |
|---|---|---|---|
| `where` | table<string, string/int/bool> | `{}` | every dotted path must equal its value (`"part.reason" = "stop"`); a key starting with `~` means "path exists" (value `true`) |
| `where_regex` | table<string,string> | `{}` | the value at the dotted path matches the regex |
| `path` | string | none | dotted path to the value, `a.b.0.c`; absent = the matched object serialised as a string (only for `done`/`error`) |
| `pick` | `first` / `last` / `join` / `max_by` | `last` | which match |
| `join_with` | string | `"\n"` | with `join` |
| `max_by` | string | none | dotted numeric path; required with `pick="max_by"` |
| `trim` | bool | `true` | |
| `regex` | string | none | capture applied to the picked string (group `value`, else group 1) |

`json_single`: the selector runs on the single top-level object (or the last JSON object in the output
when earlier lines are not JSON). `json_lines`: it runs on each parsed line in order; unparseable lines
are ignored and counted for diagnostics. `plain_text`: `result.regex` and `result.pick` operate on lines.

#### `[tui]` (optional; absent = no visible mode)

| Field | Type | Default | Constraints |
|---|---|---|---|
| `argv` | array<string> | `[]` | base argv; placeholders `{project}`, `{model}`, `{session}` only |
| `[[tui.optional]]` | array of tables | `[]` | `when` in `model, read_only, resume`; `args` |
| `read_only_argv` | array<string> | none | appended when the run asks read-only; needed for `read_only_spawned_pane` |
| `idle_after_ms` | int | 2500 | TUI-4 |
| `quiet_idle_after_ms` | int | = `idle_after_ms` | TUI-4 |
| `clear_input_keys` | string | none | for example `"\u0015"` (Ctrl-U); `clear_input_sequence` |
| `[tui.ready]` | table | absent | `regex` (empty input box visible), `not_regex` list (dialogs) |
| `[tui.busy]` | table | absent | `working_regex` -> `Some(true)`, `idle_regex` -> `Some(false)`, matched against the last 12 visible lines |
| `[tui.chrome]` | table | absent | `line_regex` array, matched against the trimmed LOWERCASED line |
| `[tui.tool_activity]` | table | absent | `line_regex` array, same input |
| `[tui.menu]` | table | absent | `regex` array: any match = open menu (`looks_like_menu`) |
| `[tui.dialogs]` | table | absent | `strip_blocks`: array of `{start_regex, end_regex}` removing boxed dialog text (`strip_dialogs`) |
| `[tui.tidy]` | table | absent | `dedupe_adjacent_lines` bool (default `false`) |
| `[tui.footer_model]` | table | absent | `regex` with a group named `model` |
| `[[tui.prompt]]` | array of tables | `[]` | approval/trust prompts, below |
| `[tui.delivery]` | table | absent | TUI-5 |
| `[tui.classify]` | table | absent | `waiting_regex` -> `PaneStatus::WaitingInput`; evaluated before the generic heuristic |

`[[tui.prompt]]`:

| Field | Type | Default | Constraints |
|---|---|---|---|
| `kind` | string | req | `command_approval`, `trust`, `broad_approval`, `question`, or any `[a-z_]+` |
| `detect_regex` | string | req | matched case-insensitively against the screen |
| `choices` | array of `{id, key?, text?}` | req | `id` such as `allow_once, deny, allow_all, trust, quit`; each has a `key` (a named key from `pty/keys.rs`) or `text` |
| `broad` | bool | `false` | `allow_all` is allowed only with `allow_broad` (SEC-7) |
| `not_when_regex` | string | none | suppress when also present (answered dialogs stay in scrollback) |

#### `[standby]`, `[model]`, `[context]`, `[mcp_host]` (all optional)

| Table.field | Type | Default | Constraints |
|---|---|---|---|
| `standby.visible` / `standby.headless` | bool | `false` | STB-2 |
| `model.source` | `none` / `footer_regex` / `output_path` / `probe_command` | `none` | MOD-1 |
| `model.probe_argv` | array<string> | none | with `probe_command` |
| `model.probe_regex` | string | none | group `model` |
| `model.default` | string | none | informational |
| `context.smartness` | int | 5 | 1-10 |
| `context.strengths` | array<string> | `["implementation"]` | subset of the `AgentCapability` kebab names (`agent_contexts/mod.rs`) |
| `context.notes` / `context.best_for` | array<string> | `[]` | <= 6 entries, <= 200 chars; `notes` also feed `capability_notes` |
| `mcp_host.config_path` | string | req if table present | `~`-expanding, HST-3 |
| `mcp_host.format` | `json` / `toml` / `yaml` | req | |
| `mcp_host.key_path` | string | req | dotted, for example `mcpServers.puppet-master` |
| `mcp_host.entry` | table | req | template with `{command}`, `{args}`, `{env}` |

### 7.2 Complete example 1: Kilo Code (the proof agent)

This is `packages/app/src-tauri/tests/agent-definitions/kilo.toml` verbatim. Headless flags verified with `kilo`
7.0.50 and re-checked on 7.8.8 (`AGENT-CHOICE.md`); the `[tui]` tables are backed by real 7.8.8 screen captures
(`src/tui/declarative/fixtures/kilo/tui_*.txt`, phase T23).

```toml
# Kilo Code (@kilocode/cli), headless half. Built from the facts verified in
# docs/specs/agent-definitions/AGENT-CHOICE.md ("Verified in T05") and re-checked on kilo 7.8.8
# (T21 fixtures in src/tui/declarative/fixtures/kilo/). The [tui] tables are from real kilo 7.8.8 captures (T23).
#
# Install: copy this file to <app data>/agents/kilo.toml, then approve it (Settings > Agents, or
# POST /agents/definitions/approve from the app session). Always pass a model, for example
# kilo/kilo-auto/free: the default model on a fresh machine can be rejected.
schema_version = 1

[agent]
id = "kilo"
label = "Kilo Code"
icon = "KC"
description = "Kilo Code CLI (@kilocode/cli), an OpenCode-derived agent with a JSON event stream."

[binary]
names = ["kilo", "kilocode"]
windows_extensions = ["exe", "cmd"]
shim_native_globs = ["node_modules/@kilocode/cli/node_modules/@kilocode/cli-windows-x64*/bin/kilo.exe"]
version_args = ["--version"]
install_hint = "npm install -g @kilocode/cli"

[features]
dispatchable = true
# `--agent ask` is read-only by configuration (T05 check 5): bash/edit/write are off and every other
# permission is denied. `--agent plan` is NOT read-only and is never used for this.
read_only_headless = true
read_only_note = "Read-only runs use `--agent ask`: no shell, edit or write tools. It can still reach the network (webfetch, websearch)."
resumable_headless = true

[run_options]
# Shell and edit tools run in the project directory without any flag (T05 checks 1-2), so
# allow_shell is an accepted no-op. `--auto` only approves `ask`-level permissions (outside the
# project dir, `.env` reads); it is deliberately not mapped to allow_shell.
allow_shell = true
shell_default_on = true

[headless]
argv = ["run", "--format", "json"]
task_separator = "--"

[[headless.optional]]
when = "read_only"
args = ["--agent", "ask"]

[[headless.optional]]
when = "model"
args = ["-m", "{model}"]

[[headless.optional]]
when = "resume"
args = ["-s", "{session}"]

[headless.output]
format = "json_lines"

# The answer is the LAST non-blank text event: a tool-call step can emit a whitespace-only text
# before the final one, and an auto-rejected permission leaves the last text empty (T05 check 3).
[headless.output.result]
where = { type = "text" }
where_regex = { "part.text" = "\\S" }
path = "part.text"
pick = "last"

[headless.output.session]
where = { "~sessionID" = true }
path = "sessionID"
pick = "first"

# A turn is over at the first step_finish with reason "stop". The process may not exit after it
# (T05 check 4), so the runner stops the child shortly after this line instead of waiting for exit.
# A turn that ends on `tool-calls` (a rejected permission) has no `stop`; process exit ends it.
[headless.output.done]
where = { type = "step_finish", "part.reason" = "stop" }

# kilo 7.8.x reports the model that ran in step_finish.
[headless.output.model]
where = { type = "step_finish" }
path = "part.model.modelID"
pick = "last"

# Two error lines can arrive (a generic one, then the specific one): the last message is the useful
# one. The exit code is no signal: 0, 1 or never (T05 check 6).
[headless.output.error]
where = { type = "error" }
path = "error.data.message"
pick = "last"

# Visible mode. Every regex below is checked against real kilo 7.8.8 captures (T23):
# src/tui/declarative/fixtures/kilo/tui_{idle,working,permission,reply}.txt.
# The 121-column pane shows kilo's sidebar, and the app's screen text joins full-width rows, so
# patterns match anywhere in the tail instead of anchoring whole rows (except line tables, which
# are matched against single rows when a row is on its own).
[tui]
argv = []
idle_after_ms = 2500
quiet_idle_after_ms = 1200
clear_input_keys = "\u0015"

[[tui.optional]]
when = "model"
args = ["-m", "{model}"]

[[tui.optional]]
when = "resume"
args = ["-s", "{session}"]

# `esc interrupt` sits in the footer for the whole turn (animated bar before it) and is gone once
# the reply is done. The footer `ctrl+p commands` shows in both states, so it only counts as idle
# when no working marker is present (working wins).
[tui.busy]
working_regex = ["esc\\s+interrupt"]
idle_regex = ["ctrl\\+p\\s+commands"]

# Footer hint row, sidebar headings and totals, and the model/timing row under a reply.
[tui.chrome]
line_regex = [
  "^\\s*tab agents\\s+ctrl\\+p commands",
  "^\\s*(?:[⠀-⣿⬝■]+\\s+)?esc interrupt",
  "^\\s*▼\\s",
  "^\\s*[\\d.,]+k?\\s+tokens\\s*$",
  "^\\s*\\d+%\\s+used\\s*$",
  "^\\s*\\$[\\d.]+\\s+spent\\s*$",
  "^\\s*▣\\s+code\\s+·",
]

# Rows above a reply: `+ Thought: 1.3s`, `→ Read .env · approved by you`, spinner `⠋ Read .env`.
[tui.tool_activity]
line_regex = [
  "^\\s*\\+ thought:",
  "^\\s*→ \\S",
  "^\\s*[⠀-⣿] \\S",
]

# The permission dialog's choice row.
[tui.menu]
regex = ["⇆\\s*select\\s+enter"]

# `▣  Code · Auto Free · Nemotron 3 Ultra (free) · 19.0s`: the model is the third field.
[tui.footer_model]
regex = "▣\\s+code\\s+·\\s+[^·\\n]+?\\s+·\\s+(?P<model>[^·\\n]+?)\\s+·\\s+\\d"

# Verified delivery (T24): the first turn of a fresh pane can be typed before kilo's input handler
# is bound and is then lost, so the task is typed, checked in the input box, and only then sent.
# The regexes document where kilo echoes input (the boxed prompt) and the transcript.
[tui.delivery]
input_echo_regex = "┃\\s+\\S"
transcript_echo_regex = "┃\\s+\\S"

# The dialog opens on "Allow once", so Enter allows once; Escape rejects.
[[tui.prompt]]
kind = "command_approval"
detect_regex = "permission required"
choices = [
  { id = "allow_once", key = "enter" },
  { id = "deny", key = "escape" },
]

[model]
source = "output_path"

[context]
smartness = 6
strengths = ["implementation", "debugging"]
notes = ["Model is chosen as provider/model, for example kilo/kilo-auto/free."]
```

### 7.2b Complete example 2: minimal TUI-only agent

```toml
schema_version = 1

[agent]
id = "mycoder"
label = "My Coder"
experimental = true

[binary]
names = ["mycoder"]
install_hint = "see https://example.invalid/mycoder"

[tui]
argv = []

[tui.busy]
working_regex = ["thinking\\.\\.\\."]

[[tui.prompt]]
kind = "command_approval"
detect_regex = "allow this command\\?"
choices = [
  { id = "allow_once", key = "y" },
  { id = "deny", key = "n" },
]
```

This registers a visible-only agent. There is no `[headless]`, so `run_agent(headless=true)` returns
`HEADLESS_UNSUPPORTED` (existing wording: `headless launch is unsupported for agent type: mycoder`);
`headless=false` creates a pane and uses the shared screen heuristics plus the two tables above.

### 7.3 Binary resolution (needed for Kilo on Windows)

`tui::executable(name)` today appends `.exe` and scans `PATH` (`tui/mod.rs::find_path_file`). That
fails for `kilo` on Windows: npm creates `kilo`, `kilo.cmd` and `kilo.ps1` in `%APPDATA%\npm` and no
`.exe` (verified on this machine). Resolution for a definition:

1. For each name and each `windows_extensions` entry (Windows) or the bare name (Unix), scan `PATH`
   (SEC-3).
2. A `.exe`/`.com` hit is used as is.
3. A `.cmd`/`.bat`/`.ps1` hit with `shim_native_globs` set: resolve each glob relative to the directory
   holding the shim and use the first existing `.exe`. This is how `kilo.exe` is reached
   (`%APPDATA%\npm\node_modules\@kilocode\cli\node_modules\@kilocode\cli-windows-x64\bin\kilo.exe`,
   verified present).
4. A `.cmd`/`.bat` hit with no native match is refused with `AGENT_DEF_SHIM_NEEDS_NATIVE`, because argv
   containing `&`, `"` or `%` passes through `cmd.exe` quoting rules (the "BatBadBut" class) and the task
   is attacker-influenced text. `[binary] allow_cmd_shim = true` permits it only for definitions that
   deliver the task on stdin (`[headless.stdin]`), never in argv.
5. Unix: a PATH hit with the execute bit.

### 7.4 Error codes

Loader codes (shown in the definitions list and `agent_definitions_changed.errors`):

| Code | When |
|---|---|
| `AGENT_DEF_INVALID_TOML` | parse error (line/col in context) |
| `AGENT_DEF_UNKNOWN_FIELD` | `deny_unknown_fields` hit; context has the path |
| `AGENT_DEF_SCHEMA_UNSUPPORTED` | `schema_version` unsupported |
| `AGENT_DEF_FIELD_RESERVED` | `base`, `kind="native_protocol"`, and similar |
| `AGENT_DEF_BAD_ID` | VAL-1 pattern |
| `AGENT_DEF_RESERVED_ID` | collides with a code profile or reserved word |
| `AGENT_DEF_DUPLICATE_ID` | two files in one source claim an id (both rejected) |
| `AGENT_DEF_SHADOWED` | lower-precedence copy (informational) |
| `AGENT_DEF_BAD_REGEX` | does not compile |
| `AGENT_DEF_REGEX_TOO_LARGE` | exceeds size or count limits |
| `AGENT_DEF_BAD_PLACEHOLDER` | unknown placeholder, wrong `{task}` count, placeholder in a disallowed table |
| `AGENT_DEF_BAD_WHEN` | `when` outside the closed set |
| `AGENT_DEF_BAD_BINARY` | VAL-3 |
| `AGENT_DEF_SHELL_INTERPRETER` | VAL-4 |
| `AGENT_DEF_DENIED_ENV` | VAL-5 |
| `AGENT_DEF_TOO_LARGE` | FMT-6 |
| `AGENT_DEF_INCONSISTENT` | for example `resumable_headless` without a session selector; read-only flag without note |
| `AGENT_DEF_SOURCE_DISABLED` | source switched off (informational) |
| `AGENT_DEF_NO_CONSENT` | project/extra dir not consented |
| `AGENT_DEF_UNAPPROVED` | needs first approval |
| `AGENT_DEF_APPROVAL_STALE` | fingerprint changed since approval |
| `AGENT_DEF_BINARY_CHANGED` | resolved path differs from the approved one (SEC-4) |
| `AGENT_DEF_SHIM_NEEDS_NATIVE` | 7.3 step 4 |

Run-time codes (bridge/MCP): `UNKNOWN_AGENT_TYPE` (400), `AGENT_NOT_INSTALLED` (carries `install_hint`),
`AGENT_REPORTED_ERROR`, `AGENT_OUTPUT_UNPARSEABLE` (exit 0 but no result selector matched; the run fails
rather than guessing), `READ_ONLY_UNSUPPORTED` (existing), `INVALID_AGENT_TYPE` (legacy alias, deprecated).

### 7.5 New routes and commands (JSON shapes)

Bridge routes require the existing session header except where noted. Tauri commands mirror them for the UI.

`GET /agents/types`

```json
{
  "agents": [
    {
      "id": "kilo", "label": "Kilo Code", "icon": "KC", "kind": "cli",
      "source": "user", "experimental": false,
      "dispatchable": true, "not_dispatchable_reason": null,
      "installed": true, "install_hint": "npm install -g @kilocode/cli",
      "modes": {"headless": true, "visible": true},
      "features": {"read_only_headless": false, "read_only_spawned_pane": false, "resumable_headless": true},
      "run_options": {"trust_workspace": false, "allow_shell": true, "shell_default_on": false, "allowed_commands": false},
      "standby": {"visible": false, "headless": false},
      "definition": {"path": "C:\\Users\\me\\AppData\\Roaming\\com.puppetmaster.app\\agents\\kilo.toml",
                     "fingerprint": "sha256:9f2c...", "state": "active"}
    }
  ],
  "dispatchable_ids": ["claude", "codex", "cursor_agent", "opencode", "opencode_native", "kilo"]
}
```

`GET /agents/definitions` returns `{sources: [{name, enabled, path, consent}], definitions: [{id?, file,
source, state, errors: [{code, message, line?, column?, path?}], fingerprint, shadowed_by?}]}`.

`POST /agents/definitions/reload` body `{}` returns the same body plus `{diff: {added, removed, changed}}`.

`POST /agents/definitions/validate` body `{"toml": "<text>", "source": "user"}` returns `{ok, errors,
rendered: {headless_argv, tui_argv}}`; it registers and approves nothing.

`POST /agents/definitions/approve` body `{"file": "<abs path>", "fingerprint": "sha256:..."}` returns
`{ok, id, state}`, or `AGENT_DEF_APPROVAL_STALE` if the fingerprint is no longer current. App session only
(APR-7). `POST /agents/definitions/revoke` body `{"id": "kilo"}`.
`POST /agents/definitions/consent` body `{"kind":"project"|"extra","path":"...","enabled":true}`.

Error body (existing shape from `bridge/errors.rs`): `{"error":{"code":"UNKNOWN_AGENT_TYPE","message":
"unknown agent_type: foo","retryable":false,"context":{"valid":["claude","codex","kilo"],"hint":"GET /agents/types"}}}`.

Event `agent_definitions_changed`: `{added:[id], removed:[id], changed:[id], errors:[{file,code}]}`.

### 7.6 Settings and config files

| File | Location | Shape |
|---|---|---|
| Source switches, extra dirs, consents | `<app data>/agent-definitions.json` (`app_paths::app_data_dir()`, `%APPDATA%\com.puppetmaster.app` on Windows) | below |
| Approvals | `<app data>/agent-approvals.json` | `{"version":1,"approvals":[{"id","source","path","fingerprint","binary":"<abs resolved path>","approved_at":"<rfc3339>"}]}` |
| User definitions | `<app data>/agents/*.toml` | 7.1 |
| Drafts (never loaded) | `<app data>/agents/drafts/*.toml` | recorder output |
| Project definitions | `<project>/.puppet-master/agents/*.toml` | 7.1 |

```json
{
  "version": 1,
  "sources": {
    "bundled": {"enabled": true},
    "user": {"enabled": true},
    "project": {"enabled": false, "consented_projects": []},
    "extra": {"enabled": false, "dirs": [{"path": "D:\\team-agents", "consented": false}]}
  },
  "watch": true
}
```

Same persistence pattern as `standby/config.rs` (`default_path`, `load`, `save`, a bad file means
defaults, values clamped on load) and the live-apply behaviour of commit `f54056a`. Bundled definitions
live in `packages/app/src-tauri/agents/`; Phase 2 ships none (Kilo ships as a user file to prove the
path); a follow-up may bundle it.

## 8. Security model and threat analysis

Trust levels: bundled (reviewed, in the binary) > user dir (the user wrote or accepted it) > extra dir
(maybe shared or synced) > project-local (arrives with a cloned repository; hostile by default).

| # | Threat | Vector | Mitigation |
|---|---|---|---|
| T1 | A repo ships `.puppet-master/agents/x.toml` that runs a malicious binary | project source | OFF by default; per-project consent; approval of exact argv; binary pinned (SRC-3, SEC-2, SEC-4) |
| T2 | A definition shadows a trusted id (`claude`) | id collision | code profiles win; reserved ids; shadowing listed (REG-7/8, VAL-1) |
| T3 | Shell injection through argv | task text with `;`, `&&`, backticks | no shell anywhere; argv elements; `.cmd` shims refused unless stdin (SEC-1, 7.3) |
| T4 | Option injection: task starts with `-` | positional task | `task_separator` or `task_flag` mandatory (HDL-7) |
| T5 | PATH/cwd binary planting (a `kilo.exe` in the project dir) | Windows cwd search | explicit PATH scan excluding cwd/project; path pinned and re-checked (SEC-3/4) |
| T6 | Definition edited after approval (TOCTOU) | file swap, sync tools | fingerprint recomputed at load AND at spawn (APR-4) |
| T7 | ReDoS / memory exhaustion | regex or giant file | linear engine, size and count limits, input cap (FMT-6, VAL-2, SEC-6) |
| T8 | Environment tampering (`LD_PRELOAD`, `NODE_OPTIONS`) | `[env]` | denylist (VAL-5) |
| T9 | Auto-approve escalation | `[[tui.prompt]]` answering "allow all" | `broad` needs caller `allow_broad`; never auto (SEC-7) |
| T10 | False read-only claim | `read_only_headless=true` on a CLI that writes | needs a `read_only` segment + note shown at approval; live "no file written" test for bundled definitions (SEC-8, PLAN P3) |
| T11 | Secret leakage | recorder drafts, event log | redaction, unrendered argv in logs, drafts not loaded (REC-3/4, SEC-9) |
| T12 | UI spoofing via label (`Claude Code (official)`) or markup | label/description | length limits, plain-text render, source badge always shown (UI-3/4) |
| T13 | Output parser DoS | multi-GB output | `max_output_bytes` (HDL-8) |
| T14 | Hot reload swaps argv under a live run | watcher | runs hold `Arc` snapshots; standby discarded on fingerprint change (REG-2, STB-4) |
| T15 | An MCP client approves its own malicious definition | MCP surface | approval is not an MCP tool; app session only (APR-7) |
| T16 | Interpreter used as the "agent" (`python -c`) | argv flags | interpreters denied by name unless `allow_interpreter`, refused for project/extra, warned at approval (VAL-4) |
| T17 | Symlink in an agents dir pointing at a sensitive file leaks content in an error message | scan | non-recursive, out-of-dir symlinks skipped, errors never echo more than an 80-char snippet (SRC-6) |

Residual risk: a user approves a definition whose binary is itself dangerous. The spec shows the exact
argv and flags like `--auto`; it cannot judge the CLI.

## 9. Compatibility and migration

- **catalog_version**: exactly one bump, in Phase 0, when the `agent_type` enums become the string
  pattern (API-1, API-5). Update the pin in `tool_registry/tests.rs::catalog_version_is_pinned` and
  mirror the schema change in `packages/mcp-server/src/catalog.ts`.
- **SCHEMA_DRIFT**: an MCP shim built before the bump reports `SCHEMA_DRIFT` against the new bridge
  (`bin/puppet_master_mcp/bridge_http.rs`). Handling: rebuild the dist exe (memory
  `rebuild-mcp-after-changes`: separate `CARGO_TARGET_DIR` + rename swap), then check that
  `/health.catalog_version` equals the shim's. Later agent additions do not change schemas, so drift
  recurs only on genuine schema edits.
- **Existing callers passing old values**: all nine ids stay valid with unchanged behaviour.
  `delegate_work` accepted `cursor` (the IDE) and still does; `INVALID_AGENT_TYPE` stays for one release
  as an alias; new code is `UNKNOWN_AGENT_TYPE`. Persisted runs (`agent_runs/persist.rs`) store
  `agent_type` strings and reload through the registry; a run whose definition is gone loads as
  `agent_gone`: viewable, not resumable.
- **TS shared enum fallback**: `AgentTypeSchema` becomes a validated string (UI-1); the `AgentType` TS
  type becomes `string` with `BuiltinAgentType` as the union of the nine; `AGENT_META` lookups go through
  `presetFor(id)` with a fallback; switches over the union get a `default` branch.
- **Rust**: `AgentType` stays for the spawn/launch layer (`resolve_command`) and serialises as before;
  every `AgentType::parse` used as a validity gate switches to registry lookup. `agent_contexts` moves to
  `Cow<'static, str>` so declarative contexts can fill it.
- **pm-regression** (`scripts/pm-regression.mjs`, 28 scenarios) passes 28/28 after every phase; its
  scripts already pass agent types as strings.
- **Rollback**: switching sources off returns the app to code-only behaviour; deleting
  `agent-definitions.json` restores defaults.

## 10. Assumptions (risky ones in bold)

1. **Kilo's `run --format json` event shape is stable.** Verified on 7.0.50: `step_start`, `text`
   (`part.text`), `tool_use`, `step_finish` (`part.reason` `stop` or `tool-calls`), `error`
   (`error.data.message`), all carrying `sessionID`. A CLI update could rename them; fixtures catch it and
   users get `AGENT_OUTPUT_UNPARSEABLE`, not a wrong result.
2. **Kilo has no `.exe` on PATH on Windows**, only npm shims; the native `kilo.exe` is under
   `node_modules/@kilocode/cli/node_modules/@kilocode/cli-windows-x64*/bin/`. The glob depends on npm's
   global layout (verified here; pnpm, bun or a different prefix may differ). Fallback is
   `AGENT_DEF_SHIM_NEEDS_NATIVE` with a clear message.
3. **Kilo's visible TUI** (verified in T23/T24 on kilo 7.8.8 with real PTY captures; `kilo.toml` `[tui]`
   tables come from `fixtures/kilo/tui_{idle,working,permission,reply}.txt`). Two facts differ from the OpenCode
   guess: the first turn is lost unless `[tui.delivery]` waits for the input to be ready, and permission
   replies must be raw key bytes (the answer routes append Enter themselves).
4. **CORRECTED (T05, AGENT-CHOICE.md):** `--auto` is NOT needed for shell tool use in `kilo run`: bash and edit
   run in the project directory with no flag. `--auto` only approves `ask`-level permissions (outside the project
   dir, `.env` reads, doom-loop), so it is not mapped to `allow_shell` (which is an accepted no-op for Kilo).
   `--agent plan` is NOT read-only; `--agent ask` is (bash/edit/write off, the rest denied), so
   `read_only_headless = true` rides on `[[headless.optional]] when = "read_only"` with `--agent ask`. `ask` can
   still use the network. A turn whose tool call was auto-rejected ends with `reason="tool-calls"`; `stop` is
   not always emitted and the process may not exit after it, so the runner treats process exit as end of turn and
   stops the child after `stop`. The result is the last NON-EMPTY `text`. The exit code is no signal (0, 1 or never).
5. **The default model fails on this machine** (OpenRouter 404 "No endpoints found that support tool use";
   an `error` event is printed). In a second attempt the process had not exited after 60 s (timeout hit), so
   an error can leave Kilo running: HDL-5/HDL-6 therefore treat a matched `error` selector as a finished
   (failed) turn, and the headless runner must still enforce its own timeout and kill. The error event
   is what stops this from looking like an empty success. `-m kilo/kilo-auto/free` works, so the live proof
   passes `model="kilo/kilo-auto/free"`; the free tier's availability may change.
6. `--` before the message is accepted (VERIFIED: `kilo run --format json -m <m> -- "-reply ..."` ran the
   dash-leading task as text). `task_separator = "--"` is therefore safe for Kilo.
7. **ArcSwap reads are safe on the PTY reader threads** (profile resolution on the reader and idle threads
   in `pty/registry/spawn.rs`). VERIFIED (T06): `registry.rs` has a concurrent swap/read stress test; a turn also
   holds its `Arc` profile, so a swap never changes a run that already started (T36b).
8. Rust `regex` (no look-around) is enough for TUI patterns. True for Cursor's rules by inspection of
   `tui/cursor/chrome/`, not exhaustively.
9. CORRECTED: `TuiProfile::id()` stayed `-> &'static str`. Declarative profiles intern their id, warning and reason
   strings with `Box::leak` once per loaded definition (T16), so no call site changed; the cost is a small leak per
   reload of a changed definition.
10. Bundled definitions: the loader accepts embedded `(name, text)` pairs (trusted, SRC-1) but none ship;
    `kilo.toml` stays a user-dir file so the approval flow remains the proof (open question 1, default taken).

## 11. Open questions

1. Should `kilo.toml` ship bundled (trusted, no approval) after Phase 3, or stay user-dir only so the
   approval flow remains the proof?
2. For `.cmd`-only installs with no native exe (some agents), is stdin task delivery acceptable, or do we
   refuse and tell the user to install a native build?
3. Should project consent be keyed by canonical path only, or also by git remote? Default here: path only.
4. Is a live "no file written" test per declared read-only agent mandatory for bundled definitions?
   Spec default: yes for bundled, note-only for user definitions.
5. Is the hybrid `base = "<code profile>"` worth building in this iteration (Phase 3 optional) or deferred
   until a concrete need?
6. Do we want a per-agent cost/concurrency note, since declarative agents can be paid APIs?
7. Command Code (`cmdcode`) is not installed; should a second proof agent be required later so the format
   is not Kilo-shaped?
8. How long should recorder raw captures be retained (default here: until the app closes)?

## 12. Done when (checkable)

1. `cargo test --lib tui::` passes, including declarative conformance over every definition under
   `packages/app/src-tauri/tests/agent-definitions/` and the Kilo definition.
2. A single file `kilo.toml` in `<app data>/agents/` (no Rust/TS edits) registers `kilo`: it appears in
   `GET /agents/types` with `source: "user"`, `state: "active"` after approval, and in the spawn menu.
3. `run_agent(agent_type="kilo", project_path=..., task=...)` headless returns a result on the live bridge
   whose receipt records `agent_definition.id = "kilo"`; `followup_task` on the handle resumes the same Kilo
   session (equal `sessionID`, and the second answer uses first-turn context).
4. `run_agent(agent_type="kilo", headless=false, ...)` creates a visible pane, delivers the task and
   returns a result (screen-captured results flagged `result_inferred`); `followup_task` works on it.
5. An unknown id returns `UNKNOWN_AGENT_TYPE` listing valid ids; an unapproved or edited definition fails
   with `AGENT_DEF_UNAPPROVED` / `AGENT_DEF_APPROVAL_STALE`.
6. Fixture tests exist for the Kilo headless parser (`success`, `tool turn`, `error event`, `resume`,
   `garbage`) and the TUI tables, and a parity test shows `DeclarativeProfile` reproducing the Claude
   headless contract on the existing Claude fixtures.
7. `catalog_version` bumped once and pinned; shim and bridge agree; the dist MCP exe is rebuilt and
   `SCHEMA_DRIFT` is absent against the new bridge.
8. `scripts/pm-regression.mjs` passes 28/28 against the new build.
9. `docs/REPO_MAP.md` ("Add an agent" row plus the `tui/` and registry rows) and `tui/README.md` are
   updated; `DEFINITION-FORMAT.md` matches the implemented schema.
10. The Phase 0 scoping fix is proven by a test that gives profile A a chrome rule and shows profile B's
    reply is untouched.
