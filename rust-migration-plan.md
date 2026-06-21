# Puppet Master Rust Migration Plan

## Current architecture read

Puppet Master already has the right high-level split:

- Rust owns durable process/control-plane work: PTYs, the embedded HTTP bridge, event log, task and lock projections, context packs, settings, mobile pairing, tunnel support, and SSE.
- React and TypeScript own the user-facing app: workspace UI, terminal rendering, mobile PWA, sidebar chat, API orchestrator loop, and client wrappers.
- External MCP currently runs through a TypeScript stdio wrapper that proxies calls to the Rust HTTP bridge.

The README promises one bridge and one tool surface. The bridge is mostly there, but the tool surface is still duplicated:

- External MCP tools are manually defined in `packages/mcp-server/src/index.ts`.
- Sidebar tools are separately defined in `packages/app/src/lib/mcp-tools.ts`.
- The Rust bridge already exposes more routes than external MCP surfaces, including tasks, locks, audit, inbox, replay, workspace state, and context packs.

This duplication is the main architecture issue to fix before adding many more MCP tools.

## Language target reality

There are two separate goals:

1. Lower the displayed JavaScript percentage.
2. Move meaningful control-plane source code into Rust.

The displayed JavaScript percentage is likely inflated by a generated checked-in bundle:

```text
packages/app/src-tauri/resources/mcp-stdio.bundle.cjs
```

That file is roughly 18.7k lines and should not count as hand-written project source.

Source-only rough count, excluding generated `dist` and Rust `target`:

- Rust: about 6.7k LOC
- TS/TSX/JS/MJS: about 18.1k LOC
- Rust source share: about 27%

To raise real Rust source share above 40%, either:

- migrate roughly 3.2k lines of durable TypeScript logic into Rust while keeping total source size similar, or
- add equivalent Rust core modules while deleting/reducing duplicate TypeScript control-plane code.

Do not rewrite React/xterm UI in Rust just to chase a percentage. Keep UI in TypeScript where it is productive. Move protocol, orchestration state, MCP, and session memory into Rust.

## Goals

- Make Rust the source of truth for MCP tools, bridge routes, error shapes, session context, pane roles, and orchestration state.
- Keep React/TypeScript focused on UI rendering, xterm integration, and thin bridge clients.
- Give users and orchestrators a more predictable model experience: fewer duplicate panes, less buffer scraping, clearer handoffs, better prompt handling, and consistent API/CLI/MCP behavior.
- Support adding many more MCP tools without copy-pasting schemas and descriptions across app, MCP package, and docs.
- Reduce JavaScript percentage honestly by removing generated bundles from language accounting and retiring redundant Node control-plane code.

## Phase 0: Repository language hygiene

Mark generated artifacts as generated or stop tracking them.

Recommended changes:

- Add `.gitattributes` entries:

```gitattributes
packages/app/src-tauri/resources/mcp-stdio.bundle.cjs linguist-generated=true
packages/app/src-tauri/resources/pwa-dist/** linguist-generated=true
packages/app/dist/** linguist-generated=true
packages/app/dev-dist/** linguist-generated=true
packages/**/dist/** linguist-generated=true
```

- Prefer building `mcp-stdio.bundle.cjs` during `npm run build` instead of committing it.
- Keep release artifacts out of source language stats unless there is a packaging reason they must be tracked.

Expected result:

- Large immediate drop in GitHub Linguist JavaScript percentage.
- No runtime behavior change.
- Cleaner baseline before measuring real migration progress.

## Phase 1: Rust tool registry

Create a Rust `tool_registry` module that becomes the source of truth for all Puppet Master tools.

It should define:

- tool name
- description
- input JSON schema
- output schema where practical
- read-only/destructive/open-world annotations
- bridge route mapping
- sidebar visibility
- external MCP visibility
- expected orchestrator usage hints

Expose it through bridge routes:

```text
GET /mcp/tools
GET /mcp/resources
GET /mcp/prompts
```

Then replace duplicated TypeScript tool lists with generated/fetched specs:

- `packages/mcp-server/src/index.ts` should no longer hardcode the tool list.
- `packages/app/src/lib/mcp-tools.ts` should consume the same registry for sidebar tool definitions.
- README/MCP docs can be generated or checked against the registry.

