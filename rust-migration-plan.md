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

## Tiny implementation steps

Use this section as the actual working checklist. Each item should be small enough to implement, test, and review on its own. Prefer one tiny PR or commit per checked item unless several adjacent documentation-only or test-only items are safer together.

Current source-only LOC baseline from `node scripts/count-loc.mjs`:

```text
Rust: 8,492
TypeScript: 18,057
JavaScript: 1,135
Total: 27,684
```

Final source-only LOC after p27 from `node scripts/count-loc.mjs`:

```text
Rust: 9,929
TypeScript: 17,688
JavaScript: 1,191
Total: 28,808
```

### Step 0: Establish the baseline

- [x] p0-1: Run the existing test suite and record the command/results in the PR notes.
- [x] p0-2: Run the existing build/typecheck command and record the command/results in the PR notes.
- [x] p0-3: Run a source line count that excludes generated output and `target`.
- [x] p0-4: Save the baseline Rust/TypeScript/JavaScript counts in this document.
- [x] p0-5: Confirm whether `packages/app/src-tauri/resources/mcp-stdio.bundle.cjs` is generated.
- [x] p0-6: Confirm whether `packages/app/src-tauri/resources/pwa-dist/**` is generated.
- [x] p0-7: Confirm whether any tracked `dist/**` folders are required for releases.
- [x] p0-8: Add a short "generated artifact policy" note to the relevant project docs if one already exists.

### Step 1: Mark generated files

- [x] p1-1: Add one `.gitattributes` entry for `packages/app/src-tauri/resources/mcp-stdio.bundle.cjs`.
- [x] p1-2: Verify GitHub Linguist recognizes that file as generated locally or through documentation of the expected behavior.
- [x] p1-3: Add one `.gitattributes` entry for `packages/app/src-tauri/resources/pwa-dist/**`.
- [x] p1-4: Add one `.gitattributes` entry for `packages/app/dist/**`.
- [x] p1-5: Add one `.gitattributes` entry for `packages/app/dev-dist/**`.
- [x] p1-6: Add one `.gitattributes` entry for `packages/**/dist/**`.
- [x] p1-7: Re-run the source line count.
- [x] p1-8: Update the baseline counts in this document.
- [x] p1-9: Run the existing test suite.
- [x] p1-10: Run the existing build/typecheck command.

### Step 2: Locate existing tool definitions

- [x] p2-1: List every tool defined in `packages/mcp-server/src/index.ts`.
- [x] p2-2: List every tool defined in `packages/app/src/lib/mcp-tools.ts`.
- [x] p2-3: List every Rust bridge route that already behaves like a tool.
- [x] p2-4: Add a temporary comparison table to this document.
- [x] p2-5: Mark which tools exist only in external MCP.
- [x] p2-6: Mark which tools exist only in the sidebar.
- [x] p2-7: Mark which Rust routes are not exposed as tools.
- [x] p2-8: Identify the smallest read-only tool that can be migrated first.
- [x] p2-9: Identify the smallest mutating tool that can be migrated later.

Post-migration comparison outcome:

| Surface | Tool source | Notes |
| --- | --- | --- |
| Rust bridge | `packages/app/src-tauri/src/tool_registry.rs` | Canonical tools, resources, prompts, visibility, and safety metadata. |
| Sidebar/API loop | `GET /mcp/tools` via `loadPuppetMasterTools` | Falls back to an empty generated-era list only when the bridge registry is unavailable. |
| External MCP | Rust stdio binary plus legacy TypeScript fallback | Both read the Rust registry; the legacy fallback no longer owns a separate tool list. |

### Step 3: Add a Rust registry shell

- [x] p3-1: Create an empty Rust `tool_registry` module.
- [x] p3-2: Export the module from the nearest existing Rust module root.
- [x] p3-3: Add a `ToolDefinition` struct with only `name` and `description`.
- [x] p3-4: Add one unit test for serializing a `ToolDefinition`.
- [x] p3-5: Add one hardcoded read-only tool definition.
- [x] p3-6: Add one unit test that the registry returns that tool.
- [x] p3-7: Run Rust tests only.
- [x] p3-8: Run the full existing test suite.

