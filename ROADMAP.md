# Puppet Master — Future versions

Living plan for what comes after **0.1.x**. Not a commitment order — themes may ship in parallel or merge across releases.

**Current baseline (0.1.2):** Tauri desktop + Rust PTY + embedded bridge, mobile PWA pairing, CLI orchestrator backends (Claude / Codex / OpenCode), bundled MCP for production installs.

---

## North star

One place to run many agents, with **shared context** instead of every agent re-reading the same terminals, files, and decisions. Fewer tokens, less copy-paste, clearer handoffs.

---

## 0.2 — Production hardening

Ship what 0.1 started reliably on all platforms.

| Area | Goals |
|------|--------|
| **npm** | Publish `@puppet-master/mcp` and `puppet-master` CLI; document install for Cursor / Claude Desktop / Codex without monorepo `node_modules` |
| **Windows parity** | GUI PATH, bundled MCP, TOML-safe paths, MSI smoke tests |
| **macOS DMG** | Login-shell PATH, app-data bridge port file, project-folder guard |
| **Docs** | README + `ROUTING.md` aligned with shipped CLI orchestrator |
| **CI** | Build app + bundle MCP on macOS and Windows; basic bridge/MCP integration tests |

---

## 0.3 — Unified orchestration context

**Problem today:** Each agent pane is an island. Orchestrators burn tokens re-reading buffers, re-explaining the repo, and re-discovering what other agents already did.

**Direction:** A single **session context layer** the bridge owns — not duplicated in every MCP tool result.

### Concepts

```
┌─────────────────────────────────────────────────────────┐
│  Session context (bridge-backed, versioned)             │
│  • project summary + goals                              │
│  • pane registry snapshot (who does what)               │
│  • recent decisions / handoff notes                     │
│  • compressed buffer digests per pane (not full scroll) │
│  • optional file / git snapshot pointers                │
└───────────────────────────┬─────────────────────────────┘
                            │
     ┌──────────────────────┼──────────────────────┐
     ▼                      ▼                      ▼
 Orchestrator          Worker pane A          Worker pane B
 (sidebar / CLI)       (Codex)                (Claude)
```

### Likely deliverables

- **`read_session_context` / `update_session_context` MCP tools** — append handoff notes, structured pane roles
- **Pane digests** — rolling summary of last N minutes per pane (LLM or heuristic), served instead of raw 200-line dumps when orchestrator only needs status
- **Handoff protocol** — explicit “delegate to pane X with this intent” object, not free-text pasted into terminals
- **Token budget hints** — orchestrator loop prefers digest → targeted `read_terminal_buffer` only when needed
- **Cross-agent visibility** — sidebar shows shared context timeline (who said what, which tools ran)

### Success criteria

- Orchestrator completes a multi-pane task with measurably fewer `read_terminal_buffer` calls
- Agents can refer to “session context” without each CLI re-scraping the grid

---

## 0.35 — Enhanced MCP & orchestrator skills

**Problem today:** MCP is a thin PTY remote control. The orchestrator gets a hardcoded system prompt (`puppet-master.ts`) and nine generic tools. There is no project-specific playbook, no structured handoffs, and CLI orchestrators (Codex / Claude / OpenCode) don’t inherit the same rules as the API sidebar.

**Direction:** Richer MCP surface + first-class **skills** and **rules** that both API and CLI orchestrators load automatically.

### Enhanced MCP (tools, resources, outputs)

| Today | Planned |
|-------|---------|
| 9 tools, mostly CRUD on panes | Same core + orchestration-native tools |
| Text-heavy tool results | Structured JSON + optional human summary |
| No MCP resources | Readable **resources** for session context, digests, rules |
| No prompts | **Sampled prompts** for “status check”, “delegate refactor”, etc. |

**New / upgraded tools (candidates)**

| Tool | Purpose |
|------|---------|
| `read_session_context` / `update_session_context` | Shared orchestration memory (see 0.3) |
| `delegate_task` | Structured handoff: target pane, intent, acceptance criteria, token budget |
| `wait_for_pane` | Block until status is `idle` / `waiting_input` / prompt pattern (with timeout) |
| `read_pane_digest` | Short summary instead of full scrollback |
| `set_pane_role` | Tag pane: `implementer`, `reviewer`, `shell`, `orchestrator` |
| `list_project_rules` | Return merged rules + skills for this repo |
| `run_orchestration_skill` | Invoke a named skill (see below) with parameters |

**Protocol improvements**

- Tool descriptions tuned for **orchestrator** behavior (reuse panes, minimal buffer reads, explicit handoffs)
- `readOnlyHint` / destructive hints where MCP hosts support them
- **MCP resources** — `puppet-master://session`, `puppet-master://rules`, `puppet-master://panes/{id}/digest`
- Consistent error shapes (`bridge_down`, `pane_not_found`, `timeout`) so CLIs recover instead of `-32000` opaque failures
- Optional **tool result caching** in bridge (e.g. `list_panes` TTL 2s) to cut duplicate calls in one turn

### Orchestrator skills

**Skills** = reusable multi-step playbooks the orchestrator can run or adapt (like Cursor skills, but pane-aware).

Examples:

