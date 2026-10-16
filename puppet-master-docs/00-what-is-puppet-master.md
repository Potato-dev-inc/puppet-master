# What is Puppet Master?

## In one sentence

Puppet Master is a Windows app that runs all your AI coding agents in one window, and keeps you in control of them.

## The longer version

People now use AI coding agents like **Claude Code**, **Codex**, **OpenCode**, **Cursor Agent** and **Kilo**. Each one runs in its own terminal window. When you use more than one, things get messy:

- You have many windows open and you lose track of which agent is doing what.
- An agent stops and waits for a "yes / no" answer, and you don't notice for an hour.
- An agent changes files you did not want changed, and it is hard to undo.
- You can't see or answer anything when you are away from your PC.
- One agent can't easily ask another agent for help.

Puppet Master fixes this. It **does not have its own AI**. It starts the real agent programs, with **your own logins**, in real terminals, and puts a control layer around them:

| Problem | What Puppet Master does |
|---|---|
| Too many windows | One window (the **cockpit**) with every agent in a sidebar |
| Missed questions | An **Inbox** with one count of "what needs you". Every question becomes a **card** with the risk in plain words (Low risk / Check first) |
| Unwanted changes | Every turn leaves a **receipt** and a **checkpoint** you can undo. An agent can work in a **separate copy** of your project, and you review and merge the result |
| Away from the PC | A **phone app**: see your agents and answer Allow / Deny from your phone |
| Agents can't work together | Through **MCP**, agents can start other agents, give them tasks and wait for the answers |
| Agents doing too much | **Budgets** (number of turns or minutes), **Stop all**, a loop guard, and switches for what agents may do |

## Who it is for

We designed it for three kinds of people:

1. **Developers.** They want many agents in parallel, and they want to review, merge and make pull requests (PRs). They use the full cockpit.
2. **People who build with AI but write little code ("vibecoders").** They want it to be safe: every task runs on a copy, with simple Keep / Throw away buttons.
3. **People who don't code.** They use **Simple mode**: "Your projects · Make something", plain words, few buttons.

The first-run welcome asks "How do you work?" and sets the app up for one of these three.

## What makes it different

- **Works with the agents you already pay for.** No new AI subscription. No API keys needed for the main agents.
- **Real terminals, not a copy.** You can always open the real terminal and type yourself.
- **Never answers a trust question for you.** When an agent asks "do you trust this folder?", Puppet Master stops and asks *you*.
- **Undo for everything.** Checkpoints and separate copies.
- **Agents controlling agents.** One agent can run a team of other agents through MCP tools (`run_agent`, `wait_agents`, …).
- **Windows first.** The Windows app is the main product. macOS is not part of the current build.

## How it is built (short)

- **Desktop app:** Tauri 2. The back end is Rust (terminals, safety rules, the local "bridge" server). The front end is React (the cockpit).
- **Bridge:** a small local server at `127.0.0.1:17321` that the cockpit, the phone and the MCP program talk to.
- **MCP program:** `puppet-master-mcp.exe`, which agents load so they can control other agents.
- **Phone app:** a web app (PWA) served by the desktop app, reached over your own Cloudflare tunnel.
- **Agent definitions:** each supported agent is described by a "TUI profile" (code) or a TOML file. New agents can be added without changing the core.

## Current version

**0.2.1**, released 2026-10-16, marked "Latest" on GitHub:
https://github.com/Potato-dev-inc/puppet-master/releases/tag/0-2-1

See [01-status-today.md](01-status-today.md) for what works and what does not.

## About the name

"Puppet Master" may have to change: there is a US trademark, an open-source project with the same pitch, and a bad meaning in Mandarin. The recommended new name is **Ninedot** (after the 3×3 dot logo). This is waiting for the founder. See [04-decisions-waiting.md](04-decisions-waiting.md).
