# Puppet Master Rust Coordination Kernel Plan
```
┌──────────────────────────────────────────────────────────────────────────────┐
│                              Clients / Agents                                │
│                                                                              │
│  Desktop UI     Mobile PWA     Sidebar LLM     External MCP     Agent Panes  │
└────────┬────────────┬──────────────┬──────────────┬──────────────┬──────────┘
         │            │              │              │              │
         │ commands   │ commands     │ commands     │ MCP tools    │ reports
         ▼            ▼              ▼              ▼              ▼
┌──────────────────────────────────────────────────────────────────────────────┐
│                         Rust Coordination Kernel                             │
│                                                                              │
│  ┌────────────────────────────────────────────────────────────────────────┐  │
│  │ Command Gateway                                                        │  │
│  │ normalizes Tauri / HTTP / MCP / scheduler / agent commands             │  │
│  └──────────────────────────────────┬─────────────────────────────────────┘  │
│                                     ▼                                        │
│  ┌────────────────────────────────────────────────────────────────────────┐  │
│  │ Policy Engine                                                          │  │
│  │ permissions, roles, leases, locks, pane safety, risk classification    │  │
│  └──────────────┬───────────────────────────────────────┬─────────────────┘  │
│                 │ accepted                              │ risky/pending      │
│                 ▼                                       ▼                    │
│  ┌────────────────────────────────────┐    ┌─────────────────────────────┐   │
│  │ Append-Only Event Log              │    │ Approval Queue              │   │
│  │ canonical source of truth          │    │ user confirms risky actions │   │
│  └──────────────────┬─────────────────┘    └──────────────┬──────────────┘   │
│                     │                                     │ approved         │
│                     │                                     ▼                  │
│                     │                            back to Command Gateway     │
│                     │                                                        │
│       ┌─────────────┼──────────────────┬────────────────────┐                │
│       ▼             ▼                  ▼                    ▼                │
│  ┌─────────┐  ┌─────────────┐  ┌────────────────┐  ┌────────────────────┐    │
│  │ Event   │  │ Projections │  │ Snapshots /    │  │ Audit Timeline     │    │
│  │ Broker  │  │ read models │  │ Compaction     │  │ human/debug view   │    │
│  └────┬────┘  └──────┬──────┘  └────────────────┘  └────────────────────┘    │
│       │              │                                                       │
│       ▼              ▼                                                       │
│  ┌─────────┐  ┌──────────────────────────────────────────────────────────┐   │
│  │ Live    │  │ Read Models                                              │   │
│  │ Streams │  │ workspace state · tasks · locks · inboxes · pane state   │   │
│  └─────────┘  │ artifacts · decisions · summaries · workspace memory     │   │
│               └──────────────┬───────────────────────────────┬───────────┘   │
│                              │                               │               │
│                              ▼                               ▼               │
│               ┌──────────────────────────┐      ┌────────────────────────┐   │
│               │ Scheduler                │      │ Context Pack Builder   │   │
│               │ deterministic mechanics  │      │ compact task packets   │   │
│               │ idle/stale/locks/retry   │      │ for agents/managers    │   │
│               └───────────┬──────────────┘      └────────────┬───────────┘   │
│                           │ proposes commands                │ context       │
│                           └──────────────┐                    ▼              │
│                                          │       ┌────────────────────────┐  │
│                                          │       │ Execution Runtime      │  │
│                                          │       │ PTY + adapters + tools │  │
│                                          │       └────────────┬───────────┘  │
│                                          │                    │ observations │
│                                          ▼                    ▼              │
│                                  back to Command Gateway ────────────────┐   │
│                                                                          │   │
│  ┌───────────────────────────────────────────────────────────────────────▼┐  │
│  │ Agent Adapters                                                         │  │
│  │ raw PTY/tool output → structured observations/reports/results/events   │  │
│  └────────────────────────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────────────────────────┘
```

## Goal

Make Rust the primary backend language for Puppet Master by moving coordination, state, events, policy, scheduling, agent interpretation, and MCP execution into the Tauri Rust core.

The product model stays simple:

