# Agent definitions: implementation report (T01–T37)

Branch `refactor/tui-profiles`, 53 commits on top of T02 (`7e56b1a`), none pushed. All phases done except
T29 (optional, blocked on SPEC Q5). Phase work was done by subagents, one phase at a time in the main tree,
each with a tests-first prompt; per-phase evidence files (commands, counts, build ids) were kept in the
session scratchpad `evidence/` folder and summarised here.

## Result in one paragraph

You can now add a CLI agent to Puppet Master by dropping one TOML file in `%APPDATA%\com.puppetmaster.app\agents\`
and approving it in **Settings > Agents**. Kilo is the proof: `kilo.toml` runs headless (`run_agent` +
`followup_task`, read-only via `--agent ask`) and visible (pane, follow-up, permission prompts answered),
with receipts that name the exact definition, fingerprint and pinned `kilo.exe`. Definitions hot-reload,
edits after approval block new runs but never break a run in flight, project/extra folders need per-path
consent, and approve/consent are only accepted from the local desktop app.

## Final verification (independently re-run by the coordinator at the end)

| Check | Result |
|---|---|
| `cargo test --lib` | **893 passed, 0 failed**, 2 ignored |
| app vitest | **33 files, 227 passed** |
| `cargo test` (all targets) | lib + MCP bin 29 tests, all ok (T37) |
| typecheck shared/app/cli/mcp | clean (T37) |
| `pm-regression` | **28/28** (T37, build `d6e8d37-dirty@1791508684`; also 28/28 at T12, T26, T32, T36, T36b) |
| MCP schema | dist exe rebuilt; fresh shim `catalog_version 66-4cd8e0886d0f544a` = bridge, `schema_drift:false` |
| Windows | `scripts/check-window-monitor.ps1` → "OK: no Puppet Master window on the primary monitor" after every visible test run, and at the end |

## Live evidence highlights

- **Kilo headless (T21, T37):** `t37-kilo-hl` read-only → "ack BANANA37", receipt `read_only: enforced`,
  `agent_definition {id: kilo, fingerprint sha256:40a784e2…, binary …\kilo.exe}`; follow-up → "BANANA37" on the
  same provider session. Kill mid-run → clean failure in ~5 s; bad model → `AGENT_REPORTED_ERROR` with Kilo's message.
- **Kilo visible (T24, T26b, T37):** pane created, task delivered, reply `clean27` with no sidebar junk,
  follow-up ok, `answer_prompt` allow/deny works (deny really denies).
- **Edit during a run (T36b):** `kilo.toml` argv edited mid-run → definition `approval_stale` within 3 s,
  the in-flight run still returned `clean27` (authoritative), next `run_agent`/`followup_task` refused
  with the specific stale-approval message; file restored byte-identical (sha256 `536451aa…` before/after) → `active`.
- **Hot reload (T33):** fingerprint change reflected in `/agents/types` in 0.27 s; SSE `agent_definitions_changed` fired.
- **Consent (T34):** hostile `evil.toml` in a project not listed until consent, then `needs_approval`, gone after revoke.
- **Security (T19b, T37):** approve/consent from an MCP session, `Authorization: Bearer`, `X-Forwarded-For`,
  `CF-Connecting-IP` → 403; `X-PM-Proxied` → 401. Task text with `; rm -rf /`, leading dashes, 100 KiB stays one argv element (unit tests).
- Your `agent-approvals.json` and `agents/kilo.toml` are byte-identical to before T37 (sha256 recorded).

## Bugs found and fixed along the way

| Found in | Bug | Fix |
|---|---|---|
| T19 review | a paired phone/browser (bearer token via the tunnel) counted as the app session → could approve a definition = run any command on this PC | `97aafe7` local-app-only gate |
| T24 | first task of a fresh Kilo pane lost (typed before Kilo's input existed) | verified delivery `[tui.delivery]` |
| T24 | "deny" on a permission prompt pressed Enter = approved | key names sent as key bytes |
| T26 | visible Kilo result was a whole-screen dump (42-col sidebar mixed in) | `dc0e03d` `[tui.layout]` sidebar crop |
| T34 | "Review and approve" never shown in Settings (UI checked a state name the backend never sends) | fixed in `94ad694` |
| T36 | editing a definition mid-run made the in-flight run fail (`agent_gone`) | `124fe2e` turn holds its profile |
| T36 | stale refusal said "this backend cannot run delegated work" | `34e6901` specific `AGENT_DEF_*` codes |
| T36 | `build_id` often unchanged between rebuilds (evidence couldn't prove which code ran) | `72fb7bd` |
| T36b | receipt said `read_only: unenforced` for a read-only run whose definition went stale | `3cfeab5` |
| T37 | unknown `agent_type` returned `AGENT_NOT_DISPATCHABLE` instead of `UNKNOWN_AGENT_TYPE` | `ea27a7f` |
| T12 | flaky test from tests swapping the global registry | `a3e3247` serialized |
| session | Puppet Master windows popped onto your primary monitor during agent tests | `eb62c3e`, `3f9db5e`, `75821a0`, `a3d2775` |

## Not done / not proven

- **T29** (hybrid `base = <code profile>`): skipped, blocked on SPEC Q5.
- **T37 clean-state Kilo proof**: not run — the permission classifier blocked deleting approvals/revoking; the
  needs_approval→approve and stale-refusal steps were instead proven live in T36/T36b plus unit tests.
- **Recorder live path** (press Rec in the real app → draft): untested live (no bridge route, computer use denied);
  unit/fixture tests pass (draft validates, 7/9 Kilo fixtures as generated, 9/9 after two small blocks).
- **Generic "Add as generic agent"** and **export/import**: unit-tested only (not run against your real agents folder).
- `[mcp_host]` writer: library only, tested on temp dirs; nothing in the app calls it yet.
- `LaunchSpec` had no env channel, so `[env] set/unset` was parsed, validated and fingerprinted but never applied. **Fixed on `feat/account-profiles`**: `LaunchSpec.env` / `env_remove` are applied by every launch path (see `docs/features/account-profiles.md`). There is still no stdin channel → stdin-task definitions are not supported (Kilo doesn't need them).
- Known flakes (not seen at the end): `startup_recovery_releases_locks_held_by_interrupted_operations` (Access denied),
  `recover_all_visits_nonselected_indexed_projects`, cursor `timeout` `WORKER_STOP_FAILED` once.

## Suggestions (in priority order)

1. **Scope project agents to their project.** They are in the one global registry and only gated at spawn, so two
   consented projects with the same id shadow each other and the spawn menu can show another project's agent.
2. **Finish the "stale profile" cleanup:** `run_warnings()` still reads the live registry (same class of bug as the receipt fix).
3. **Add env/stdin to `LaunchSpec`** — env is done (`feat/account-profiles`); stdin (long prompts) is still open.
4. **Second proof agent (SPEC Q7)** so the format isn't Kilo-shaped; also add the Q6 cost/concurrency note.
5. **Give the recorder a bridge route** (local-app only) so record→draft can be tested end to end, and wire the
   `[mcp_host]` writer to a Settings action.
6. **Spawn menu** still offers only OpenCode (API)/PowerShell presets; consider listing approved definitions.
7. Small: "experimental" badge on Settings rows; fix the three flakes; add a `test` script to `packages/mcp-server`;
   decide whether `Box::leak` interning of definition strings is acceptable for long sessions with many edits.
8. Push the branch / open a PR when you're ready (not done — nothing was pushed).

## Commits by phase

| Phase | Commits |
|---|---|
| T01–T02 | 6c33f37, 7e56b1a |
| T03 | 92657a8 |
| T04 | 06620ba |
| T05 | db0190f |
| T06–T11 | 268b8cf, 247ef4b, 5fcfcd7, d2d9d16, f68bf53, 385ef47 |
| T12 (Phase 1 gate) | 8d8f18c, a3e3247 |
| T13–T15 | 6603f90, 371d143, c9c690a, f422a2f |
| T16–T18 | fb6c3c9, 5bf753d, 9510043 |
| T19–T21 (Kilo headless ★) | 6d4643e, 97aafe7, 859263a, f7f494c |
| T22–T24 (Kilo visible ★) | ad5f761, 425ef77, 9ca5cb9, de92de2, ba42370 |
| T25–T26 (Phase 2 gate) | a55b285, 7d05d8f, 65a4233, dc0e03d |
| T27, T28, T30 (generic ★) | 0eac5f9, 659506f, cc5533f |
| T31–T32 (recorder ★) | daa37fb, f4313e7, e0e1629 |
| T33–T34 | fa03936, 94ad694 |
| T35–T36 (Phase 5 gate) | ca7c0dd, 17d650a, 124fe2e, 34e6901, 72fb7bd, afcaee6, 3cfeab5 |
| T37 | dc0a4ad, d6e8d37, ea27a7f |
| window placement | eb62c3e, 3f9db5e, 75821a0, a3d2775 |

## After T37

Commits after T37 (`git log 74fc57f..HEAD`, oldest first). Feature/test docs: `docs/FEATURES.md`, `docs/TEST-PLAN.md`.

| Commit | What |
|---|---|
| db71c99 | visible kilo delivery reads the uncropped screen (sidebar crop hid the prompt on the home screen); a dispatch failure keeps a keep_pane=true pane |
| 987513c | generic agent definitions render their visible command (binary name) in the approve preview and validate route |
| 5543216 | orphan detached pane window closes: backend honours X on a window whose pane is gone; the sweep escalates if destroy did not take |
| 1e3957f | UI: headless-only and experimental agents show in the spawn menu (disabled with reason), Overview Agents card, live registry refresh + no-restart toast, experimental badge on definition rows |
| 64cb604 | visible agent-run panes open in the main window grid by default; `placement` auto/grid/window on run_agent/delegate_work/spawn_agent |
| 4108522 | grid run panes get a wide tile; narrow sidebar-profile panes read per row so visible kilo results stay clean below 121 cols |
| 10d38df | visible prompt delivery waits for a slow-starting TUI to draw before typing; blank screens do not burn typing attempts (up to 20 s) |
| 8f36afa | one pop-out window per pane: dedupe concurrent opens per label, stop the StrictMode-leaked `pane://detach` listener |
| 773d283 | single-instance guard; bridge port file removed on exit only if it names our port; MCP client probes 17321-17330 |
| eacd325 | pane exit detected by an exit watcher, not only ConPTY EOF, so a dead pane's pop-out closes immediately |

Resolved from the earlier "What is left" list: item 6 (spawn menu lists registered agents, headless-only greyed) and the experimental badge in item 7.
