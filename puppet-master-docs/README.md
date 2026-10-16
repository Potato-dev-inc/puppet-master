# Puppet Master docs

Everything about Puppet Master in one folder: what it is, where we are, what comes next, and one file for every feature.

Written 2026-10-16 for version **0.2.1**, in simple English. Each feature file says honestly whether it works today, is a Preview, is partly built, or is still coming.

## Start here

| File | What is in it |
|---|---|
| [00 · What is Puppet Master](00-what-is-puppet-master.md) | The product in one page: the problem, the solution, who it is for |
| [01 · Status today](01-status-today.md) | Releases, what works, what is not done, where files are |
| [02 · Install and update](02-install-and-update.md) | How to install, update and connect agents and the phone |
| [03 · Plans and roadmap](03-plans-and-roadmap.md) | Alpha (2 Nov) and soft launch (7 Dec), next features, business plan |
| [04 · Decisions waiting](04-decisions-waiting.md) | What only the founder can decide, with recommendations |
| [05 · The website](05-website.md) | The landing-page draft: what it says, what is missing |
| [06 · Glossary](06-glossary.md) | The words used in these docs |

## Features (one file each)

### Everyday use

| # | Feature | Status |
|---|---|---|
| 01 | [The cockpit](features/01-cockpit.md) | Works today |
| 02 | [Inbox and "Needs you"](features/02-inbox-needs-you.md) | Works today |
| 03 | [Approval cards](features/03-approval-cards.md) | Works today |
| 04 | [Receipts and History](features/04-receipts-and-history.md) | Works today |
| 05 | [Undo and checkpoints](features/05-undo-checkpoints.md) | Works today |
| 06 | [Command palette and Ask bar](features/06-command-palette-and-ask-bar.md) | Works today for the Ctrl+K palette |
| 07 | [Simple mode](features/07-simple-mode.md) | Preview (Labs, off by default) |
| 08 | [First-run welcome](features/08-first-run-welcome.md) | Works today |
| 09 | [Settings](features/09-settings.md) | Works today |
| 10 | [Follow-ups and agent questions](features/10-follow-ups-and-agent-questions.md) | Works today |
| 11 | [Live preview](features/11-live-preview.md) | Works today |

### Agents

| # | Feature | Status |
|---|---|---|
| 12 | [Supported agents](features/12-supported-agents.md) | Works today |
| 13 | [Set up agents and Doctor](features/13-set-up-agents-and-doctor.md) | Works today |
| 14 | [Standby workers (keep agents ready)](features/14-standby-workers.md) | Works today |
| 15 | [Trust and start-up screens](features/15-trust-and-startup-dialogs.md) | Works today |
| 16 | [Typing permit](features/16-typing-permit.md) | Works today |
| 17 | [Agent authority](features/17-agent-authority.md) | Works today |
| 18 | [Tested versions and canary check](features/18-tested-versions-canary.md) | Works today |
| 19 | [Hook Lane (live updates from agents)](features/19-hook-lane.md) | Partly built |
| 20 | [Accounts and keys](features/20-accounts-and-keys.md) | Works today |
| 21 | [Pane titles, activity line and pane status](features/21-pane-titles-and-activity.md) | Works today |

### Safety and changes

| # | Feature | Status |
|---|---|---|
| 22 | [Separate copies (git worktrees)](features/22-separate-copies-worktrees.md) | Works today |
| 23 | [Review, merge and pull requests](features/23-review-merge-and-pull-requests.md) | Works today |
| 24 | [Budgets](features/24-budgets.md) | Works today |
| 25 | [Usage limit and hand-off](features/25-usage-limit-and-hand-off.md) | Partly built |
| 26 | [Tamper guard](features/26-tamper-guard.md) | Preview (Labs, off by default) |
| 27 | [Loop guard and Stop all (panic stop)](features/27-loop-guard-and-panic-stop.md) | Works today |
| 28 | [Flight recorder and diagnostic file](features/28-flight-recorder-and-diagnostics.md) | Works today |
| 29 | [Security: the bridge and phone permissions](features/29-security-bridge-and-devices.md) | Works today |
| 30 | [Missions](features/30-missions.md) | Preview (Labs, off by default) |
| 31 | [Second opinion](features/31-second-opinion.md) | Works today, but tested only by automated tests |

### Agents controlling agents, phone, team

| # | Feature | Status |
|---|---|---|
| 32 | [Agents controlling agents (MCP)](features/32-agents-controlling-agents-mcp.md) | Works today |
| 33 | [One brain (shared project memory)](features/33-one-brain.md) | Works today |
| 34 | [Agent skill pack ("Add Puppet Master to your agents")](features/34-agent-skill-pack.md) | Works today |
| 35 | [Toolbelt (add an MCP server once, for every agent)](features/35-toolbelt.md) | Works today, but checked by automated tests only |
| 36 | [Phone app](features/36-phone-app.md) | Works today, with limits |
| 37 | [Phone approval card (Allow once / Deny)](features/37-phone-approval-card.md) | Works today |
| 38 | [Phone terminal](features/38-phone-terminal.md) | Works today, with limits |
| 39 | [Co-work Heads-up (team)](features/39-co-work-heads-up.md) | Preview (Labs, off by default) |
| 40 | [Team relay (end-to-end encrypted)](features/40-team-relay.md) | Partly built |
| 41 | [Developer testing (fake agent, replay corpus, UI harness)](features/41-developer-testing.md) | Works today |

## Status words

- **Works today**: built and in 0.2.1. The file says if it was checked live or only by automated tests.
- **Preview (Labs, off by default)**: built, but you turn it on in Settings › General › Labs.
- **Partly built**: some parts work, others are still coming.
- **Coming**: designed, not built yet.

## More detail

These docs are a summary. The full reports are in `D:\pm-night\` (night logs, plans, designs, test reports). The developer docs are in the repo under `docs/`.