- The orchestrator is the manager.
- Agent panes are the workforce.
- MCPs and tools are departments.
- The Rust bridge is the local coordination kernel.
- The task board, inboxes, locks, and workspace state are projections from an append-only event log.

The target is not a full rewrite. React, the desktop UI, mobile PWA, and lightweight TypeScript client glue should remain TypeScript. Rust should own authority and coordination.

## Target Architecture

```text
Clients / Agents / MCP Hosts
        |
        | Commands
        v
Command Gateway
HTTP / Tauri / MCP / WebSocket
        |
        v
Validator / Policy Engine
permissions, roles, approvals, leases, pane safety
        |
        +---- risky command ----> Approval Queue ----+
        |                                            |
        +---- accepted command ----------------------+
                                                     |
                                                     v
Append-Only Event Log
canonical source of truth, replayable, snapshot-backed
        |
        +----> Event Broker
        |      SSE / WebSocket / broadcast streams
        |
        +----> Projection Builders
        |      rebuildable read models
        |
        +----> Snapshots / Compaction
        |
        v
Read Models
workspace state, task board, agent inboxes, resource locks,
pane state, audit timeline, workspace memory
        |
        +----> Scheduler
        |      idle detection, retries, stale leases, review routing
        |
        +----> Context Pack Builder
               compact task prompts, relevant files, locks, decisions,
               summaries, evidence, expected report format
                    |
                    v
Execution Runtime
Pane Runtime + Agent Adapters + Tool/MCP Runtime
PTY I/O, parsed observations, tool calls, shell/repo operations
                    |
                    v
Agent Workforce
manager, coder, tester, reviewer, shell agents
                    |
                    | stdout / reports / tool results
                    v
Agent Adapters
Claude/Codex/OpenCode/Bash parsers
                    |
                    v
structured observations appended back to Event Log
```

Core loop:

```text
command -> validated event -> projection -> scheduler/context pack -> agent/tool execution -> observation event
```

## Ownership Boundaries

### Rust Owns

- PTY runtime and pane registry
- HTTP bridge, SSE, and WebSocket event streams
- command validation and policy
- approval queue
- append-only event log
- event replay and snapshots
- projection builders and read models
- task board and task leases
- resource locks
- agent inboxes
- scheduler
- context pack builder
- agent adapters
- native MCP server/runtime
- tool routing and local MCP client execution
- persistence
- mobile pairing and security-sensitive bridge behavior

### TypeScript Owns

- React desktop UI
- sidebar and settings UI
- terminal rendering with xterm.js
- mobile PWA
- thin generated API clients
- LLM provider configuration UI
- temporary compatibility wrappers while Rust features land

### Node Eventually Shrinks To

- npm distribution shim for `@puppet-master/mcp`
- compatibility entry point that launches the Rust MCP binary or talks to the Rust bridge

## Phase 1: Rust Becomes The Context Authority

Move duplicated context intelligence out of TypeScript fallbacks and into Rust.

### Work

- Add Rust `agent_contexts` module.
- Implement real `GET /agent-contexts`.
- Implement complete `GET /panes/:id/agent-context`.
- Move model detection logic from `packages/shared/src/agent-contexts.ts` into Rust.
- Return static profile, pane metadata, detected model, and recent buffer preview from Rust.
- Keep TypeScript schemas aligned with Rust responses.
- Remove or reduce TypeScript fallback behavior in `packages/mcp-server/src/index.ts` and `packages/app/src/lib/mcp-tools.ts`.

### Acceptance Criteria

- External MCP and sidebar receive the same context response from Rust.
- `/agent-contexts` no longer returns `[]`.
- Model inspection works through the bridge without TypeScript reconstructing it.
- Existing MCP smoke tests still pass.

## Phase 2: Event Types And Append-Only Log

Introduce the canonical event model without changing every workflow at once.

### Work

- Add Rust modules:
  - `commands.rs`
  - `events.rs`
  - `event_log.rs`
  - `actors.rs`
- Define stable identifiers:
  - `EventId`
  - `CommandId`
  - `ActorId`
  - `TaskId`
  - `PaneId`
  - `ResourceId`