User impact:

- API sidebar, CLI orchestrators, and external MCP hosts see the same tool surface.
- New MCP tools can be added once in Rust.
- Fewer mismatches like sidebar-only `press_key`.

## Phase 2: Rust MCP stdio server

Rewrite the stdio MCP wrapper in Rust.

Target shape:

- A Rust binary, for example `puppet-master-mcp`.
- It reads the bridge port file.
- It speaks MCP over stdio.
- It forwards tool calls to the Rust bridge.
- It uses the Rust tool registry for `tools/list`.
- It returns typed, consistent errors.
- The npm package becomes a thin installer/shim for the Rust binary.

Error shape examples:

```json
{ "code": "bridge_down", "message": "Puppet Master desktop bridge is not running" }
{ "code": "pane_not_found", "message": "Unknown pane: codex-123" }
{ "code": "lock_conflict", "message": "Resource is already locked", "resource_id": "file:src/App.tsx" }
{ "code": "timeout", "message": "Timed out waiting for pane to become idle" }
```

Delete or shrink:

- `packages/mcp-server/src/index.ts`
- generated `mcp-stdio.bundle.cjs`
- bundle scripts that only exist for the TypeScript MCP wrapper

User impact:

- Faster startup.
- Fewer Node/runtime dependency issues.
- Better cross-platform production story.
- Big reduction in real and displayed JavaScript.

## Phase 3: Session context and pane digests

Implement the ROADMAP 0.3 session context layer in Rust.

Add bridge/MCP tools:

```text
read_session_context
update_session_context
read_pane_digest
set_pane_role
```

Persist through the existing project event log and projections:

- project summary
- current user goal
- pane roles
- task ownership
- decisions
- handoff notes
- recent tool activity
- compressed pane digests
- pointers to relevant files/git state when available

Pane digests should summarize terminal output without requiring orchestrators to repeatedly call `read_terminal_buffer`.

User impact:

- Orchestrators stop wasting tokens re-reading panes.
- Users get clearer continuity across sidebar, CLI orchestrators, mobile, and external MCP clients.
- Multi-agent handoffs become easier to understand and recover.

## Phase 4: Structured delegation

Add `delegate_task` as a first-class Rust tool instead of making every orchestrator free-text a prompt into a worker pane.

Suggested input:

```json
{
  "task_id": "task_...",
  "target_pane_id": "codex-...",
  "intent": "Implement Rust MCP tool registry",
  "acceptance_criteria": [
    "Tool list served from Rust",
    "Sidebar and external MCP use the same registry",
    "Tests cover schema output"
  ],
  "locked_resources": [
    "directory:packages/app/src-tauri/src",
    "file:packages/mcp-server/src/index.ts"
  ],
  "evidence_required": [
    "Tests run",
    "Files changed",
    "Known gaps"
  ],
  "token_budget_hint": 8000,
  "timeout_ms": 600000
}
```

The bridge should render agent-specific prompts using Rust agent adapters:

- Claude Code prompt style
- Codex CLI prompt style
- OpenCode prompt style
- shell/powershell command-oriented style

User impact:

- Workers receive consistent instructions.
- Acceptance criteria and evidence become explicit.
- The app can display who owns what without parsing terminal text.

## Phase 5: Move orchestration runtime state into Rust

Move the durable parts of `packages/app/src/lib/puppet-master.ts` into Rust.

Rust should own:

- tracked worker panes
- standby polling
- wake reasons
- prompt detection
- routine permission auto-approval policy
- pane status transitions
- run metadata
- tool activity timeline
- timeout handling

TypeScript can still own:

- LLM API streaming
- chat UI rendering
- user cancellation controls
- bridge client calls

Bridge endpoints can look like:

```text
POST /orchestrator/runs
GET  /orchestrator/runs/:id
POST /orchestrator/runs/:id/events
POST /orchestrator/runs/:id/cancel
```

User impact:

- Sidebar and CLI-backed orchestrators follow the same standby behavior.
- Mobile can observe orchestration state directly.
- The app can recover from UI reloads without losing run state.

