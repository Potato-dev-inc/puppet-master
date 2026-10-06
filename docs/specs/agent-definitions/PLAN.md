# Declarative agent definitions: PLAN

Companion to `SPEC.md` (requirement ids such as REG-1 refer to it). Format per task:
`- [ ] N. <task> — files it may touch — done-when`. Each task is roughly one commit. The file list is an
allow-list: touching anything else needs a plan edit. Paths are relative to the repo root; `src/` means
`packages/app/src-tauri/src/`.

Global rules for every task: commit finished work unasked, give each handoff a fresh `build_id`; edits to
`src-tauri` restart the live dev app (about 1 minute), so never edit while a live test runs; rebuild the dist
MCP exe after any MCP-affecting edit (separate `CARGO_TARGET_DIR` + rename swap); keep
`scripts/pm-regression.mjs` at 28/28 at each phase gate; use unique handle names in tests.

Dependency sketch: P0 -> P1 -> P2 -> (P3, P4, P5 in parallel after P2; P5 hot reload needs P2 loader).

---

## Phase 0: groundwork (no new feature, no behaviour change except the bug fix and one schema bump)

Unlocks: safe composition of many profiles; one stable MCP schema so adding agents never changes
`catalog_version` again. Effort: 2-3 days. Risk: the `catalog_version` bump invalidates the pinned test and
any running shim (SCHEMA_DRIFT).

- [ ] P0-1. Scope the five free fold dispatchers to one profile — `src/tui/mod.rs`, `src/tui/conformance.rs`,
  `src/agent_runs/completion/clean.rs`, `src/pane_wait/check.rs`, plus every caller `grep` finds of
  `tui::is_chrome_line|strip_dialogs|tidy_reply|is_tool_activity_line|looks_like_menu` — done-when:
  signatures take `agent_type: &str` and call only `profile_for(agent_type)`; callers pass the pane/run's
  agent type; a new test gives profile A a chrome rule and proves profile B's screen is unchanged
  (SPEC done-when 10); `cargo test --lib` green; existing Cursor/OpenCode fixtures replay unchanged.
  Depends on: nothing. Check first that the callers have an agent type in scope; if a caller has none,
  thread it from `PaneInfo`/run snapshot rather than defaulting to "all profiles".
- [ ] P0-2. Single `agent_type` definition for the MCP catalog — `src/tool_registry/catalog/agents.rs`,
  `src/tool_registry/catalog/operations.rs`, `src/tool_registry/catalog/panes.rs`,
  `src/tool_registry/schema.rs`, `src/tool_registry/tests.rs`, `packages/mcp-server/src/catalog.ts` —
  done-when: one helper `agent_type_schema()` (type string + pattern `^[a-z][a-z0-9_]{1,31}$` + description
  pointing at `GET /agents/types`) is used by every tool that takes `agent_type`/`backend`; the three
  enums are gone; the pinned `catalog_version_is_pinned` value is updated once; the TS catalog matches;
  `cargo test --lib tool_registry` green. Depends on: P0-3 (error code must exist before schema loosens).
- [ ] P0-3. `UNKNOWN_AGENT_TYPE` error + shared lookup helper — `src/tui/mod.rs`, `src/bridge/dispatch.rs`,
  `src/bridge/errors.rs`, `src/bridge/routes/panes.rs`, `src/pty/registry/spawn.rs`,
  `src/operations/` (only the file holding `OperationError`), `src/bridge/tests.rs` — done-when:
  `tui::require_registered(id) -> Result<&dyn TuiProfile, UnknownAgentType{valid}>`; the gate sites above use
  it; the response is HTTP 400 `UNKNOWN_AGENT_TYPE` with `context.valid`; `INVALID_AGENT_TYPE` still returned
  on the `delegate_work` path with the same body; tests cover run/spawn/delegate paths.
- [ ] P0-4. `GET /agents/types` (static content only: the nine code profiles) — `src/bridge/routes/mcp.rs`
  (or the file where `/agents` GET routes live: `src/agent_runs/routes.rs`, `src/agent_runs/routes/list.rs`),
  `src/tui/mod.rs`, `src/tui/types.rs` (a `Descriptor` struct), `src/bridge/tests.rs` — done-when: route
  returns the 7.5 shape for existing ids, `installed` from `is_installed()`, `modes` derived from
  `Features`/`headless_command` support; a unit test asserts every `profiles()` id appears.