- Start with JSONL persistence for easy inspection.
- Design for SQLite migration once querying and snapshots become important.
- Record bridge-level facts:
  - `PaneSpawned`
  - `PaneKilled`
  - `PaneInputWritten`
  - `PaneOutputObserved`
  - `PaneStatusChanged`
  - `McpToolCalled`
  - `McpToolCompleted`

### Acceptance Criteria

- Every pane spawn/write/kill emits an event.
- Event log survives app restart.
- Event entries include timestamp, actor, correlation id, and payload.
- A simple replay command can print the reconstructed pane timeline.

## Phase 3: Projections And Read Models

Build current state from events instead of making every caller infer it.

### Work

- Add projection builders for:
  - workspace state
  - task board
  - pane state
  - agent inboxes
  - resource locks
  - audit timeline
- Add endpoints:
  - `GET /workspace/state`
  - `GET /tasks`
  - `GET /locks`
  - `GET /agents/:id/inbox`
  - `GET /audit`
- Mark read models as disposable and rebuildable from the event log.

### Acceptance Criteria

- Delete read model cache and rebuild it from event log.
- Rebuilt state equals pre-delete state for tested scenarios.
- UI and MCP clients can read task/pane/lock state without scraping terminal buffers.

## Phase 4: Task Board, Leases, And Resource Locks

Add explicit work ownership so agents do not duplicate or conflict.

### Work

- Add task commands:
  - `CreateTask`
  - `ClaimTask`
  - `RenewTaskLease`
  - `UpdateTaskStatus`
  - `CompleteTask`
  - `BlockTask`
  - `AssignReviewer`
- Add lock commands:
  - `AcquireResourceLock`
  - `ReleaseResourceLock`
  - `ExpireResourceLock`
- Lock resource types:
  - file
  - directory
  - command
  - port
  - git branch
  - pane ownership
- Expose MCP tools:
  - `create_task`
  - `claim_task`
  - `report_task_status`
  - `complete_task`
  - `list_tasks`
  - `acquire_resource_lock`
  - `release_resource_lock`

### Acceptance Criteria

- Two agents cannot claim the same exclusive task.
- Two agents cannot simultaneously lock the same file.
- Stale task leases expire.
- Orphaned locks are released or escalated after pane exit.

## Phase 5: Agent Adapters

Convert raw terminal output into structured observations.

### Work

- Add adapter trait:

```rust
trait AgentAdapter {
    fn agent_type(&self) -> AgentType;
    fn observe(&mut self, pane_id: &str, text: &str) -> Vec<SystemEvent>;
}
```

- Implement adapters:
  - `claude.rs`
  - `codex.rs`
  - `opencode.rs`
  - `bash.rs`
- Detect:
  - agent ready
  - agent idle
  - agent blocked
  - permission prompt
  - command started
  - command finished
  - test failure
  - test success
  - file change mention
  - final answer/report
- Feed adapter observations into the event log.

### Acceptance Criteria

- PTY output creates structured observation events.
- Permission prompts are detected consistently.
- The system can identify ready/blocked/done states without repeated manual buffer reads.

## Phase 6: Context Pack Builder

Stop sending raw scrollback and oversized global context to agents.

### Work

- Add `context_pack.rs`.
- Inputs:
  - task
  - relevant event history
  - workspace memory
  - resource locks
  - pane status
  - recent failures
  - user constraints
  - manager instructions
- Output:
  - compact prompt
  - expected report format
  - allowed tools/MCPs
  - ownership boundaries
  - evidence requirements
- Add endpoint:
  - `POST /context-packs`
- Add MCP tool:
  - `build_context_pack`

### Acceptance Criteria

- Generated context pack is smaller than equivalent raw scrollback prompt.
- Context pack includes task, relevant files, locks, decisions, and expected output.
- Agents receive context packs when assigned work.

## Phase 7: Scheduler

Add deterministic coordination mechanics while keeping judgment with the LLM manager.

### Scheduler Owns

- idle agent detection
- stale task retries
- lease expiry
- lock expiry
- tester wakeups after file changes
- reviewer routing after implementation
- blocked task escalation
- simple policy-driven assignment

### LLM Manager Still Owns

