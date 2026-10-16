# Glossary: words used in these docs

| Word | Meaning |
|---|---|
| **Agent** | An AI coding program like Claude Code, Codex, OpenCode, Cursor Agent or Kilo. |
| **Pane** | One agent's terminal inside Puppet Master. |
| **Cockpit** | The main Puppet Master window: sidebar, panes and Inbox. |
| **Inbox / Needs you** | The list of things waiting for your answer. The number is the same on the PC, the tray and the phone. |
| **Card** | A box that shows one question or event, with buttons (Allow once / Deny, Trust this folder / Don't trust, …). |
| **Turn** | One task sent to an agent and the agent's work until it stops. |
| **Receipt** | The summary of one turn: what was asked, what changed, which approvals were given. |
| **Checkpoint** | A saved state of your files before a turn, so you can undo. |
| **Separate copy (worktree)** | A second copy of your git project where an agent works, so your main folder is not touched until you merge. |
| **Merge** | Bringing the changes from a separate copy into your main project. |
| **PR (pull request)** | A request on GitHub to merge changes. Others can review it there. |
| **Trust dialog** | The screen an agent shows the first time in a folder: "Do you trust this folder?". Puppet Master never answers it for you. |
| **Typing permit** | Puppet Master's rule for when it may type into an agent: only when the agent is ready, never in the middle of its work. |
| **Bridge** | The small local server inside the app (`127.0.0.1:17321`). The cockpit, the phone and MCP talk to it. |
| **MCP** | Model Context Protocol. A standard way to give an AI agent extra tools. Puppet Master's MCP tools let one agent control other agents. |
| **Handle** | The ID you get back from `run_agent`. You use it to follow up or wait for the answer. |
| **Headless** | An agent run with no visible terminal (in the background). |
| **PWA** | Progressive Web App. A web page you can add to your phone's home screen, so it works like an app. This is the phone app. |
| **Tunnel** | A secure connection from the internet to your PC (Cloudflare), so your phone can reach the app. |
| **Pairing** | Connecting a phone to your PC once, by scanning a QR code. |
| **Device caps (capabilities)** | What a paired phone may do: watch, answer, type or control. |
| **Labs** | Built features that are off by default, under Settings › General › Labs. |
| **Preview** | A Labs feature that is new and may change. |
| **Simple mode (Plain mode)** | A simpler look for people who don't code. |
| **Mission** | A goal plus checks. Puppet Master keeps the agent working until the checks pass (Preview). |
| **Budget** | A limit on turns or minutes for an agent. |
| **Hand-off** | Giving the work to another agent when one hits its usage limit. Only detection is built today. |
| **Hook Lane** | Exact signals from Claude/Codex ("started", "asking", "finished"). Today they are only recorded next to what Puppet Master reads from the screen. |
| **Canary** | A quick start-up test of an agent, to see if its screens look as expected. |
| **Relay** | A server that passes encrypted messages between team members' apps. Built, not deployed. |
| **Heads-up** | Team feature: tells teammates when you push, so their agent can catch up. |
| **Tamper guard** | Finds tests that an agent skipped, deleted or made weaker. |
| **Flight recorder** | A short recording of a failed run, kept on your PC, to help debug. |
| **Fake TUI** | A test tool that pretends to be Claude/Codex screens, used for automatic tests. |