### Step 4: Add schema fields gradually

- [x] p4-1: Add an `input_schema` field to `ToolDefinition`.
- [x] p4-2: Add a unit test for an empty object input schema.
- [x] p4-3: Add an `output_schema` field to `ToolDefinition`.
- [x] p4-4: Add a unit test for omitting or returning an output schema consistently.
- [x] p4-5: Add a `visibility` field for sidebar and external MCP availability.
- [x] p4-6: Add a unit test for a sidebar-visible tool.
- [x] p4-7: Add a unit test for an external-MCP-visible tool.
- [x] p4-8: Add a `safety` or annotation field for read-only/destructive/open-world behavior.
- [x] p4-9: Add a unit test for a read-only annotation.
- [x] p4-10: Add a unit test for a destructive annotation.
- [x] p4-11: Run Rust tests only.

### Step 5: Expose `/mcp/tools`

- [x] p5-1: Add a bridge route handler for `GET /mcp/tools`.
- [x] p5-2: Return the single registry tool from the route.
- [x] p5-3: Add a route-level test for successful JSON output.
- [x] p5-4: Add a route-level test for stable field names.
- [x] p5-5: Start the app or bridge locally.
- [x] p5-6: Call `GET /mcp/tools` manually and save the observed output in PR notes.
- [x] p5-7: Run Rust tests.
- [x] p5-8: Run the full existing test suite.

### Step 6: Move one read-only tool into the registry

- [x] p6-1: Pick one simple read-only tool from the comparison table.
- [x] p6-2: Add its name and description to the Rust registry.
- [x] p6-3: Add its input schema to the Rust registry.
- [x] p6-4: Add its bridge route mapping to the Rust registry.
- [x] p6-5: Add a unit test for the exact tool name.
- [x] p6-6: Add a unit test for the exact input schema shape.
- [x] p6-7: Confirm `GET /mcp/tools` includes the tool.
- [x] p6-8: Do not remove the TypeScript copy yet.
- [x] p6-9: Run Rust tests.
- [x] p6-10: Run the full existing test suite.

### Step 7: Make external MCP read the registry

- [x] p7-1: Add a tiny TypeScript client function that fetches `GET /mcp/tools`.
- [x] p7-2: Add a test for registry fetch success.
- [x] p7-3: Add a test for bridge-down registry fetch failure.
- [x] p7-4: Use the fetched registry for `tools/list`.
- [x] p7-5: Keep the old hardcoded list as a fallback for one step if needed.
- [x] p7-6: Add logging or error text for registry fetch failures.
- [x] p7-7: Verify external MCP still starts.
- [x] p7-8: Verify `tools/list` returns the migrated read-only tool.
- [x] p7-9: Run package tests for `packages/mcp-server`.
- [x] p7-10: Run the full existing test suite.

### Step 8: Remove one external MCP duplicate

- [x] p8-1: Remove the migrated read-only tool's hardcoded schema from `packages/mcp-server/src/index.ts`.
- [x] p8-2: Keep its call forwarding behavior unchanged.
- [x] p8-3: Add or update a test that proves `tools/list` still includes the tool.
- [x] p8-4: Add or update a test that proves calling the tool still reaches the same bridge route.
- [x] p8-5: Run package tests for `packages/mcp-server`.
- [x] p8-6: Run the full existing test suite.

### Step 9: Make the sidebar read the registry

- [x] p9-1: Add a tiny app client function that fetches `GET /mcp/tools`.
- [x] p9-2: Add a test for sidebar registry fetch success.
- [x] p9-3: Add a test for sidebar registry fetch failure.
- [x] p9-4: Render the migrated read-only tool from the fetched registry.
- [x] p9-5: Keep existing sidebar behavior for all unmigrated tools.
- [x] p9-6: Verify the sidebar still displays existing tools.
- [x] p9-7: Verify the migrated tool appears once.
- [x] p9-8: Run app tests.
- [x] p9-9: Run the full existing test suite.

### Step 10: Remove one sidebar duplicate

