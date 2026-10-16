# Agent authority

**Status:** Works today · The switches and the list work (seen live). The consent card is tested only by automated tests. The default values are still an open decision.

## What it is
Agents can use Puppet Master (PM) through MCP, a standard way for agents to use tools. An agent can start other agents, take over a pane, answer questions in a pane, and merge another agent's work. "Agent authority" is a set of switches that limits this. It also keeps a list of recent approvals and control changes, so you can see who allowed what.

## Why it matters
Without limits, any agent could take over any pane, even one you opened yourself, and answer its questions. An agent could also merge the work of its helper without you looking. These switches let you choose how much agents may do on their own.

## How to use it
1. Open **Settings** and choose **General**. Go to the group **Safety**. (The older location, Bridge & MCP, now only shows a link called "Agent safety".)
2. Switch A: **Ask me before an agent controls a pane it didn't start.** Today it is **off**. When on, an agent that did not open a pane must ask you first, in the Inbox, before it types into the pane or answers its questions. Low-risk answers do not need a card. An answer whose risk is not known counts as not low.
3. Switch B: **Let agents merge separate copies.** Today it is **on**. When off, an agent can show you what changed in a separate copy (a git worktree), but only you can merge it, from the **Changes** tab.
4. Read **Recent approvals and control changes**. It shows the last 20 entries: time, what happened, and who did it. Below it you may see "Log checked: N entries, none changed."
5. When switch A is on and an agent asks, a card appears in the Inbox: "Allow Claude Code to control Codex 1?" The buttons are **Deny** (the main button, on purpose), **Allow once**, and **Always for this pane**. **Open terminal** shows the pane.

## What the two switches change

| | Switch A (ask before control) | Switch B (agents may merge) |
|---|---|---|
| Today | Off | On |
| Recommended later | On | Off |
| When it is off | Agents take over any pane without asking | Only you can merge, from **Changes** |
| When it is on | An agent asks you first, in the Inbox | An agent can merge its helper's copy by itself |

## How it works (simple)
- **Lineage.** PM remembers who started each pane. A pane is "own" to an agent when that agent started it, when another connection of the same coordinator did, or when one of its linked helpers did. Panes you opened are in nobody's lineage.
- **Where the gate sits.** Taking over a pane, adopting a pane, answering a prompt (`answer_prompt`), replying to OpenCode permissions, and merging a copy. Raw keys sent into a pane that shows a dialog are always written to the list, even when switch A is off.
- **"Always for this pane"** belongs to the coordinator at the top of the asking agent's helper chain. Its reconnects and helpers keep it. Any other agent gets its own card.
- **The card cannot be answered everywhere.** Not from a lock screen, and not over plain local HTTP. The desktop needs a real click.
- **You are never gated.** If you merge in the **Changes** tab, no switch applies.
- **The list.** Each line is linked to the one before it, so a change after the fact is detected. A lock file keeps two writers from breaking the chain. Nothing in the list can be edited from the window.
- **Where it is kept.** The two switches live in the app's own file (`agent_authority.json`), not in `settings.json`. A paired phone cannot flip them over the bridge.

## Limits and what is not done yet
- With the defaults, any agent can take over any pane, and an agent can merge its helper's work. Recommended later: switch A on and switch B off. This needs the founder's decision. A saved choice always wins over a new default.
- Helper links are not made automatically yet (`link_child_session` has no caller in the app code). So a helper that opens its own pane may count as "another agent" for its parent. With switch A on, you could see a card for that pane. The text on the switch says helpers are not asked. I could not confirm that in code.
- The MCP schema of `reply_opencode_permission` cannot carry the `allow_broad` option yet.
- Plain local HTTP is still trusted as the app for everything except answering a consent card.
- I did not check in detail whether switch A also covers an agent's answer to a folder-trust prompt. See [Trust and start-up screens](15-trust-and-startup-dialogs.md).
- The list shows the last 20 entries. Older entries are kept in monthly files on disk.

## For developers
- `src-tauri/src/agent_authority/`: `gate.rs` (`check_answer`, `check_raw_keys`), `consent.rs`, `merge.rs`, `config.rs`. Defaults are two constants in `mod.rs`: `DEFAULT_ASK_BEFORE_AGENT_CONTROL = false`, `DEFAULT_AGENTS_MAY_MERGE = true`.
- `mcp_sessions/lineage.rs` (who started a pane), `audit/` (the chained log), `questions/` (`ConsentInfo`).
- MCP: `take_over`, `answer_prompt`, `review_worktree(action=merge)`. Error codes: `CONSENT_PENDING`, `CONSENT_DENIED`. Route: `POST /worktrees/{handle}/merge` writes a Merge line.
- Settings UI: `components/settings/sections/bridge/Security.tsx` (shown under General > Safety by `settings/arrange.tsx`). Inbox: `components/cockpit/ConsentCard.tsx`.
- Developer doc: `docs/features/agent-authority.md`.

## Related
- [Trust and start-up screens](15-trust-and-startup-dialogs.md)
- [Agents controlling agents (MCP)](32-agents-controlling-agents-mcp.md)
- [Security: bridge and devices](29-security-bridge-and-devices.md)
- [Separate copies (worktrees)](22-separate-copies-worktrees.md)
- [Review, merge and pull requests](23-review-merge-and-pull-requests.md)