- priorities
- ambiguous decomposition
- strategy changes
- quality judgment
- user communication
- conflict resolution when policy is insufficient

### Work

- Add `scheduler.rs`.
- Scheduler watches projections and emits commands.
- Add event subscriptions internally from event broker to scheduler.
- Implement simple policies first:
  - if task is unclaimed and compatible agent is idle, assign it
  - if coder reports file modified, create or wake test task
  - if task lease expires, mark stale and reassign or escalate
  - if implementation completes, assign review task

### Acceptance Criteria

- Coder completion can automatically wake tester.
- Stale task is recovered without manual manager polling.
- Scheduler decisions are recorded as commands/events.
- LLM manager can override scheduler assignments.

## Phase 8: Approval Queue And Policy Engine

Make risky operations explicit and user-controlled.

### Work

- Add command classification:
  - safe
  - needs approval
  - denied
- Add approval events:
  - `ApprovalRequested`
  - `ApprovalGranted`
  - `ApprovalDenied`
  - `ApprovalExpired`
- Add risky categories:
  - destructive filesystem operations
  - git push
  - git reset/checkout destructive operations
  - network tunnels
  - secret access
  - process killing outside owned pane
  - package publishing
- Add endpoints:
  - `GET /approvals`
  - `POST /approvals/:id/grant`
  - `POST /approvals/:id/deny`

### Acceptance Criteria

- Risky commands do not execute before approval.
- Pending approvals appear in UI/read model.
- Approval resolution is event logged.

## Phase 9: Native Rust MCP Server

Move MCP execution authority from Node into Rust.

### Current Path

```text
MCP host -> Node stdio server -> HTTP bridge -> Rust
```

### Target Path

```text
MCP host -> Rust MCP server -> Rust coordination kernel
```

### Work

- Implement Rust MCP stdio server.
- Expose existing tools:
  - `bridge_health`
  - `list_panes`
  - `spawn_agent`
  - `read_terminal_buffer`
  - `write_terminal_input`
  - `kill_pane_process`
- Expose new coordination tools:
  - task tools
  - lock tools
  - context pack tools
  - approval tools
  - workspace state tools
- Keep `@puppet-master/mcp` as npm shim that launches the Rust binary.

### Acceptance Criteria

- External MCP hosts can use Rust MCP server directly.
- Node MCP package remains installable.
- Tool results match or improve current Node MCP behavior.

## Phase 10: Tool/MCP Runtime

Use more MCPs coherently by assigning them through roles and policy.

### Work

- Add tool registry:
  - local shell tools
  - repo/git tools
  - browser tools
  - iOS/Xcode tools
  - database tools
  - documentation/search tools
  - GitHub/release tools
- Associate tools with roles:
  - manager
  - coder
  - tester
  - reviewer
  - release
  - research
- Route tool calls through policy and event log.
- Record all tool calls as events.

### Acceptance Criteria

- Agents receive only role-appropriate tools.
- Tool calls are auditable.
- Tool failures produce structured events and can trigger scheduler behavior.

## Phase 11: Snapshots, Compaction, And Workspace Memory

Keep long sessions fast and useful.

### Work

- Add periodic snapshots for projections.
- Add replay from checkpoint.
- Add workspace memory:
  - durable decisions
  - repo facts
  - resolved issues
  - long-running project summaries
  - known flaky tests
- Make context pack builder read workspace memory.

### Acceptance Criteria

- Long event logs do not slow startup significantly.
- Replay from snapshot yields same state as full replay.
- Context packs include durable decisions without rereading old logs.

## Test Strategy

Treat the architecture as testable coordination behavior.

Do not rely primarily on real LLM agents for tests. Use deterministic simulated agents first, shell agents second, and real Claude/Codex/OpenCode agents for end-to-end smoke tests.

### Test Levels

1. Simulated agents
   - scripts or Rust actors that receive assignments and emit known events
   - fast and deterministic

2. Shell agents
   - real PTY panes running deterministic shell scripts
   - validates PTY, adapters, and event flow

3. LLM agents
   - real Claude/Codex/OpenCode panes
   - validates practical workflow and prompt quality

### Core Scenarios

#### Basic Assignment

