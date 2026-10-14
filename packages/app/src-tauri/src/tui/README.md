# TUI profiles

One folder per agent CLI/TUI. Anything that is true of **one** TUI and not the others
lives here, so a break in one TUI is fixed in one place and cannot disturb the rest.

```
tui/
  mod.rs          profile_for, require_registered, shared helpers (read the registry)
  registry.rs     runtime Registry: ArcSwap snapshot of Arc<dyn TuiProfile>, seeded with the nine code profiles
  profile.rs      TuiProfile trait (every per-agent behaviour, with generic defaults)
  types.rs        Kind, Features, LaunchSpec, HeadlessLaunch, Reply, Warning, RunFacts
  generic.rs      default prompt handling + the profile for unregistered agent types
  conformance.rs  rules every registered profile must pass (runs for new profiles automatically)
  declarative/    agents defined by TOML files (schema, loader, approvals, DeclarativeProfile); see "Adding an agent without code"
  cursor/         Cursor Agent CLI (`agent` alias preferred) + interactive `cursor` alias
    launch.rs       headless command line, default model lookup
    headless.rs     reading headless output (result, rejected calls, trust screen)
    screen.rs       interactive TUI: status, approval/trust prompts, footer model
    chrome.rs       which screen lines are furniture, not reply text
    fixtures/       real captures replayed by the tests
  claude/         Claude Code        (mod.rs profile, headless.rs)
  codex/          Codex              (mod.rs profile, headless.rs)
  opencode/       OpenCode TUI + opencode_native, TUI-facing part only (mod.rs, screen.rs)
                  (the HTTP client is crate::opencode)
  shell/          cmd / powershell / bash (mod.rs, prompt.rs)
```

## The rule

Generic code (`pty`, `agent_runs`, `pane_wait`, `bridge`, …) **never matches on an agent-type
string**. It calls `tui::profile_for(agent_type)` and uses a `TuiProfile` method or a `Features`
flag. If you find yourself writing `if agent_type == "cursor_agent"` outside this folder, add a
method (with a generic default) to `TuiProfile` instead.

## Adding a TUI (e.g. a new CLI called `foo`)