- [x] p10-1: Remove the migrated read-only tool's hardcoded sidebar definition.
- [x] p10-2: Keep its UI action behavior unchanged.
- [x] p10-3: Add or update a test that proves the sidebar still displays the tool.
- [x] p10-4: Add or update a test that proves no duplicate tool appears.
- [x] p10-5: Run app tests.
- [x] p10-6: Run the full existing test suite.

### Step 11: Repeat the registry migration one tool at a time

- [x] p11-1: Move one more read-only tool into Rust.
- [x] p11-2: Add Rust registry tests for that tool.
- [x] p11-3: Remove the external MCP duplicate for that tool.
- [x] p11-4: Remove the sidebar duplicate for that tool.
- [x] p11-5: Run package tests.
- [x] p11-6: Run the full existing test suite.
- [x] p11-7: Repeat until all read-only tools are registry-backed.
- [x] p11-8: Move one mutating tool into Rust.
- [x] p11-9: Add safety annotation tests for that mutating tool.
- [x] p11-10: Remove the external MCP duplicate for that tool.
- [x] p11-11: Remove the sidebar duplicate for that tool.
- [x] p11-12: Run package tests.
- [x] p11-13: Run the full existing test suite.
- [x] p11-14: Repeat until all current tools are registry-backed.

### Step 12: Add registry-backed resources and prompts

- [x] p12-1: Add an empty Rust resource registry.
- [x] p12-2: Expose `GET /mcp/resources`.
- [x] p12-3: Add one resource definition for `puppet-master://session`.
- [x] p12-4: Add a route-level test for `GET /mcp/resources`.
- [x] p12-5: Make external MCP use the resource registry for resource listing.
- [x] p12-6: Run package tests.
- [x] p12-7: Add an empty Rust prompt registry.
- [x] p12-8: Expose `GET /mcp/prompts`.
- [x] p12-9: Add one prompt definition for `status_check`.
- [x] p12-10: Add a route-level test for `GET /mcp/prompts`.
- [x] p12-11: Make external MCP use the prompt registry for prompt listing.
- [x] p12-12: Run the full existing test suite.

### Step 13: Start the Rust MCP stdio binary

- [x] p13-1: Add a new Rust binary target named `puppet-master-mcp`.
- [x] p13-2: Make the binary print or return a version in a testable way.
- [x] p13-3: Add a smoke test that the binary starts.
- [x] p13-4: Add port-file reading behind a tiny function.
- [x] p13-5: Add a unit test for valid port-file parsing.
- [x] p13-6: Add a unit test for missing port-file behavior.
- [x] p13-7: Add bridge health checking behind a tiny function.
- [x] p13-8: Add a unit test for bridge-down error shape.
- [x] p13-9: Run Rust tests.

### Step 14: Teach the Rust binary one MCP method

- [x] p14-1: Implement MCP initialize handling.
- [x] p14-2: Add a stdio-level test for initialize.
- [x] p14-3: Implement `tools/list` using the Rust tool registry.
- [x] p14-4: Add a stdio-level test for `tools/list`.
- [x] p14-5: Confirm the JSON-RPC response shape matches MCP expectations.
- [x] p14-6: Run Rust tests.
- [x] p14-7: Run the existing external MCP tests for comparison.

### Step 15: Forward one tool call through the Rust binary

- [x] p15-1: Pick the same migrated read-only tool.
- [x] p15-2: Implement tool-call forwarding to the bridge.
- [x] p15-3: Add a test for successful forwarding.
- [x] p15-4: Add a test for bridge-down forwarding failure.
- [x] p15-5: Add a test for bridge error propagation.
- [x] p15-6: Run Rust tests.
- [x] p15-7: Manually call the tool through the Rust binary.

### Step 16: Add the npm shim

- [x] p16-1: Add a package script or bin entry that invokes the Rust binary.
- [x] p16-2: Keep the TypeScript MCP wrapper available during the transition.
- [x] p16-3: Add documentation for selecting the Rust MCP wrapper.
- [x] p16-4: Verify the npm shim starts the Rust binary.
- [x] p16-5: Verify `tools/list` works through the npm shim.
- [x] p16-6: Run package tests.
- [x] p16-7: Run the full existing test suite.

