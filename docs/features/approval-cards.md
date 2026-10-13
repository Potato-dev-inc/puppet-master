# Plain-language approval cards with risk

Branch `feat/approval-cards`. Feature 1 of 2 (live preview is `live-preview.md`).

## What and why
About 93% of agent permission prompts get accepted, which pushes people toward bypass mode, and vibecoders (persona B) and non-technical users (C) cannot judge `rm -rf ../old-site`. Every approval now says in plain words what will happen and how risky it is, for every agent.

Agent-agnostic: `prompt_risk` reads only the command text, the prompt kind, the cwd and the pane's project folder. It never looks at the agent name. Per-agent additions come from a `[risk]` table in the TOML definition, through `TuiProfile::risk_rules()`.

## Design
- `src-tauri/src/prompt_risk/` (`mod.rs` types and aggregation, `command.rs` segmenting and words, `paths.rs` lexical workspace test, `rules.rs` the rule set, `tests.rs`).
  - `assess(&RiskInput, &[RiskRule]) -> Risk { level: low|medium|high, headline, reasons }`.
  - Commands are split on unquoted `;`, `&&`, `||`, `|`, `&`, newlines (a here-doc keeps only its first line); `bash -c`, `cmd /c`, `powershell -Command`, `sudo` are unwrapped and analysed recursively (depth 3). POSIX words use `shlex` (MIT OR Apache-2.0, added to `Cargo.toml`); PowerShell and cmd lines use a backslash-preserving splitter because `shlex` would turn `C:\Users` into `C:Users`.
  - Rules: recursive delete (`rm -rf`, `rmdir /s`, `Remove-Item -Recurse`, `del /s`, `find -delete`), paths outside the workspace (lexical, case-insensitive on Windows, `/c/...` Git Bash spelling, `~`, `..`, temp/appdata variables), history rewrite or force (`push --force`, `reset --hard`, `clean -fdx`, `filter-branch`), network piped to a shell (`curl|sh`, `iwr|iex`, `bash <(curl ..)`), encoded PowerShell, global installs, publish, elevation (`sudo`, `runas`, `-Verb RunAs`), system settings, secrets (`.env`, `id_rsa`, credentials files: read = medium, write or send = high), writes outside the project (including `>` redirects).
  - Low: tests, builds, lint, read-only commands, git reads and commits. Unknown commands: medium, "Run a command". Deleting well-known rebuildable folders inside the project (`node_modules`, `dist`, `target` ...) is low.
  - A call without a workspace treats relative paths as inside and absolute ones as "could not be checked" (medium, with that wording); the prompt's own `cd` dir stands in for a workspace.
- Exposure: `risk` is attached in `pane_prompt::detect_in_screen` (no workspace) and recomputed in `pane_prompt::detect` (the pane's `cwd` is the workspace). So `PanePrompt` (`pane_prompt` command, `pty://prompt`), `required_action.risk`/`raw_prompt.risk` on the bridge, and `pane_wait` prompts all carry it. `PanePrompt.risk` is optional and camelCase; `PromptChoice` gained `broad` (backend `choice_is_broad`).
- TOML: optional `[[risk.patterns]]` with `match` (regex, case-insensitive), `level`, `headline` (1-60 chars), `reason` (<= 200). A definition can only raise what built-in rules found; it can set a level freely only for a command no built-in rule recognised. Validated at load (`AGENT_DEF_BAD_REGEX` at `risk.patterns.N.match`). Not in the approval fingerprint, because it can only raise.
- UI (monochrome, no colour): `components/cockpit/ApprovalPrompt.tsx`, `lib/approval-risk.ts`, `styles/approval-card.css`. Glyph per level (ring, triangle, filled stop), weight 500/600/700, level word ("Low risk", "Check first", "High risk"), headline plus one reason, "Show details" for the full command and all reasons. High risk: caution line, left rule, the "always allow / allow all / do not ask again" family behind "More options" (still reachable), Deny is the primary button and takes keyboard focus (never Allow). Used by the Inbox card and the pane prompt bar; prompts without `risk` render the old UI. `cockpit-api.onPrompt` re-reads the prompt through the command so the event's workspace-less risk is replaced by the workspace-aware one.

## Evidence
Run in `D:\pm-approval-cards`, 2026-10-14.
- `cargo test --lib` (`CARGO_BUILD_JOBS=3`): `1051 passed; 1 failed; 2 ignored`, the failure is the known `bridge::tests::definitions::approve_requires_app_session`.
- New Rust tests (`prompt_risk::tests`, 17 incl. wiring): `command_samples_get_the_expected_level_and_headline` (73 samples: 44 bash, 16 PowerShell, 13 cmd, each with expected level and exact headline), `every_result_explains_itself_in_plain_words`, `workspace_boundary_is_judged_lexically` (prefix siblings, `..`, case, slashes, Git Bash drive, `cd` carry), `without_a_workspace_...`, `prompt_cwd_stands_in_for_a_missing_workspace`, `chained_commands_report_the_worst_part_first`, `quoted_separators_do_not_split_a_command`, `encoded_powershell_is_high`, `definition_patterns_can_raise_but_never_lower`, `a_toml_risk_table_extends_the_built_in_rules_for_that_agent_only`, `a_bad_risk_pattern_or_headline_is_rejected_at_load`, `a_detected_prompt_carries_its_risk_everywhere_it_is_returned` (real Cursor fixture; UI shape and bridge `required_action` shape), `a_prompt_without_risk_still_serialises_for_old_uis`, `allow_all_is_flagged_broad_for_the_ui`.
- `npm test --workspace=@puppet-master/app`: `Test Files 50 passed (50)`, `Tests 396 passed (396)` (new: `ApprovalPrompt.test.tsx`, 10 tests: layout rules, high-risk demotion, focus on Deny not Allow, More options still answers, details, no focus steal, no inline colours).
- `npx tsc --noEmit -p packages/app`: clean.
- Fails before, passes after (mutation): with `layoutChoices` not treating `high` as high, `ApprovalPrompt.test.tsx` -> `Tests 3 failed | 7 passed`; with the headline "Delete a folder outside this project" renamed in `rules.rs`, `command_samples_get_the_expected_level_and_headline` -> `11 of 73 samples differ`. Both restored.
- Screenshots (harness incl. screenshot-script fix 7651d54): `D:\pm-night\evidence\approval-cards\approval-low.png` (Run tests, Low risk, Yes primary) and `approval-high.png` (Delete a folder outside this project, High risk, caution, details open, Deny focused and primary, Allow once secondary, More options). Both viewed. Mock phrases/prompts are fixtures (`HIGH_RISK_PROMPT`).

## Limits and follow-ups
- Risk is a heuristic over text: a script (`./deploy.sh`, `npm run x`) is judged by its name, not its contents; unknown means medium.
- The reader thread's `pty://prompt` event has no workspace; the UI corrects it by re-reading via `pane_prompt`. Passing the pane cwd into `PanePrompt::from_detected` at the call in `pty/registry/spawn.rs` would make that unnecessary (not done: file off limits).
- Enter typed into a focused terminal still goes to the agent's own dialog; the card can only control its own buttons.
- Phone/PWA does not render risk yet (the field is in the bridge JSON). Not exercised with real agents.