1. **Id and PTY launch**: a profile reports its id from `TuiProfile::id()`. A code TUI that
   `pty/agents.rs` should launch directly also gets an `AgentType::Foo` variant (`as_str`,
   `parse`, the `resolve_*` launch commands), mirrored in `packages/shared/src/agents.ts`
   (`BUILTIN_AGENT_TYPES`, used only as the offline fallback). A profile that is not an
   `AgentType` spawns its PTY through `TuiProfile::spawn_command` instead. Gates read the
   registry, never the enum: `tui::require_registered` (HTTP 400 `UNKNOWN_AGENT_TYPE`).
   The UI lists agents from `agents_list` / `lib/agentRegistry.ts`; no frontend list needs
   editing for a new registered agent. A turn holds the `Arc` profile it started with (and the
   receipt's `read_only` comes from `WorkerPersist::run_features`, recorded at spawn), so a
   registry swap or a definition edit never changes a run in flight.
2. **Profile**: create `tui/foo/mod.rs` with a unit struct implementing `TuiProfile`, and split
   it into `launch.rs`/`headless.rs`/`screen.rs`/`chrome.rs` as logic appears (see `claude/`
   for the smallest example, `cursor/` for the fullest). Override only what differs. Start from:

   ```rust
   pub struct Foo;

   impl TuiProfile for Foo {
       fn id(&self) -> &'static str { "foo" }
       // features(): Features { read_only_headless: true, ..Features::CLI }
       // headless_command / extract_result / extract_provider_session  → headless runs
       // classify_screen / detect_prompt / prompt_reply                → visible pane
       // is_chrome_line / is_tool_activity_line / detect_footer_model  → screen result capture
       // capability_notes                                              → caveats in the contract
   }
   ```
3. **Register**: `pub mod foo;` and add `Arc::new(foo::Foo)` to the code list in `Registry::seeded()` (`registry.rs`). Keep the order
   stable: `read_only_backends()` reports it in registration order.
4. **Fixtures + tests**: put real captures in `tui/foo/fixtures/` and replay them in that
   profile's tests. Prefer a real capture over a hand-written screen.
5. `cargo test --lib tui::` — `conformance.rs` checks the new profile against the shared rules
   (unique id, `AgentType` round-trip, shared launch policy, parsers reject garbage, every
   detected prompt can be answered, an empty screen is never "blocked").

## Submit, composer and startup-gate hooks

Declared per profile, generic defaults, used by `agent_runs/delivery.rs`, `bridge/startup_gate.rs` and
`agent_runs/completion.rs` (see `docs/features/fix-live-codex.md`):

- `submit_sequence()`: pause between typed text and the submit keys (Codex: 400 ms, its paste-burst window swallows a quicker Enter).
- `composer_state(screen)`: `Empty / HasText / Working`, so delivery can confirm a submit.
- `startup_gate(screen)`: a screen the agent shows before it can take a task. See "Startup gates" below: one model for every agent.

## Adding an agent without code

If the CLI can be driven by an argument list, a TOML file replaces steps 1-4 above: no Rust, no rebuild.

1. Write `<app data>/agents/foo.toml` (Windows: `%APPDATA%\com.puppetmaster.app\agents\`). Format and examples:
   `docs/specs/agent-definitions/DEFINITION-FORMAT.md`; the worked example is `kilo.toml` (SPEC 7.2).
2. Check it without registering anything: `POST /agents/definitions/validate` with `{"toml": "...", "source": "user"}`
   returns `{ok, errors, rendered}` (rendered argv with placeholders). Settings > Agents lists errors with file and line.
3. Approve it in Settings > Agents (the dialog shows the argv). A new or changed file stays `unapproved` until then.
4. Fixtures: real captures go in `tui/declarative/fixtures/<name>/`; `conformance.rs` runs its `rules` over every
   definition in `fixtures/definitions`. Run `cargo test --lib tui::declarative`.

5. Editing a definition while the app runs is safe: `declarative/watch.rs` reloads on save (150 ms debounce) and emits
   `agent_definitions_changed`. Runs already in flight keep the argv they started with; the next run is refused
   (`approval_stale`) until the changed file is re-approved. If an active file becomes invalid the previous
   registration is kept.
6. Project (`<project>/.puppet-master/agents`) and extra folders load only after per-path consent in Settings > Agents
   (consent never replaces approval).
7a. A definition may carry `[instructions]` (`file`/`files` relative to the project root, optional `header`): the files the agent reads its project instructions from; Puppet Master renders the shared project memory into them (`docs/features/one-brain.md`). Code profiles override `TuiProfile::instruction_files`.
7b. A definition may carry `[mcp_host]` (config_path under home/project, format json/toml/yaml, key_path, entry with
   `{command}`/`{args}`/`{env}`); `mcp_install/declarative.rs` writes only that key, makes a `.bak`, and is idempotent.
8. Share one definition: Settings > Agents > Export (TOML, plus `<name>.fixtures.zip` when `<id>.fixtures/` exists) and
   Import (validated, placed in the user folder unapproved; never overwrites; no network).

Code stays the right choice when the agent needs a protocol client (OpenCode native) or logic a table cannot express.
Declarative profiles live in `tui/declarative/` (`DeclarativeProfile` implements `TuiProfile`; generic code never
sees the difference) and register through `loader::reload`.

## Startup gates (update notices, folder trust, sign-in)

A screen the agent shows before it can take any task is a **startup gate**. One hook, one model, for every agent
(`docs/features/fix-live-claude.md` and `fix-live-codex.md` are the two branches this merges):

```
TuiProfile::startup_gate(screen) -> Option<StartupGate { id, summary, dismiss: Option<keys> }>
```

| gate | `dismiss` | what generic code does |
|---|---|---|
| Dismissible notice (Codex "Update available" menu) | `Some(keys)` (Esc: skip this launch, never Enter, never "update now") | `bridge/startup_gate.rs` closes it with those keys before the task is typed (one gate at a time, waits for the screen to change, at most 4 rounds). It is never a card. If it ignored its keys the run parks until the user deals with it. |
| Only the user may pass it (folder trust, sign-in, onboarding) | `None` | It becomes a **prompt card** and parks the run (`required_action`): kind `workspace_trust` (the trust question) or `startup_screen` (anything else). The run types the task only after the screen is gone. Puppet Master never answers it. |

How the card is built (`pane_prompt::detect_in_screen`): the profile's own `detect_prompt` wins, so Claude and Cursor
give an answerable `workspace_trust` card with `choices [trust, deny]` and the keys to press them. A profile that only
declares `startup_gate` (Codex trust, login, onboarding) gets a card with no choices (the keys are not known): the user
answers in the pane and the card says so. A profile that has an answerable card should ALSO report the gate from
`startup_gate`, so completion and delivery know the pane cannot take a task. `classify_screen` should read the gate as
`waiting_input` (many gates print no y/n text).

What happens around a gate, the same for every agent:

- `bridge/dispatch.rs`: dismiss the dismissible ones, then park on whatever is left (`bridge/approvals.rs`).
- `bridge/approvals.rs`: the wait ends when no gate and no prompt is on screen. If the agent exits while a gate
  was showing, the run fails: `WORKSPACE_TRUST_DECLINED` for the trust gate (the user said no), `STARTUP_SCREEN_NOT_COMPLETED`
  for a sign-in or setup screen. Neither is retried and neither falls back to a headless run in a folder the user just refused.
- `pane_prompt::dispatch_blocker` refuses to type over a gate (`PANE_BLOCKED_BY_PROMPT`).
- `agent_runs/completion.rs`: while a gate shows, the turn never completes, so a startup screen is never a run's answer.
- `tui::conformance`: every registered and declarative profile's gates are sane (no panics on huge or blank screens,
  nothing dismissible on a blank screen).

Adding a gate for a new agent: return a `StartupGate` from `startup_gate` with a real capture as a fixture; give it
`dismiss` only if the keys close the screen without choosing anything that changes the machine or grants trust.
Tests that pin the model: `bridge/startup_gate_tests.rs` (Claude and Codex through the same functions),
`bridge/startup_gate.rs`, `tui/claude/prompt_tests.rs`, `tui/codex/screen.rs`.

## Where a fix for an existing TUI goes

| Symptom | File |
|---|---|
| Pane shows idle/blocked/running wrongly | `<tui>/screen.rs` (`classify_screen`) |
| Approval or trust prompt not detected / wrong key sent | `<tui>/screen.rs` (`detect_prompt`, `prompt_reply`); Claude: `claude/prompt.rs` |
| Headless result empty / session not resumed | `<tui>/headless.rs` or the `extract_*` methods |
| Wrong flags on a headless launch | `<tui>/launch.rs` (`headless_command`) |
| Reply text polluted with banner / tool rows | `<tui>/chrome.rs` |
| Follow-ups park on "the input box already holds text" while it shows a faint suggestion or hint | `<tui>/screen.rs` + `TuiProfile::input_screen` (leave the agent's own faint cells out; helper `tui/faint.rs`; Claude `without_suggestion`, Codex `without_placeholder`) |
| Model name wrong | `detect_footer_model` / `default_model` |

## Known leftovers (still keyed by `AgentType`, not yet profile methods)

- `pty/agents.rs` per-OS launch commands and `pty/registry.rs` Windows PowerShell hosting for
  `cursor-agent.ps1` (spawn infrastructure, mirrors the TypeScript enum).
- `agent_contexts.rs` display labels/strengths per agent.
- `opencode_native` branches in `bridge.rs`, `agent_runs/*` and `pty/registry.rs`: that is a
  protocol backend, not a screen-scraped TUI, and lives with the OpenCode client.

## Agent doctor and activity hooks

Optional `TuiProfile` methods: `version_command`, `install_hint`, `login_hint` (agent doctor, `agent_health/`) and `activity` (plain-language activity line, default in `activity.rs`). TOML: `binary.login_hint`, `[tui] activity_regex`. See `docs/features/agent-doctor-and-activity.md`.
