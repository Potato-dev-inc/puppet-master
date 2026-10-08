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
  cursor/         Cursor Agent CLI + Cursor IDE launcher
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
   editing for a new registered agent.
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

## Adding an agent without code

If the CLI can be driven by an argument list, a TOML file replaces steps 1-4 above: no Rust, no rebuild.

1. Write `<app data>/agents/foo.toml` (Windows: `%APPDATA%\com.puppetmaster.app\agents\`). Format and examples:
   `docs/specs/agent-definitions/DEFINITION-FORMAT.md`; the worked example is `kilo.toml` (SPEC 7.2).
2. Check it without registering anything: `POST /agents/definitions/validate` with `{"toml": "...", "source": "user"}`
   returns `{ok, errors, rendered}` (rendered argv with placeholders). Settings > Agents lists errors with file and line.
3. Approve it in Settings > Agents (the dialog shows the argv). A new or changed file stays `unapproved` until then.
4. Fixtures: real captures go in `tui/declarative/fixtures/<name>/`; `conformance.rs` runs its `rules` over every
   definition in `fixtures/definitions`. Run `cargo test --lib tui::declarative`.

Code stays the right choice when the agent needs a protocol client (OpenCode native) or logic a table cannot express.
Declarative profiles live in `tui/declarative/` (`DeclarativeProfile` implements `TuiProfile`; generic code never
sees the difference) and register through `loader::reload`.

## Where a fix for an existing TUI goes

| Symptom | File |
|---|---|
| Pane shows idle/blocked/running wrongly | `<tui>/screen.rs` (`classify_screen`) |
| Approval or trust prompt not detected / wrong key sent | `<tui>/screen.rs` (`detect_prompt`, `prompt_reply`) |
| Headless result empty / session not resumed | `<tui>/headless.rs` or the `extract_*` methods |
| Wrong flags on a headless launch | `<tui>/launch.rs` (`headless_command`) |
| Reply text polluted with banner / tool rows | `<tui>/chrome.rs` |
| Model name wrong | `detect_footer_model` / `default_model` |

## Known leftovers (still keyed by `AgentType`, not yet profile methods)

- `pty/agents.rs` per-OS launch commands and `pty/registry.rs` Windows PowerShell hosting for
  `cursor-agent.ps1` (spawn infrastructure, mirrors the TypeScript enum).
- `agent_contexts.rs` display labels/strengths per agent.
- `opencode_native` branches in `bridge.rs`, `agent_runs/*` and `pty/registry.rs`: that is a
  protocol backend, not a screen-scraped TUI, and lives with the OpenCode client.