### Step 17: Switch MCP to Rust by default

- [x] p17-1: Change the default MCP entrypoint to the Rust binary shim.
- [x] p17-2: Keep a documented fallback to the TypeScript wrapper for one release if needed.
- [x] p17-3: Verify install/start behavior on the local platform.
- [x] p17-4: Verify bridge-down errors are user-friendly.
- [x] p17-5: Verify `tools/list` matches the sidebar registry.
- [x] p17-6: Run Rust tests.
- [x] p17-7: Run package tests.
- [x] p17-8: Run the full existing test suite.

### Step 18: Shrink the old TypeScript MCP wrapper

- [x] p18-1: Remove code paths that are now handled by the Rust binary.
- [x] p18-2: Delete unused TypeScript MCP schema definitions.
- [x] p18-3: Delete bundle scripts that only served the old wrapper if no longer needed.
- [x] p18-4: Keep `mcp-stdio.bundle.cjs` as a generated one-release fallback because packaging still uses it.
- [x] p18-5: Update package docs.
- [x] p18-6: Run package tests.
- [x] p18-7: Run the full existing test suite.
- [x] p18-8: Re-run source line counts.
- [x] p18-9: Update the counts in this document.

### Step 19: Add session context storage

- [x] p19-1: Define the smallest Rust session context struct with `current_goal`.
- [x] p19-2: Add a serialization unit test.
- [x] p19-3: Persist `current_goal` through the existing event log.
- [x] p19-4: Add a projection test for `current_goal`.
- [x] p19-5: Expose `read_session_context`.
- [x] p19-6: Add a route/tool test for `read_session_context`.
- [x] p19-7: Expose `update_session_context`.
- [x] p19-8: Add a route/tool test for updating `current_goal`.
- [x] p19-9: Run Rust tests.

### Step 20: Add pane roles

- [x] p20-1: Define allowed pane roles in Rust.
- [x] p20-2: Add serialization tests for each role.
- [x] p20-3: Add `set_pane_role`.
- [x] p20-4: Add a test for setting a valid role.
- [x] p20-5: Add a test for rejecting an invalid role.
- [x] p20-6: Persist pane role changes in the event log.
- [x] p20-7: Add pane role projection tests.
- [x] p20-8: Show pane roles in `read_session_context`.
- [x] p20-9: Run Rust tests.

### Step 21: Add pane digests

- [x] p21-1: Define a small pane digest struct.
- [x] p21-2: Add a serialization test.
- [x] p21-3: Store a manually supplied digest first.
- [x] p21-4: Expose `read_pane_digest`.
- [x] p21-5: Add a test for an existing digest.
- [x] p21-6: Add a test for a missing digest.
- [x] p21-7: Add digest update events to the event log.
- [x] p21-8: Add projection tests.
- [x] p21-9: Run Rust tests.

### Step 22: Add structured delegation skeleton

- [x] p22-1: Define a `DelegateTaskRequest` struct.
- [x] p22-2: Add a serialization/deserialization test.
- [x] p22-3: Validate required fields.
- [x] p22-4: Add a test for missing `intent`.
- [x] p22-5: Add a test for empty acceptance criteria.
- [x] p22-6: Add a `delegate_task` route that validates but does not launch a worker yet.
- [x] p22-7: Add a route/tool test for validation success.
- [x] p22-8: Add a route/tool test for validation failure.
- [x] p22-9: Run Rust tests.

### Step 23: Render delegation prompts

- [x] p23-1: Add a Rust prompt renderer trait or small function.
- [x] p23-2: Implement the Codex prompt style first.
- [x] p23-3: Add a snapshot or string-shape test.
- [x] p23-4: Include acceptance criteria in the rendered prompt.
- [x] p23-5: Include locked resources in the rendered prompt.
- [x] p23-6: Include evidence requirements in the rendered prompt.
- [x] p23-7: Record Claude/OpenCode/shell styles as later follow-ups; Codex style is the implemented path.
- [x] p23-8: Run Rust tests.

### Step 24: Move orchestration state one field at a time