Flow:

```text
CreateTask -> TaskClaimed -> ContextPackBuilt -> AgentPrompted -> AgentReported
```

Pass:

- exactly one agent receives the task
- task board projection matches event log

#### No Duplicate Work

Flow:

```text
one task, two idle coder agents
```

Pass:

- only one agent claims the task
- the other remains idle or receives different work

#### File Lock Conflict

Flow:

```text
two tasks require the same file
```

Pass:

- first task gets the file lock
- second waits, reroutes, or blocks
- no simultaneous writes to the same file

#### Tester Wakeup

Flow:

```text
Coder emits FileModified -> Scheduler creates/wakes test task -> Tester emits TestPassed/TestFailed
```

Pass:

- no manager polling required
- test result becomes an event

#### Agent Blocked

Flow:

```text
AgentReportedBlocked
```

Pass:

- task becomes blocked
- manager is notified
- no repeated terminal polling loop

#### Approval Queue

Flow:

```text
risky command requested
```

Pass:

- command does not execute
- approval request appears
- only approval resolution allows execution

#### Replay

Flow:

```text
run scenario -> delete read models -> replay event log
```

Pass:

- rebuilt task board, locks, inboxes, pane ownership, and audit timeline match

#### Crash Recovery

Flow:

```text
kill worker pane mid-task
```

Pass:

- pane exit event appears
- task lease expires
- task is reassigned or blocked
- no orphaned lock remains forever

#### Context Pack Quality

Flow:

```text
compare generated context pack against raw scrollback prompt
```

Pass:

- context pack is smaller
- includes relevant task, files, constraints, locks, and evidence
- excludes irrelevant terminal history

#### Multi-Agent Review

Flow:

```text
coder completes task -> scheduler assigns reviewer -> review result recorded
```

Pass:

- task does not become complete until review policy passes

### Metrics

Track these before and after the architecture changes:

- polling calls per task
- repeated terminal buffer reads per task
- task assignment latency
- handoff latency from coder to tester/reviewer
- duplicate task assignments
- lock conflicts prevented
- stale tasks recovered
- prompt/context size
- replay time
- recovery success rate
- user approval interruptions
- tool failure recovery rate

Expected direction:

```text
Polling calls:        down
Duplicate work:       down
Handoff latency:      down
Prompt size:          down
Recovery failures:    down

Traceability:         up
Parallelism:          up
Replayability:        up
User control:         up
Agent autonomy:       up
```

## Recommended First Implementation Slice

Build the smallest loop that proves the architecture:

```text
Create task
-> fake coder claims task
-> context pack is built
-> fake coder emits FileModified
-> scheduler wakes fake tester
-> fake tester emits TestPassed
-> projection marks task complete
```

This validates:

- commands
- events
- projections
- scheduler
- context packs
- fake agent runtime
- feedback loop into the event log

Once this deterministic loop works, wire the same path to real pane agents.

## Practical Milestones

### Milestone A: Rust Context Authority

- Rust `/agent-contexts`
- Rust `/panes/:id/agent-context`
- Rust model detection
- TypeScript fallback reduction

### Milestone B: Event Spine

- event types
- append-only log
- pane events
- event broker

### Milestone C: Coordination State

- task board
- locks
- inboxes
- audit projection
- replay tests

### Milestone D: Agent Intelligence Boundary

- adapters
- context packs
- structured reports

### Milestone E: Autonomous Mechanics

- scheduler
- lease recovery
- tester/reviewer wakeups
- approval queue

### Milestone F: Rust MCP

- native MCP server
- npm shim
- expanded coordination tools

## Non-Goals

- Do not rewrite the React UI in Rust.
- Do not make the deterministic scheduler responsible for strategic judgment.
- Do not expose every MCP/tool to every agent by default.
- Do not require real LLM agents for deterministic coordination tests.
- Do not build snapshots before replay and projections are useful.
- Do not turn the event log into mutable state; projections are rebuildable, events are facts.

## Guiding Principle

Every important system fact should become an event. Every current-state view should be a projection. Every agent prompt should be a context pack. Every risky action should pass policy. Every autonomous decision should be auditable.