## Phase 6: Enhanced MCP resources and prompts

Expose MCP resources backed by Rust projections:

```text
puppet-master://session
puppet-master://rules
puppet-master://panes
puppet-master://panes/{id}/digest
puppet-master://tasks
puppet-master://locks
puppet-master://audit
```

Expose MCP prompts:

```text
status_check
delegate_refactor
implement_with_review
fix_ci
handoff_to_worker
summarize_session
```

User impact:

- MCP clients can inspect state without expensive tool calls.
- Common workflows become one prompt/tool call rather than a long manual chain.

## Phase 7: Rules and skills in Rust

Implement a Rust rules/skills loader:

Rule sources:

- global app settings
- `.puppet-master/rules/*.md`
- `.puppet-master/skills/<name>.md`
- `.puppet-master/skills/<name>/SKILL.md`
- optional imports from `AGENTS.md`, `CLAUDE.md`, and `.cursor/rules`
- session overrides from the event log

Merge order:

```text
global -> project -> imported repo rules -> session
```

Expose:

```text
list_project_rules
read_project_rules
list_orchestration_skills
run_orchestration_skill
preview_orchestrator_prompt
```

User impact:

- API and CLI orchestrators follow the same project playbook.
- Users can set routing preferences such as "Codex implements, Claude reviews" once.
- Skills become pane-aware instead of generic markdown instructions.

## Phase 8: Retire legacy Node bridge

`packages/bridge` appears redundant now that the embedded Rust bridge is canonical.

Options:

1. Delete it after tests confirm nothing depends on standalone Node bridge behavior.
2. Keep only a tiny test stub if needed for package-level tests.
3. Move any useful protocol helpers into Rust or `packages/shared`.

User impact:

- Less duplicate architecture.
- Less JS source.
- Fewer bridge behavior mismatches.

## Phase 9: UI improvements enabled by Rust core

Once Rust owns the orchestration state, improve the user-facing model/orchestrator experience:

- First-run setup: detect installed CLIs, API keys, MCP health, PATH issues, and mobile pairing readiness.
- Model routing screen: show agent strengths, current model signal, cost/auth mode, and recommended use.
- Pane roles: implementer, reviewer, shell, orchestrator, observer.
- Session timeline: decisions, delegations, locks, completions, errors.
- Worker ownership badges on panes.
- Lock conflict UI with clear release/renew actions.
- Context preview before delegation.
- Rules and skills editor with merged prompt preview.
- CLI orchestrator status panel showing whether MCP config is installed and reachable.

User impact:

- Users stop needing to understand the bridge/MCP internals.
- Model choice becomes explicit and explainable.
- Orchestrator progress is inspectable instead of hidden in terminal buffers.

## Suggested order of implementation

1. Mark generated bundles as generated or remove them from source control.
2. Add Rust tool registry and expose `/mcp/tools`.
3. Make sidebar/external MCP consume the registry.
4. Rewrite stdio MCP as Rust binary with npm shim.
5. Add session context and pane digests.
6. Add `delegate_task`, `wait_for_pane`, and `set_pane_role`.
7. Move standby/prompt-detection orchestration state into Rust.
8. Add MCP resources/prompts.
9. Add rules and skills loader.
10. Retire legacy Node bridge.
11. Build UI surfaces on top of the new Rust state.

## Success metrics

- Rust source share above 40% after generated artifacts are excluded.
- JavaScript displayed percentage near or below 20% after generated artifacts are excluded or removed.
- No duplicate tool definitions across Rust bridge, sidebar, and external MCP.
- Sidebar and external MCP expose the same tool list.
- `press_key` and future TUI-control tools work everywhere.
- Orchestrators use pane digests/session context before raw scrollback.
- Multi-pane tasks require fewer `read_terminal_buffer` calls.
- Users can see pane ownership, task status, locks, and evidence in the UI.
- CLI orchestrators follow the same rules and skills as the API sidebar.

## Non-goals

- Do not rewrite the React app in Rust.
- Do not replace xterm.js unless there is a specific terminal rendering problem.
- Do not add Rust code only to manipulate language percentages.
- Do not add more MCP tools by copy-pasting TypeScript schemas into multiple locations.