- [ ] P0-5. Pre-flight facts for Kilo — `docs/specs/agent-definitions/AGENT-CHOICE.md` only — done-when:
  the three unverified facts (A4 `--auto`/shell, `--agent plan` read-only behaviour, TUI capture) have a
  recorded result or are explicitly left as assumptions; fixtures saved under
  `src/tui/declarative/fixtures/kilo/` are NOT created yet (P2). Optional but cheap.
- [ ] P0-6. Phase gate — build dist exe, restart app — done-when: `/health.catalog_version` equals the shim's
  (no SCHEMA_DRIFT), `scripts/pm-regression.mjs` 28/28.

Test plan: unit (P0-1 scoping, P0-3 errors, P0-4 listing), catalog pin, live pm-regression 28/28.

---

## Phase 1: dynamic registry and `AgentId`

Unlocks: a profile can exist that is not in the source tree; the closed enum stops gating. Effort: 4-6 days.
Risk: the widest compile-fix surface (about 50 `profile_for(` sites, 6 `AgentType::parse` gates,
`agent_contexts`), and a possible hot-path cost.

- [ ] P1-1. Introduce `AgentId` and registry skeleton — `src/tui/registry.rs` (new), `src/tui/mod.rs`,
  `src/tui/profile.rs`, `src/tui/conformance.rs`, `src/tui/claude/mod.rs`, `src/tui/codex/mod.rs`,
  `src/tui/cursor/mod.rs`, `src/tui/opencode/mod.rs`, `src/tui/shell/mod.rs`, `src/tui/generic.rs`,
  `src/lib.rs` (mod decl only), `Cargo.toml` (`arc-swap`) — done-when: `TuiProfile::id(&self) -> &str`;
  `Registry` holds `ArcSwap<Vec<Arc<dyn TuiProfile>>>` seeded with the nine code profiles in the existing
  order; `profile_for` returns `Arc<dyn TuiProfile>`; `read_only_backends()` order unchanged (test); all
  `tui::` tests green. Depends on: P0.
- [ ] P1-2. Convert callers from `&'static dyn TuiProfile` to `Arc` — every file `cargo check` flags under
  `src/agent_runs/`, `src/bridge/`, `src/pane_wait/`, `src/pty/`, `src/standby/`, `src/agent_contexts/`,
  `src/commands/` (allow-list is exactly those directories; no logic changes) — done-when: `cargo check`
  and `cargo test --lib` green; a stress test (`src/tui/registry.rs` tests) swaps the snapshot 10k times
  while 8 threads call `profile_for` and finds no panic and no stale-after-swap beyond one call.
- [ ] P1-3. Replace `AgentType::parse` validity gates with registry lookup — `src/pty/registry/spawn.rs`,
  `src/bridge/dispatch.rs`, `src/bridge/routes/panes.rs`, `src/commands/agent_models.rs`,
  `src/agent_runs/persist.rs`, `src/agent_contexts/inspect.rs`, `src/tui/conformance.rs` — done-when: those
  files call the registry; `AgentType` is used only inside `pty/agents.rs` + the code-profile launch path;
  `conformance.rs` checks "every code profile id resolves through the registry" instead of
  `AgentType::parse`; persisted-run reload tolerates an id that is no longer registered (`agent_gone`).
- [ ] P1-4. Spawn path for non-enum ids — `src/pty/agents.rs`, `src/pty/registry/spawn.rs`,
  `src/pty/registry/command.rs`, `src/tui/types.rs` (`SpawnSpec` returned by a new
  `TuiProfile::spawn_command(&SpawnRequest) -> Option<LaunchSpec>`, default `None` = use `resolve_command`)
  — done-when: a test profile with `spawn_command` spawns a PTY through `spawn.rs` with no `AgentType`;
  the nine code ids spawn exactly as before (live: pm-regression subset spawn scenarios).
