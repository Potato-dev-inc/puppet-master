# Puppet Master inside your agent

Packs that let people find and use Puppet Master (PM) from INSIDE their favourite agent, and teach the agent to use it well: when to delegate, the five-tool path, receipts and checks, worktree isolation, and asking the human.

Everything here is data and text. Nothing runs at install time.

| Folder | What it is |
|---|---|
| `skills/` | The canonical skills: `puppet-master` (taught to the model) and `pm-review`, `pm-delegate`, `pm-status` (slash commands, user-invoked). The PM app embeds these files. |
| `claude-code/plugin/` | Claude Code plugin: the skills + `.mcp.json`. Marketplace file: `/.claude-plugin/marketplace.json`. |
| `codex/` | `config.toml` (MCP block), `AGENTS.snippet.md`, and a skills-only Codex plugin. Marketplace file: `/.agents/plugins/marketplace.json`. |
| `cursor/` | `mcp.json`, `rules/puppet-master.mdc`, and a Cursor plugin folder for the Cursor Marketplace. |
| `opencode/` | `opencode.json` (MCP block), a `pm-delegator` subagent, and `/pm-review`, `/pm-delegate`, `/pm-status` commands. |
| `mcp-registry/` | `server.json` draft for the official MCP Registry (+ the vendored schema it is checked against). |
| `badges/` | README badge snippets. |

The copies inside each plugin folder are generated: edit `skills/`, then run `node scripts/sync-skill-pack.mjs` (`--check` only compares). A Rust test (`agent_pack::tests::pack_files_match_canonical`) fails on drift.

## The skill in one screen

`skills/puppet-master/SKILL.md` teaches, from the real tool schemas:

1. `quick_ask(task)` for a tiny question; read `answer` and `receipt.not_verified`.
2. `run_agent(agent_type, project_path, task)` for real work; returns `status`, `result`, `receipt`, `handle`.
3. `wait_agents(handles)` to collect results (loop; each call waits at most 120 s).
4. `followup_task(handle, task)` for the next turn; `send_message` steers the current one.
5. `list_agents()` to reuse a worker.

Plus: do not trust "done" (`receipt.outcome`, `checks`), `isolation: "worktree"` with `review_worktree`, `ask_user` and `answer_prompt`. It never names the hidden aliases (`list_workers`, `send_agent`, `cancel_agent`, `wait_for_model`, `wait_for_task`, `wait_for_worker`); a test enforces that against `tool_registry::ALIASES`.

The three commands are skills with `disable-model-invocation: true`, so only their short descriptions cost context until someone runs them.

## Install

### From Puppet Master (recommended)

Settings > Bridge & MCP > **Add Puppet Master to your agents**. It finds the agents on this computer, shows every file it would write, and installs:

- the four skills into each agent's user-level skills folder (declared by the agent's profile, `TuiProfile::skills_host`): `~/.claude/skills` for Claude Code, `~/.agents/skills` for Codex, Cursor and OpenCode (they all read it);
- Puppet Master's own MCP entry, using the path of the installed app's MCP program, through the toolbelt's `[mcp_host]` writer. An entry that is already there is never rewritten.

**Undo** puts every file back: created files are removed, replaced files are restored from a copy, config files return byte for byte. A file you changed after the install is left alone.

Cursor and OpenCode also read `~/.claude/skills`, so with Claude Code installed they may list the same skill twice. The content is identical.

### Claude Code plugin

```text
/plugin marketplace add Potato-dev-inc/puppet-master
/plugin install puppet-master@pm
```

or `claude plugin marketplace add Potato-dev-inc/puppet-master` then `claude plugin install puppet-master@pm`. One-off, without installing: `claude --plugin-dir integrations/claude-code/plugin`. Commands appear as `/puppet-master:pm-review`, `/puppet-master:pm-delegate`, `/puppet-master:pm-status`.

### Codex

Add `codex/config.toml` to `~/.codex/config.toml` (or `codex mcp add puppet-master -- npx -y @puppet-master/mcp`). Keep `tool_timeout_sec`: Codex's default tool timeout is 60 s and `run_agent` may wait up to 300 s. Put `codex/AGENTS.snippet.md` in `~/.codex/AGENTS.md`, or install the skills (`$puppet-master`, `$pm-review`). Plugin: `codex plugin marketplace add Potato-dev-inc/puppet-master` (skills only; Codex's plugin MCP format was not verified, so keep the `config.toml` block).

### Cursor

Merge `cursor/mcp.json` into `~/.cursor/mcp.json`, copy `cursor/rules/puppet-master.mdc` to `.cursor/rules/`. Skills in `~/.agents/skills` or `~/.cursor/skills` work too. Marketplace: submit `cursor/plugin/` at cursor.com/marketplace/publish (manual review).

### OpenCode

Merge `opencode/opencode.json` into `~/.config/opencode/opencode.json`; copy `opencode/agents/*` and `opencode/commands/*` to `~/.config/opencode/agents|commands/` (or `.opencode/` per project). OpenCode also reads the skills from `~/.agents/skills`.

### MCP Registry

`mcp-registry/server.json` is a draft for `mcp-publisher publish`. Before publishing: add `"mcpName": "io.github.Potato-dev-inc/puppet-master"` to `packages/mcp-server/package.json`, publish the npm package, and `mcp-publisher login github`. The name follows the repository owner, so revisit it if the product is renamed.

## Known gaps (read before publishing)

- **The published npm package is stale.** `npx -y @puppet-master/mcp` (npm 0.1.3) starts the legacy server and lists 17 shell-mode tools, with no `run_agent` or `quick_ask`. The static plugin and config files here use that command because it is the only stable one; they are correct once a release with the Rust shim is published (the repo is at 0.1.4). The in-app installer does not have this problem: it writes the installed app's own program path.
- Codex plugin MCP bundling, the Cursor plugin and the MCP Registry entry are drafts that follow the docs below but have not been installed from the real stores.
- `[skills_host]` is declared by the built-in profiles only; user-made TOML agent definitions cannot declare a skills folder yet.

## Sources (checked 2026-10-14)

- Claude Code: [plugins](https://code.claude.com/docs/en/plugins), [manifest reference](https://code.claude.com/docs/en/plugins/manifest-reference), [marketplaces](https://code.claude.com/docs/en/plugin-marketplaces), [skills](https://code.claude.com/docs/en/skills).
- Codex: [MCP config](https://developers.openai.com/codex/mcp), [skills](https://developers.openai.com/codex/skills), [AGENTS.md](https://developers.openai.com/codex/guides/agents-md), [plugins](https://developers.openai.com/plugins/build/plugins).
- Cursor: [rules](https://cursor.com/docs/context/rules), [MCP](https://cursor.com/docs/context/mcp), [plugins](https://cursor.com/docs/plugins), [skills](https://cursor.com/docs/skills), [install links](https://cursor.com/docs/context/mcp/install-links).
- OpenCode: [MCP servers](https://opencode.ai/docs/mcp-servers/), [agents](https://opencode.ai/docs/agents/), [commands](https://opencode.ai/docs/commands/), [skills](https://opencode.ai/docs/skills/), [config](https://opencode.ai/docs/config/).
- MCP Registry: [quickstart](https://modelcontextprotocol.io/registry/quickstart), [package types](https://modelcontextprotocol.io/registry/package-types), [schema 2025-12-11](https://static.modelcontextprotocol.io/schemas/2025-12-11/server.schema.json).