- [x] p24-1: Identify one durable field in `packages/app/src/lib/puppet-master.ts`.
- [x] p24-2: Add the matching Rust state field.
- [x] p24-3: Add a Rust unit test for its default value.
- [x] p24-4: Add an event that updates the field.
- [x] p24-5: Add a projection test for the field.
- [x] p24-6: Expose the field through a bridge endpoint.
- [x] p24-7: Make TypeScript read that field from Rust.
- [x] p24-8: Remove only the duplicate TypeScript ownership for that field.
- [x] p24-9: Run app tests.
- [x] p24-10: Run Rust tests.
- [x] p24-11: Record additional durable fields as follow-ups after standby policy migration.

### Step 25: Retire the Node bridge carefully

- [x] p25-1: List every reference to `packages/bridge`.
- [x] p25-2: Mark each reference as test-only, build-only, docs-only, or runtime.
- [x] p25-3: Remove one unused reference.
- [x] p25-4: Run tests.
- [x] p25-5: Remove one more unused reference.
- [x] p25-6: Run tests again.
- [x] p25-7: Confirm no still-useful Node bridge helper needed moving.
- [x] p25-8: Confirm no moved-helper tests were required.
- [x] p25-9: Delete `packages/bridge` only after no runtime or test references remain.
- [x] p25-10: Run the full existing test suite.

### Step 26: Build UI on Rust state one surface at a time

- [x] p26-1: Show pane roles in the UI.
- [x] p26-2: Add a test or visual check for pane roles.
- [x] p26-3: Show session timeline events in the UI.
- [x] p26-4: Add a test or visual check for timeline rendering.
- [x] p26-5: Show lock conflict state in the UI.
- [x] p26-6: Add a test or visual check for lock conflict rendering.
- [x] p26-7: Show MCP health in the UI.
- [x] p26-8: Add a test or visual check for MCP health rendering.
- [x] p26-9: Show context preview before delegation.
- [x] p26-10: Add a test or visual check for context preview rendering.

### Step 27: Final cleanup and measurement

- [x] p27-1: Confirm there are no duplicate tool definitions across Rust, sidebar, and external MCP.
- [x] p27-2: Confirm sidebar and external MCP expose the same registry-backed tool list.
- [x] p27-3: Confirm generated bundles are excluded from language stats or no longer tracked.
- [x] p27-4: Re-run source line counts.
- [x] p27-5: Update the final Rust/TypeScript/JavaScript counts in this document.
- [x] p27-6: Run the full existing test suite.
- [x] p27-7: Run the full build/typecheck command.
- [x] p27-8: Run a security scan appropriate for the repo.
- [x] p27-9: Update README/MCP docs.
- [x] p27-10: Write a final migration summary.

Final migration summary:

- Rust now owns the MCP tool, resource, and prompt registry, the stdio MCP binary, session context, pane roles, pane digests, lock-conflict projection, delegation prompt rendering, and the first durable orchestrator standby policy fields.
- Sidebar/API orchestration and the legacy TypeScript MCP fallback both load tool definitions from `GET /mcp/tools`; hardcoded TypeScript tool definition lists are empty fallbacks rather than active duplicate sources.
- `packages/bridge` has been retired. The MCP package smoke test now uses a tiny local HTTP stub and the Rust MCP binary, which validates the packaged Rust executable and registry-backed `tools/list`.
- Generated bundles and dist outputs are covered by `.gitattributes`; `mcp-stdio.bundle.cjs` remains tracked only as a generated one-release TypeScript fallback.
- Verification completed: `cargo test --bin puppet-master-mcp`, `cargo test --lib`, `npm run typecheck`, `npm test`, `npm run test:mcp-package`, and `npm audit --audit-level=moderate`.
- `npm audit --audit-level=moderate` is clean after moving the app to Vite 6.4.3. `cargo audit` was attempted but is not installed in this environment.
- Known benign stderr remains in `npm test`: jsdom/xterm reports `HTMLCanvasElement.prototype.getContext` as not implemented, while the test suite still passes.
- Final source share is Rust 34.5%, TypeScript 61.4%, and JavaScript 4.1%; the migration stopped at functional ownership rather than adding Rust-only code to chase the aspirational 40% Rust metric.

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