- [ ] P1-5. `agent_contexts` owned strings and registry-backed listing — `src/agent_contexts/mod.rs`,
  `src/agent_contexts/profiles.rs`, `src/agent_contexts/inspect.rs`, `src/agent_contexts/tests.rs`,
  `src/tui/profile.rs` (`fn context(&self) -> Option<ContextProfile>`) — done-when:
  `AgentContextProfile` uses `Cow<'static, str>` and `agent_type: AgentId`; the nine profiles keep their
  exact current text (tests compare JSON snapshots before/after); the list is built from the registry.
- [ ] P1-6. Standby keyed by id string — `src/standby/config.rs`, `src/standby/key.rs`, `src/standby/pool.rs`,
  `src/standby/host.rs`, `src/standby/tests.rs` — done-when: no `AgentType` in `standby/`; existing
  `standby-pool.json` files load unchanged; tests unchanged and green.
- [ ] P1-7. TS shared + frontend read the registry — `packages/shared/src/agents.ts`,
  `packages/shared/src/*.test.ts` (if present), `packages/app/src/lib/ansi.ts`,
  `packages/app/src/components/TerminalViewport.tsx`, `packages/app/src/lib/standby.ts`, spawn-menu
  component(s) (find with grep `AGENT_META|AgentTypeSchema`), `packages/app/src/lib/agentRegistry.ts` (new),
  `src/commands/agents.rs` (new Tauri command wrapping the P0-4 listing), `src/lib.rs`/`main` handler
  registration — done-when: `AgentTypeSchema` accepts any pattern-valid id; `presetFor(id)` has a fallback;
  the spawn menu and `TUI_AGENTS` come from the command with the compiled list as offline fallback;
  `npm run typecheck`/shared tests green; UI shows the nine agents exactly as before (manual screenshot).
- [ ] P1-8. Docs for the phase — `docs/REPO_MAP.md` (registry row, `tui/` row), `src/tui/README.md`
  ("Adding a TUI" step 1 no longer needs the enum for declarative agents) — done-when: wording matches code.
- [ ] P1-9. Phase gate — done-when: workspace tests green (`cargo test --lib`, TS tests), dist exe rebuilt,
  pm-regression 28/28 on a fresh `build_id`.

Test plan: unit (registry swap/ordering/stress), snapshot tests for `agent_contexts`, persisted-run reload
test, TS unit tests for `presetFor`, live pm-regression 28/28 plus the spawn-close script
(`scripts/spawn-close-agent.py` if present).

---

## Phase 2: DeclarativeProfile, schema, loader, validation, sources, Settings page, Kilo proof

Unlocks: the definition of done (Kilo through ONE TOML, headless + visible). Effort: 8-12 days (largest
phase). Risk: visible-mode regexes for an unfamiliar TUI; Windows binary resolution; the approval UX.

New module: `src/tui/declarative/` with `mod.rs` (profile), `schema.rs` (serde structs), `validate.rs`,
`template.rs` (argv rendering), `select.rs` (JSON selector engine), `output.rs` (three parser kinds),
`screen.rs` (TUI tables), `resolve.rs` (binary resolution), `fingerprint.rs`, `loader.rs`, `sources.rs`,
`approvals.rs`, `fixtures/`, `tests/`. Abbreviated below as `decl/`.

- [ ] P2-1. Schema structs + strict parse — `decl/schema.rs`, `decl/mod.rs` (stub), `src/tui/mod.rs`
  (`pub mod declarative;`), `Cargo.toml` (`toml`, `sha2`, `globset`/`glob`), `decl/tests/schema.rs` — done-when:
  the SPEC 7.1 tables parse with `deny_unknown_fields`; the two SPEC examples parse; every unknown field,
  wrong type and bad `schema_version` has a test asserting its error code and path. Depends on: P1.
- [ ] P2-2. Validation — `decl/validate.rs`, `decl/tests/validate.rs` — done-when: each `AGENT_DEF_*` code of
  SPEC 7.4 is produced by at least one test (id pattern, reserved ids, regex compile + size limit, regex
  count, placeholders and `{task}` count, `when` set, bare binary, interpreter denylist, env denylist,
  size limits, read-only note, resumable consistency). No I/O in this module (VAL-8).
