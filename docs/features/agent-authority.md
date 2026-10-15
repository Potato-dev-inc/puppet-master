# Agent authority: consent, merge gate, audit log

Branch `sec/agent-authority` (roadmap P0-7 and P0-9). Full report: `D:\pm-night\p0\agent-authority\REPORT.md`.

**Why.** `take_over(grant=true)` was self-asserted: any agent could take any pane, including the user's own, and answer its prompts. `review_worktree(merge)` let an orchestrator merge its helper's work.

**Two switches, both off-the-record defaults = today.** Flip the defaults in `src-tauri/src/agent_authority/mod.rs` (one constant each):

| | today | recommended |
|---|---|---|
| A "Ask me before an agent controls a pane it didn't start" | off | on |
| B "Let agents merge separate copies" | on | off |

**Where.** `agent_authority/` (gate, consent, merge, config), `mcp_sessions/lineage.rs`, `audit/`, `questions/` (`ConsentInfo`), Settings > Bridge & MCP > Security, Inbox `ConsentCard`.

**What the gate covers (review R12).**
- "Always for this pane" is the asking agent's grant: keyed by (`mcp_sessions::grant_identity`, pane), i.e. the coordinator at the top of the caller's helper chain. Its reconnects and helpers keep it; any other agent gets its own card.
- Every way an agent answers a prompt goes through `check_answer` (setting A: a card for a pane outside its lineage above low risk) and writes an Approval audit line: `answer_prompt`; OpenCode permission and question replies (`bridge/routes/opencode.rs::gate_reply`, and an agent's `reply: "always"` needs `allow_broad=true`); raw keys from `write_terminal_input`/`press_key` into a pane whose screen shows a dialog (`gate::check_raw_keys`, audited even with A off, which keeps today's behaviour of letting them through).
- The direct `POST /worktrees/{handle}/merge` route writes a Merge audit line (phone or local program).
- The audit log holds an exclusive lock on `<dir>/.append.lock` and re-reads the tip under it, so two writers keep one chain; month files follow the chain, not the clock.

**Limits.** The MCP schema of `reply_opencode_permission` cannot carry `allow_broad` yet (the catalog is pinned); the shim forwards it when an agent passes it anyway.  Helper links (`link_child_session`) are not made automatically yet; until they are, a helper's pane counts as "another agent's" for its parent. Plain loopback HTTP is still trusted as the app for everything except answering a consent card (needs per-launch secret, R3 idea 13).
