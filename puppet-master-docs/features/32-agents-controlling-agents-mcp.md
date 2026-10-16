# Agents controlling agents (MCP)

**Status:** Works today. A live run with a real agent used `quick_ask` and got an answer with a receipt. Most of the other tools are checked by automated tests only.

## What it is

MCP (Model Context Protocol) is a standard way for an AI agent to use tools. Puppet Master (PM) gives your agents a set of MCP tools. With these tools, one agent can start another agent, ask it a question, send it a new task, and wait for the answer. For example, Claude Code can ask Codex for a second opinion, and you can watch Codex work in its own pane.

The tools are served by a small program, `puppet-master-mcp.exe`. It talks to the PM app on your computer. So the PM app must be running.

## Why it matters

- You can let one "lead" agent split a job and send parts to helper agents. You do not copy and paste between terminals.
- Every result comes with a receipt. The receipt says what was checked and what was **not** verified. You do not have to trust "done".
- A helper can ask **you** a question with the `ask_user` tool. The question appears in the Inbox and on your phone.

## How to use it

1. Open Puppet Master. Go to **Settings > Bridge & MCP**.
2. Find **Add Puppet Master to your agents**. Click **Review and install**. See [Agent skill pack](34-agent-skill-pack.md) for what this does.
3. Check the list of agents and files. Click **Install**.
4. Restart agents that were already open. In Claude Code, type `/mcp` to reconnect.
5. In your agent, ask in plain words. Example: "Ask Codex to review my changes." The agent calls the PM tools for you.
6. Watch the helper in PM. By default it opens in its own pop-out window, so you can see it work.

Other ways to connect:

- **Settings > Bridge & MCP > Install MCP runtime** writes the connection for Claude Code, Codex and OpenCode.
- The same page has text you can copy for Claude Desktop and Codex, if you want to edit the config by hand.

## How it works (simple)

A **handle** is a stable ID for one helper conversation. `run_agent` returns a handle. You use the same handle to steer the helper, give it a next task, or collect its answer.

The usual path has five tools:

1. `quick_ask(task)` for a small question. It returns the answer and a receipt.
2. `run_agent(agent_type, project_path, task)` starts real work. It opens a pane, runs the task, and returns the result and a handle.
3. `wait_agents(handles)` waits for results. One call waits up to 120 seconds, so an agent calls it again in a loop for long jobs.
4. `followup_task(handle, task)` starts the next turn in the same conversation.
5. `list_agents()` shows helpers and open panes you can reuse.

`send_message(handle)` is different. It steers the turn that is running now. It gives a receipt, not a result.

### Tool modes

The mode decides which tools an agent can see in its list. The default is **agent**.

| Mode | Tools listed | Use it for |
|---|---|---|
| agent | 22 | Controlling other agents (default) |
| shell | 16 | Raw terminal control: read a pane, type, press keys |
| both | 32 | Agent tools and shell tools together |
| all | 68 | Everything, including old names and advanced tools |

Tools that are not listed still work if the agent calls them by name (in the modes that allow them). The list is short on purpose, so agents are not confused by too many tools. An agent can call `set_mode`, or you can start the program with `--mode <mode>` or the setting `PUPPET_MASTER_MODE`.

### The main tools

| Tool | What it does |
|---|---|
| `quick_ask` | One small question, read-only, answer returned |
| `run_agent` | Start or reuse a helper; options include `checks`, `isolation:"worktree"`, `headless`, budgets |
| `wait_agents` | Wait for one or more handles |
| `followup_task` | Next task on the same helper |
| `send_message` | Steer the current turn |
| `list_agents` | Helpers you can reuse |
| `answer_prompt` | Answer an approval card that blocks a helper |
| `interrupt_agent`, `close_agent` | Stop a turn; end a helper |
| `review_worktree` | Show, merge or discard a helper's separate copy |
| `ask_user` | Ask you a question and wait for your answer |
| `agent_transcript`, `inspect_agent` | Read what a helper did and its state |

`run_agent` with `checks` can test the result (for example, run `npm test`). With `isolation:"worktree"` the helper works in its own separate copy of the project. `until_checks_pass` starts a Mission, which is a Preview feature in Labs.

## Limits and what is not done yet

- The PM app must be running. The tool program does not start it.
- If the tool program and the app are different versions, every result carries a loud warning. Reinstall or reconnect to fix it.
- Codex stops a tool call after 60 seconds by default, and PM does not change that setting for Codex in the app. For long jobs the agent must loop `wait_agents`.
- Two switches in **Settings > Bridge & MCP > Security** are today's defaults, and the founder still needs to decide them: "Ask me before an agent controls a pane it didn't start" is **off**, and "Let agents merge separate copies" is **on**. The tour recommends the opposite for both.
- Cursor, Kilo and OpenCode are less tested than Claude Code and Codex.
- The public npm package `@puppet-master/mcp` (0.1.3) is the old server. Use the in-app install, not npm.

## For developers

- Tool list: `packages/app/src-tauri/src/tool_registry/catalog/*.rs` (68 tools). Modes and tiers: `tool_registry/mode.rs`, `tool_registry/surface.rs`.
- Tests pin the counts 22 / 16 / 32 / 68: `tool_registry/surface_tests.rs`.
- The stdio program: `src/bin/puppet-master-mcp.rs` and `src/mcp_shim/`. It calls the app's local HTTP bridge, for example `POST /agents/run`, `/agents/quick`, `/questions/ask`.
- Settings: **Settings > Bridge & MCP** in `components/settings/sections/BridgeSection.tsx`. Starting mode: `--mode` or `PUPPET_MASTER_MODE`.
- Authority switches: `agent_authority/` (`DEFAULT_ASK_BEFORE_AGENT_CONTROL`, `DEFAULT_AGENTS_MAY_MERGE`).
- Concepts: `docs/wiki/concepts.md`.

## Related

- [Agent skill pack](34-agent-skill-pack.md)
- [Toolbelt](35-toolbelt.md)
- [Phone approval card](37-phone-approval-card.md) (answering `ask_user` from your phone)
- [Developer testing](41-developer-testing.md)