- [ ] P2-3. Argv templating — `decl/template.rs`, `decl/tests/template.rs` — done-when: a function
  `render(def, HeadlessLaunch) -> Vec<String>`; placeholders per element, `when` segments in declaration
  order, `task_separator`/`task_flag` placement; property test: a task containing spaces, quotes, `;`, `&&`,
  newline, a leading `-` and 100 KiB of text stays exactly one argv element and never changes the other
  elements.
- [ ] P2-4. JSON selector engine — `decl/select.rs`, `decl/tests/select.rs` — done-when: `where`,
  `where_regex`, `~exists`, dotted paths with indices, `pick` first/last/join/max_by, `regex` capture,
  `trim`; garbage input and missing paths return `None`; fuzz-ish table of 30 cases including deeply nested
  JSON (depth cap 64) and a 1 MiB single line.
- [ ] P2-5. Output parsers (`plain_text`, `json_single`, `json_lines`) wired to
  `extract_result/extract_provider_session/extract_model/headless_result_ready/rejected_call_warning` and the
  error selector — `decl/output.rs`, `decl/tests/output.rs` — done-when: exit code != 0 returns `None`;
  `max_output_bytes` enforced; ANSI stripped; `unparseable_lines` counted.
- [ ] P2-6. Binary resolver — `decl/resolve.rs`, `src/tui/mod.rs` (new `resolve_binary` next to
  `executable`, existing fns untouched), `decl/tests/resolve.rs` — done-when: PATH scan excludes cwd and the
  project dir; Windows `.exe` > shim-glob > refuse (`AGENT_DEF_SHIM_NEEDS_NATIVE`); unit tests use a temp
  dir tree that mimics `%APPDATA%\npm\node_modules\@kilocode\cli\node_modules\@kilocode\cli-windows-x64\bin\kilo.exe`.
  Live check on this machine finds the real `kilo.exe`.
- [ ] P2-7. TUI table engine — `decl/screen.rs`, `decl/tests/screen.rs` — done-when: `is_chrome_line`,
  `is_tool_activity_line`, `looks_like_menu`, `strip_dialogs`, `tidy_reply`, `busy_marker`,
  `classify_screen`, `detect_footer_model`, `detect_prompt/prompt_reply` (with `broad`/`allow_broad`
  semantics identical to `generic::prompt_reply`), `standby_screen`, `clear_input_sequence`; screen text cap
  256 KiB; idle windows clamped per TUI-4.
- [ ] P2-8. `DeclarativeProfile` implementing `TuiProfile` — `decl/mod.rs`, `src/tui/types.rs` (only if a
  field is missing), `decl/tests/profile.rs` — done-when: every trait method maps to a table or to the
  default; `features()`, `run_options()`, `accepts_headless_model`, `standby_support`, `capability_notes`,
  `is_installed()` (resolver) behave per SPEC; `headless_launch(id, ..)` shared policy applies.
- [ ] P2-9. Conformance over definitions — `src/tui/conformance.rs`, `src/tui/declarative/tests/conformance.rs`,
  `src/tui/declarative/fixtures/definitions/*.toml` (test definitions: kilo, minimal-tui, generic) — done-when:
  every rule in `conformance.rs` iterates registry code profiles AND every test definition (including
  "extract_result rejects garbage and failed exits", "prompts round-trip", "empty screen never blocked",
  idle bounds); a deliberately broken definition in `.../fixtures/definitions-bad/` fails each rule it should.
- [ ] P2-10. Loader, sources, fingerprint, approvals — `decl/loader.rs`, `decl/sources.rs`,
  `decl/fingerprint.rs`, `decl/approvals.rs`, `src/app_paths.rs` (helper `agents_dir()`),
  `decl/tests/loader.rs` — done-when: scanning follows SPEC SRC-1..SRC-7 (non-recursive, 64 files, symlink
  guard); precedence and shadowing per REG-7/8; fingerprint is stable under whitespace/comment changes and
  changes with argv/binary/env; approvals persist in `agent-approvals.json` with the same
  load/save/bad-file-means-empty behavior as `standby/config.rs`; stale/unapproved states block
  registration with the right codes; tests use temp dirs only.
