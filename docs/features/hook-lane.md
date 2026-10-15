# Live updates from agents (Hook Lane v0, watch only)

Design: `D:\pm-night\DESIGN-HOOK-LANE.md`. Probes: `docs/specs/hook-lane/PROBES.md`.

## What and why
Claude Code and Codex can run a command at fixed moments: a session starts, the user sends a prompt,
a tool asks for permission, a file is edited, a turn stops. Puppet Master registers one fixed command,
`puppet-master-mcp hook <agent>`, for those moments. The command forwards the event to the app over
loopback with a per-pane token and exits 0 with no output, whatever happens.

v0 is **watch only** ("shadow"): events are recorded next to what the screen reader decided, and the
two are compared. Nothing acts on them: no typing, answering, approving or unlocking. The comparison
is the data that decides whether hook events may ever be used as evidence (v1, founder's call).

Personas: A (developers) get a record of where the screen reader and the agent disagree; B/C see one
plain doctor line ("Live updates: Connected ...") and, for Codex, the one-time step.

Agent-agnostic: every per-agent fact is in a `[hooks]` TOML table (`tui/claude/hooks.toml`,
`tui/codex/hooks.toml`, or a declarative definition's `[hooks]`). `agent_hooks/` never names an agent.

## Design
| Part | Where |
|---|---|
| `[hooks]` schema + `AGENT_DEF_HOOKS_*` validator codes | `tui/declarative/hooks.rs`, `TuiProfile::hook_scheme` |
| Emitter (`hook` subcommand) | `mcp_shim/hook.rs` (3 lines in `bin/puppet-master-mcp.rs`) |
| Token, projection, ledger, shadow recorder, doctor row | `agent_hooks/{token,ingest,ledger,shadow,health}.rs` |
| `POST /hooks/event`, scope guard | `agent_hooks/route.rs`; one call each in `bridge/router.rs` and `bridge/http.rs::gate` |
| Claude: `--settings <hooks dir>/claude-settings.v1.json` | `agent_hooks/install/launch.rs` |
| Codex: managed entries in `.codex/hooks.json` | `agent_hooks/install/project.rs`, `git_exclude::exclude_file` |
| Frozen hook exe path, recorded once | `agent_hooks/install/stable_exe.rs` |
| Setting `agent_hooks_mode` (off / shadow / evidence; default shadow) plus per-agent `agent_hooks_agents`; an agent may declare `default_mode = "off"` (Codex: opt-in, it writes a project file) | `settings_store.rs`, `agent_hooks::apply_settings` |
| Settings > Agents > Live updates; doctor line | `AgentsSection.tsx`, `AgentDoctor.tsx` |

No new crates (`sha2`, `rand`, `base64`, `subtle` were already dependencies).

## Safety rules and where they are enforced
- Hooks never type/answer/approve/unlock: `agent_hooks` has no path to those modules
  (`the_hook_lane_cannot_reach_anything_that_types_answers_or_unlocks`).
- Fail open: emitter always exits 0, silent, 300 ms watchdog (`tests/hook_emitter.rs`).
- No `.claude` settings file is ever written (`enabling_hooks_for_every_profile_writes_no_file_any_other_agent_reads`,
  validator `AGENT_DEF_HOOKS_SHARED_FILE`).
- Never a git-tracked file (`a_git_tracked_hooks_file_is_never_written`).
- Hook token: 403 `HOOK_TOKEN_SCOPE` on every other route (`the_hook_token_gets_403_on_every_other_route_through_the_real_gate`).
- Paired phone refused (`a_paired_phone_is_refused_on_the_hook_route_through_the_real_gate`).

## Limits
See `D:\pm-night\p0\hook-lane\REPORT.md` ("What is left").