| Skill | Flow |
|-------|------|
| `explore-and-delegate` | `list_panes` → `read_session_context` → pick agent → `delegate_task` |
| `implement-with-review` | Codex implements → Claude reviews → orchestrator summarizes |
| `fix-ci` | Spawn shell pane → run test command → route failures to strongest code agent |
| `sync-mobile` | Push status digest to mobile / wait for user reply via bridge |

**Delivery**

- Built-in skills ship in `@puppet-master/shared` (versioned)
- Project skills: `.puppet-master/skills/<name>.md` or `.puppet-master/skills/<name>/SKILL.md`
- Optional import from repo `AGENTS.md`, `CLAUDE.md`, `.cursor/rules`
- API loop: skills appended to system prompt + exposed via `run_orchestration_skill`
- CLI loop: skills injected on orchestrator pane start + available through MCP `list_project_rules` / resources

### Orchestrator rules

**Rules** = always-on constraints (cheaper than skills; loaded every turn).

| Layer | Location | Examples |
|-------|----------|----------|
| **Global** | App settings / defaults | Always `list_panes` first; max 12 turns; prefer digests over full buffer |
| **Project** | `.puppet-master/rules/*.md` | “Use Codex for implementation, Claude for review”; test command; branch naming |
| **Session** | Session context store | User overrides for this workspace only |

**Rule merge order:** global → project → session (later wins on conflict).

**UI (ties to 0.4)**

- Settings → Orchestrator → Rules & skills (view, enable/disable, edit project files)
- “Export rules” for sharing across repos
- Indicator when CLI orchestrator is running with stale rules (file changed → re-ensure MCP / reload)

### Success criteria

- API and CLI orchestrators follow the **same** project rules without copy-paste
- Common workflows (delegate, review, CI fix) are one skill call or one short prompt, not 6+ tool round-trips
- Measurable drop in tokens per task vs 0.1.x baseline (track in orchestrator run metadata)

---

## 0.4 — Better UI

Move from “working shell” to “daily driver.”

| Theme | Ideas |
|-------|--------|
| **Desktop shell** | Splash → home (sessions) → new session → workspace (`docs/design/desktop-flow-demo.html` → real routes) |
| **Workspace** | Clear orchestrator vs worker pane layout; drag-to-arrange grid; pane labels and roles |
| **Orchestrator UX** | Unified chat + MCP log; backend switcher that doesn’t feel bolted on; error states that say what to fix |
| **Mobile PWA** | Pairing polish, tunnel vs custom URL UX, orchestrator mirror on phone |
| **Visual design** | Stronger typography, spacing, status language; light/dark that feels intentional |
| **Onboarding** | First-run: pick project → detect CLIs → MCP status → optional mobile pair |
| **Rules & skills** | In-app editor for `.puppet-master/rules` and skills; preview merged orchestrator prompt |

---

## 0.5 — Distribution & updates

| Item | Notes |
|------|--------|
| **npm** | `@puppet-master/mcp` as primary external surface; semver, changelog, `npx` story that works outside the repo |
| **Desktop releases** | GitHub Releases: DMG (macOS), MSI/NSIS (Windows); version in app matches tag |
| **Auto-update** | Tauri updater plugin: check on launch, download, restart (with user consent); delta updates later |
| **Crash / feedback** | Optional anonymous “bridge health” ping or export logs for support |

---

## 1.0 — “Orchestrated by default”

Criteria for calling it 1.0 (rough):

- [ ] CLI orchestrator stable on macOS + Windows production builds
- [ ] `@puppet-master/mcp` on npm with documented host setup
- [ ] Session context layer used by both API and CLI orchestration paths
- [ ] Enhanced MCP v2 (structured tools + resources + project rules/skills)
- [ ] Desktop shell (home / sessions / workspace) shipped
- [ ] Auto-update on at least one platform
- [ ] Mobile pairing documented and reliable via tunnel or custom domain

---

## Backlog / explore later

- **Agent-to-agent messages** — structured channel (not PTY bytes) for “ask pane B” without typing into its TUI
- **Session templates** — “full stack” = frontend + backend + test panes with preset cwd and agents
- **Cloud sync (opt-in)** — encrypted session context across machines; never raw terminal secrets by default
- **Skill marketplace** — share/export orchestration skills (community repo)
- **Rule lint** — validate project rules don’t contradict (e.g. two agents marked sole implementer)
- **MCP server composition** — optional sub-MCPs (git, CI, browser) registered in one Puppet Master config
- **Linux** — first-class `.deb` / AppImage and PATH story
- **Observability** — token/cost estimates per orchestrator run when using API backend

---

## How to use this doc

- Pick a **version theme** when planning PRs (e.g. 0.2 = npm + Windows only).
- When a theme ships, add a line to `CHANGELOG.md` and trim or move items here.
- Big bets (session context, UI shell) deserve a short design note in `docs/` before large refactors.

---

## Related docs

- [ROUTING.md](ROUTING.md) — API vs CLI orchestrator backends
- [MCP_HOSTS.md](MCP_HOSTS.md) — external MCP registration
- [PUBLISHING.md](PUBLISHING.md) — npm publish steps (today)
- [README.md](README.md) — current architecture and quick start