- [ ] P2-11. Registry integration — `src/tui/registry.rs`, `decl/loader.rs`, `src/lib.rs` (init call),
  `src/bridge/routes/mcp.rs` or `src/agent_runs/routes.rs` (route table), `src/bridge/tests.rs` — done-when:
  startup loads sources into the registry; routes `GET /agents/definitions`, `POST .../reload`, `.../validate`,
  `.../approve`, `.../revoke`, `.../consent` exist per SPEC 7.5; `GET /agents/types` now includes
  declarative entries; approval requires the app session (APR-7, test that an MCP session gets 403).
- [ ] P2-12. Run integration: record `agent_definition` in run snapshots and receipts, enforce fingerprint
  and pinned binary at spawn — `src/agent_runs/` (the snapshot struct + `persist.rs`), `src/tui/registry.rs`,
  `src/agent_runs/completion/` only where `extract_result` failures map to `AGENT_OUTPUT_UNPARSEABLE` /
  `AGENT_REPORTED_ERROR`, `src/bridge/errors.rs` — done-when: OBS-2 holds; `AGENT_DEF_APPROVAL_STALE` and
  `AGENT_DEF_BINARY_CHANGED` are returned by a run started after an edit; a run in flight is unaffected by
  an edit (test with two snapshots).
- [ ] P2-13. Settings > Agents page — `packages/app/src/components/settings/AgentsSettings.tsx` (new),
  `packages/app/src/components/settings/` index/route wiring (grep `Performance` settings tab added in
  `1b52915` for the pattern), `packages/app/src/lib/agentDefinitions.ts` (new), `src/commands/agents.rs`,
  component tests — done-when: lists definitions with source badge and state; shows errors with code/file/
  line; Reload button; source toggles; extra dir add/remove; consent toggles; Approve dialog shows rendered
  argv (APR-3) and warnings; no HTML injection (UI-4 test with a `<img onerror>` label).
- [ ] P2-14. Kilo Code definition + fixtures — `packages/app/src-tauri/tests/agent-definitions/kilo.toml`
  (identical to `docs/specs/.../AGENT-CHOICE.md` draft), `src/tui/declarative/fixtures/kilo/` (`headless_success.jsonl`,
  `headless_tool_turn.jsonl`, `headless_error.jsonl`, `headless_resume.jsonl`, `headless_garbage.txt`,
  `tui_idle.txt`, `tui_working.txt`, `tui_permission.txt`, `tui_reply.txt`), `decl/tests/kilo.rs` — done-when:
  headless fixtures are REAL captures from `kilo` 7.0.50 (saved from the commands in `AGENT-CHOICE.md`; the
  `sessionID` and path values scrubbed); TUI fixtures are real PTY captures through the app's recorder or
  `read_terminal_buffer`; tests assert result, session id, done marker, error message, resume argv; TUI
  tests assert chrome/busy/prompt tables on the captures. If the visible TUI behaves differently from the
  draft, the regexes change HERE and SPEC 7.2 is updated to match.
- [ ] P2-15. Live proof, headless — no source files (script run only; a scripted scenario may be added to
  `scripts/pm-regression/` as `kilo.mjs` — then that dir is the allow-list) — done-when: with `kilo.toml` in
  `<app data>/agents/` and approved, `run_agent(agent_type="kilo", model="kilo/kilo-auto/free", ...)`
  returns the expected text; `followup_task` on the handle gets an answer that depends on turn 1; both receipts
  carry `agent_definition`; killing the process mid-run yields a failed run, not a hang.
- [ ] P2-16. Live proof, visible — same allow-list as P2-15 — done-when: `headless=false` creates a Kilo pane,
  the task is delivered and verified on screen (or `verifies_prompt_delivery` is off and delivery is
  proven by the result), a result is returned flagged `result_inferred`, `followup_task` works, and a
  permission dialog is detected and answered through `answer_prompt` with `STALE_PROMPT` not triggered.
- [ ] P2-17. Docs — `docs/REPO_MAP.md`, `src/tui/README.md` (new "Adding an agent without code" section),
  `docs/specs/agent-definitions/DEFINITION-FORMAT.md` (diff against reality) — done-when: they describe the
  implemented code.
- [ ] P2-18. Phase gate — dist exe rebuilt; pm-regression 28/28; Kilo live proof repeated on the fresh
  `build_id`.

Test plan: unit per module (schema, validate, template, select, output, resolve, screen, loader,
approvals), fixture replay, conformance over definitions, bridge route tests, component tests, live headless
+ visible proof, pm-regression.

