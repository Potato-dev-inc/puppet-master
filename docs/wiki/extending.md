# Extending

Where to start a change. The full table is [REPO_MAP.md § 4](../REPO_MAP.md). Symptom → one shared function, not a patch per caller.

## Add an MCP tool

1. `tool_registry.rs::tools` — `ToolDefinition` (name, description, method, path, schemas, `ToolSafety`).
2. Put the name in the right list in `tool_visible_in_mode` (agent / shell / both).
3. Implement the HTTP route (`bridge.rs::route` or `agent_runs/routes.rs`).
4. `bin/puppet-master-mcp.rs::call_tool` **only** if the shim must shape args or replies.
5. `mcp_sessions.rs::authorize` if ownership applies.
6. Mirror the name in `packages/app/src/lib/mcp-tools.ts` if the UI lists tools.
7. Rebuild the MCP exe **and** the app ([developing.md](developing.md)). `catalog_version` changes automatically; mismatch is `SCHEMA_DRIFT`.

Changing a description or schema is the same rebuild: both sides, or health shows drift.

Server instruction text: `tool_registry.rs::mcp_instructions`.

## Add an agent / TUI

Follow [tui/README.md](../../packages/app/src-tauri/src/tui/README.md) — do not invent a parallel path.

1. `AgentType` in `pty/agents.rs` + `packages/shared/src/agents.ts` + `agent_contexts.rs`.
2. `tui/<name>/` implementing `TuiProfile`. Override only what differs. Smallest example: `tui/claude/`. Fullest: `tui/cursor/`.
3. Register in `tui/mod.rs::PROFILES` (order is `read_only_backends()` order).
4. Real screen fixtures, not hand-drawn ones. `cargo test --lib tui::` runs `conformance.rs` automatically.

If you are about to write `if agent_type == "foo"` **outside** `tui/`, stop and add a trait method with a generic default instead.

`opencode_native` is not a new scraped TUI. HTTP client lives in `opencode/`; TUI-facing bits in `tui/opencode/`.

## Fix “the agent finished but the result is empty / wrong”

| Symptom | Start here |
|---------|------------|
| Headless result empty / session not resumed | `tui/<agent>/headless.rs` (`extract_result`, `extract_provider_session`) |
| Visible reply is chrome / truncated | `tui/<agent>/chrome.rs`, `completion.rs::clean_extracted_result` / `widen_reply` |
| Idle / blocked / running LED wrong | `tui/<agent>/screen.rs::classify_screen`, `pty/status.rs` |
| Completion too early or never | Visible: `observe_tui_turn`. Headless: `execute_headless` + `headless_result_ready`. Native: `agent_runs/native.rs` message order |
| Enter not sent / prompt not delivered | `agent_runs/delivery.rs`; profile `verifies_prompt_delivery`. OpenCode: `opencode/delivery.rs` |
| Approval key wrong | `tui/<agent>/screen.rs` (`detect_prompt`, `prompt_reply`), then `pane_prompt.rs` |

## Ownership, locks, close, checks

| Task | Start here |
|------|------------|
| Leases / `take_over` / attach | `mcp_sessions.rs`; plan `docs/plans/wp7-ownership-lifecycle.md` |
| Resource locks | `bridge.rs::acquire_operation_locks` |
| Pane closed after a run | `agent_runs/pane_close.rs`; plan `docs/plans/wp9-pane-close.md` |
| Acceptance checks | `agent_runs/checks.rs` + `checks` on the `run_agent` schema |
| Idempotency / handles | `operations.rs::create_operation`, `persist.rs` |
| `watch` CLI | `packages/cli/src/watch.ts` + `watch_command.rs`; redeploy the CLI bundle |
| Error codes | Grep the quoted `CODE`; glossary in [REPO_MAP.md § 7](../REPO_MAP.md) |

## UI

`packages/app/src` — components, hooks, `lib/`. Keep TS enums in sync with Rust via `packages/shared` and `lib/tauri.ts`. Frontend is only sketched in REPO_MAP; treat that package as the source.

## Docs after a behavior change

1. This wiki — if a concept or the run flow changed.
2. `docs/REPO_MAP.md` — if you added a module, tool, or invariant.
3. `docs/orchestrator/*` — if orchestrators must call tools differently.
4. Root `README.md` — only product-facing facts (derive from [wiki overview](README.md)).

Do not add a third map. Link here or to REPO_MAP.
