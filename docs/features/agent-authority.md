# Agent authority: consent, merge gate, audit log

Branch `sec/agent-authority` (roadmap P0-7 and P0-9). Full report: `D:\pm-night\p0\agent-authority\REPORT.md`.

**Why.** `take_over(grant=true)` was self-asserted: any agent could take any pane, including the user's own, and answer its prompts. `review_worktree(merge)` let an orchestrator merge its helper's work.

**Two switches, both off-the-record defaults = today.** Flip the defaults in `src-tauri/src/agent_authority/mod.rs` (one constant each):

| | today | recommended |
|---|---|---|
| A "Ask me before an agent controls a pane it didn't start" | off | on |
| B "Let agents merge separate copies" | on | off |

**Where.** `agent_authority/` (gate, consent, merge, config), `mcp_sessions/lineage.rs`, `audit/`, `questions/` (`ConsentInfo`), Settings > Bridge & MCP > Security, Inbox `ConsentCard`.

**Limits.** Helper links (`link_child_session`) are not made automatically yet; until they are, a helper's pane counts as "another agent's" for its parent. Plain loopback HTTP is still trusted as the app for everything except answering a consent card (needs per-launch secret, R3 idea 13).