---

## Phase 3: conformance expansion, fixture expectations, parity (not porting)

Unlocks: confidence that the declarative machinery is as strict as the code profiles, and a safe place to
add the optional hybrid. Effort: 3-4 days (+2 for the optional hybrid). Risk: parity tests can overfit.

- [ ] P3-1. Fixture expectation files: each fixture can have a sibling `*.expect.toml`
  (`result`, `session`, `done`, `error`, `chrome_lines`, `prompt_kind`) — `src/tui/declarative/tests/expect.rs`,
  `src/tui/declarative/fixtures/**/*.expect.toml`, `docs/specs/agent-definitions/DEFINITION-FORMAT.md` —
  done-when: one generic test walks every definition and its fixtures; adding a fixture needs no Rust.
- [ ] P3-2. Author check without cargo: Tauri command `agents_check_file(path, fixtures_dir?)` and a "Check with
  fixtures" button — `src/commands/agents.rs`, `packages/app/src/components/settings/AgentsSettings.tsx`,
  `src/tui/declarative/tests/expect.rs` (shared runner reused) — done-when: an author can point at a definition and
  a folder of captures and see per-fixture pass/fail.
- [ ] P3-3. Claude parity test — `src/tui/declarative/tests/parity_claude.rs`,
  `src/tui/declarative/fixtures/definitions/claude-parity.toml` (test only, never registered) — done-when:
  over the existing Claude headless fixtures/strings (those used in `src/tui/claude/headless.rs` tests) the
  declarative profile returns the same `extract_result`, `extract_provider_session`, `extract_model` and
  `headless_result_ready` as `tui::claude::Claude`; where parity is impossible (documented) the test lists
  the gap. Same for Codex `json_lines` if its tests allow.
- [ ] P3-4. Read-only live test harness — `scripts/pm-regression/readonly-check.mjs` (new) — done-when:
  for any agent declaring `read_only_headless`, a script asks it to create a file under `read_only=true`
  and fails if the file appears; documented as mandatory for bundled definitions (SEC-8).
- [ ] P3-5. (OPTIONAL) Hybrid `base = "<code profile id>"` — `decl/schema.rs`, `decl/mod.rs`,
  `decl/validate.rs`, `decl/tests/hybrid.rs`, `docs/.../DEFINITION-FORMAT.md` — done-when: only data fields
  (binary names, argv, idle windows, label/icon, regex tables when non-empty) override; all other methods
  delegate to the base; `base` pointing at a non-code id is `AGENT_DEF_RESERVED_ID`; a hybrid of `claude`
  with another binary name works in a test. Skip unless open question 5 is answered yes.
- [ ] P3-6. Phase gate — all suites green, pm-regression 28/28.

Test plan: expectation-driven fixture tests, parity tests, live read-only harness. Risks: parity gaps
reveal real behaviour differences; document rather than patch the code profiles (out of scope).

---

## Phase 4: generic zero-config mode and recorder

Unlocks: "try any CLI in 30 seconds" and a draft TOML from a real session. Effort: 5-7 days. Risk: redaction
gaps leaking secrets; draft regexes that are plausible but wrong.

- [ ] P4-1. Generic (`kind = "generic"`) profile mode — `decl/mod.rs`, `decl/schema.rs`, `decl/validate.rs`,
  `decl/tests/generic.rs` — done-when: a three-line definition (`id`, `label`, `binary.names`, `experimental =
  true`) yields a visible-only, dispatchable-only-when-experimental profile using shared heuristics; receipts
  show `result_inferred` and the experimental warning (GEN-1..3).
- [ ] P4-2. "Add as generic agent" action — `packages/app/src/components/settings/AgentsSettings.tsx`,
  `src/commands/agents.rs`, `decl/loader.rs` (writes `<app data>/agents/<id>.toml` then requires approval) —
  done-when: UI creates the file from a command name; the file is NOT auto-approved.
- [ ] P4-3. Recorder core — `src/agent_definitions_recorder/` (new: `mod.rs`, `redact.rs`, `snapshots.rs`,
  `tests.rs`), `src/lib.rs` (mod decl), `src/pty/registry/mod.rs` (tap point, read-only access to screen text
  only) — done-when: start/stop per pane; snapshots on idle/working/prompt transitions; `redact.rs` has tests for
  env values, `sk-...`, `ghp_...`, `AKIA...`, 40+ char base64/hex, home path; never runs on standby panes (REC-5).
- [ ] P4-4. Draft generator — `src/agent_definitions_recorder/draft.rs`, `.../draft_tests.rs`, fixtures under
  `src/agent_definitions_recorder/fixtures/` — done-when: given the Kilo captures from P2-14 the draft contains a
  busy regex that matches the working capture and not the idle capture, a chrome regex list, a
  prompt table, and (from a headless probe with JSON lines) a `[headless]` skeleton with `result`/`session`
  selectors that parse the probe; each candidate carries a `# from: <sample line>` comment (REC-2); drafts
  are written to `<app data>/agents/drafts/` (REC-4).
- [ ] P4-5. Recorder UI — `packages/app/src/components/` (pane toolbar button; find with grep
  `kill_pane_process` UI), `packages/app/src/components/settings/AgentsSettings.tsx` (drafts list: open
  folder, "Validate draft"), `src/commands/agents.rs` — done-when: record, stop, see draft path, validate it
  through `/agents/definitions/validate`.
- [ ] P4-6. Phase gate — a recorded Kilo session produces a draft that validates and, after the human edits
  at most two lines, passes the Kilo fixtures; pm-regression 28/28.

Test plan: redaction table tests, draft generation against real captures, UI tests, live record of a Kilo pane.

---

## Phase 5: hot reload, approval UX polish, project/extra consent, MCP host writer, sharing

Unlocks: the full trust model for shared and project-local definitions, and live iteration. Effort: 6-9 days.
Risk: file-watcher flakiness on Windows (editors writing temp files, sync tools), consent UX that users click
through.

- [ ] P5-1. Watcher + debounce + atomic swap — `decl/watch.rs`, `decl/loader.rs`, `src/tui/registry.rs`,
  `Cargo.toml` (`notify`), `decl/tests/watch.rs` — done-when: save a file, registry updates within 1 s
  (UI-5); a half-written file keeps the previous snapshot; editors' temp/rename saves handled; the watcher
  never reads outside enabled source dirs.
- [ ] P5-2. `agent_definitions_changed` event + standby invalidation — `src/bridge/sse.rs`,
  `src/standby/pool.rs`, `src/standby/tests.rs`, `packages/app/src/lib/agentRegistry.ts` — done-when: SSE and
  Tauri event carry the diff; a changed fingerprint discards that agent's standby workers (STB-3/4, tested
  with the fake host); the UI refreshes without polling.
- [ ] P5-3. Project-local + extra-dir consent flows — `decl/sources.rs`, `decl/approvals.rs`,
  `src/bridge/routes/` (consent route from P2-11), `packages/app/src/components/settings/AgentsSettings.tsx`,
  an inline consent banner component, tests — done-when: opening a project containing
  `.puppet-master/agents/` shows a banner (not a modal) with the file list; nothing loads until consent +
  approval; consent is per canonical path; revoking consent unregisters; a cloned repo at a new path asks
  again.
- [ ] P5-4. Approval UX hardening — `AgentsSettings.tsx`, `decl/approvals.rs`,
  `decl/fingerprint.rs` — done-when: diff view between the approved and current argv on `approval_stale`;
  warnings list (`--auto`, `--dangerously`, `--yolo`, `--force`, env keys, interpreter); approval records the
  resolved binary path; a binary swap triggers `AGENT_DEF_BINARY_CHANGED` and a re-approve prompt.
- [ ] P5-5. Declarative MCP host writer — `decl/schema.rs` (`[mcp_host]` already reserved), `src/mcp_install/declarative.rs`
  (new), `src/mcp_install/mod.rs`, `src/mcp_install/tests.rs`, `decl/validate.rs` — done-when: JSON/TOML/YAML
  writers, key-path set, `.bak`, refuse paths outside home/project (HST-3), idempotent on re-run; a test
  definition for a fake host writes the right entry into a temp file; existing `claude/codex/opencode`
  writers untouched.
- [ ] P5-6. Sharing: export/import one definition — `src/commands/agents.rs`,
  `packages/app/src/components/settings/AgentsSettings.tsx`, `decl/loader.rs` — done-when: Export copies the
  TOML (and its fixtures folder as a zip) to a user-chosen file; Import validates and places it in the user dir
  UNAPPROVED; there is no network fetch.
- [ ] P5-7. Docs — `docs/REPO_MAP.md`, `src/tui/README.md`, `DEFINITION-FORMAT.md` — done-when: reflect the
  implemented watcher, consent and writer.
- [ ] P5-8. Phase gate — pm-regression 28/28; live edit of `kilo.toml` while a Kilo run is in flight leaves that
  run unaffected and the next run uses the new argv only after re-approval.

Test plan: watcher tests on temp dirs (Windows file-lock behaviour included), consent matrix tests,
writer tests on temp files, live edit-during-run.

---

## Final verification task list

- [ ] V-1. Suites: `cargo test --lib` (all), `cargo test --lib tui::` (conformance over code + definitions),
  `cargo test --lib tool_registry`, `cargo test --lib standby`, `npm test` / `npm run typecheck` in `packages/shared`,
  `packages/app`, `packages/mcp-server` — all green; record counts.
- [ ] V-2. Dist MCP exe rebuilt with a separate `CARGO_TARGET_DIR` and renamed into place; app restarted; note the
  new `build_id` from `/health`.
- [ ] V-3. SCHEMA_DRIFT check: `/health.catalog_version` equals the shim's `catalog_version()`; `bridge_health`
  tool shows no drift; run `list_agents` through the shim.
- [ ] V-4. `node scripts/pm-regression.mjs` against the new build: 28/28 (both backends as before).
- [ ] V-5. Live Kilo proof from a clean state: delete `agent-approvals.json`, place `kilo.toml`, observe
  `needs_approval`, approve, then: headless `run_agent` + `followup_task` (same `sessionID`, context carried), visible
  `run_agent` + `followup_task`, unknown id -> `UNKNOWN_AGENT_TYPE` with valid list, edit one argv token ->
  `AGENT_DEF_APPROVAL_STALE`.
- [ ] V-6. Security spot checks: project-local source OFF by default; a hostile `.puppet-master/agents/evil.toml`
  in a test project does not load; `python`/`cmd` as a binary is rejected; a task of `-x; rm -rf /` arrives as one
  argv element; MCP session cannot hit `/agents/definitions/approve`.
- [ ] V-7. Docs: `docs/REPO_MAP.md` (the "Add an agent / TUI profile" row, `tui/` row, registry row, new routes),
  `src/tui/README.md` ("Adding an agent without code"), `DEFINITION-FORMAT.md` verified against the schema by
  running its examples through `/agents/definitions/validate`; `docs/specs/agent-definitions/SPEC.md` assumptions
  updated with what was verified.
- [ ] V-8. Memory: update `MEMORY.md` and a new memory note with the registry location, the definitions dir, the Kilo
  Windows `.exe` resolution gotcha, and the `catalog_version` pin policy.
- [ ] V-9. Commit with a fresh `build_id` recorded in the message body; no unrelated working-tree changes staged.

## Effort summary

| Phase | Days (one engineer) | Main risk |
|---|---|---|
| P0 groundwork | 2-3 | `catalog_version` bump and SCHEMA_DRIFT during rollout |
| P1 registry + AgentId | 4-6 | wide compile surface, hot-path cost, persisted-run compatibility |
| P2 declarative core + Settings + Kilo proof | 8-12 | visible-TUI regexes for Kilo; Windows binary resolution; approval UX |
| P3 conformance/fixtures/parity (+optional hybrid) | 3-4 (+2) | parity tests overfitting |
| P4 generic mode + recorder | 5-7 | redaction gaps; plausible-but-wrong drafts |
| P5 hot reload, consent, MCP host writer, sharing | 6-9 | Windows watcher behaviour; consent fatigue |
| Final verification | 1-2 | live-test flakiness (known intermittents in memory notes) |
| Total | 29-43 | |

Minimum path to the stated "done when" (Kilo through one TOML, headless + visible, 28/28): P0 + P1 + P2 +
V-tasks, about 15-24 days. P3-P5 are the hardening and trust phases the user chose to include.
