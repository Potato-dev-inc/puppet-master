# Plans and roadmap

## The big dates (plan, not promises)

- **Private alpha: Mon 2 Nov 2026.**
  - Who: 5 power-user design partners and 20 people from the waitlist.
  - What they get: a signed build that can update itself.
- **Soft launch: Mon 7 Dec 2026.**
  - The public v1.0 installer, also on winget.
- **Rule: "slip the date, not the gate".** If the must-have items are not ready by Fri 30 Oct, the alpha moves to Mon 9 Nov.

## Must-have before the alpha (in order)

1. **Licence decision.** The founder decides. See [04-decisions-waiting.md](04-decisions-waiting.md).
2. **The rename** (Ninedot?). Decide by Wed 21 Oct.
3. **CI on GitHub Actions**, so every change is tested automatically.
4. **Installer testing on a clean Windows PC.** It must cover install, update from 0.1.4 and 0.2.x, and uninstall.
5. **Code signing**, so Windows stops warning. The founder buys a certificate.
6. **Auto-update**: the app updates itself with one click.
7. **Crash reports, opt-in.** Nothing is sent without your click.
8. **Telemetry, opt-in**: an update-check counter and weekly counts. It is off by default.
9. **Real phone tests**: an iPhone home-screen app and an Android phone.
10. **Real Cloudflare named-tunnel test.**
11. **Dogfood day.** The founder uses it for a full day. Then we set the safety defaults.
12. **A new npm package** for the MCP server.
13. **Docs and website**: install guide, phone setup, privacy page, FAQ, and a working waitlist.
14. **Team relay deployed** (needed for the team beta).

## Product features next

| Feature | What it is | State |
|---|---|---|
| **Phone polish** | One count, buttons in thumb reach, a bottom bar | Next. It waited for the phone card, which is now done. |
| **Hand-off at the limit** | When an agent runs out of usage, give the work to another agent | Designed. Only detection is built. |
| **Missions v1** | Phone view, push notifications for missions, resume after a usage limit | Missions v0 is done (Preview) |
| **Stuck-card actions** | "Give it a hint", "Stop", "Ask another agent" on a stuck agent | Planned |
| **Pocket** | A small phone view to start tasks quickly | Planned |
| **Bedtime queue** | Queue tasks to start later (for example, at night) | Planned |
| **`pm claude`** | Start Claude Code through Puppet Master from any terminal | Planned |
| **Explainer and error translator** | Explain agent errors in plain words | Planned |
| **Undo polish** | Turns grouped in History, "Tell the agent I undid this" | Partly done |
| **Hook Lane evidence mode** | Use the exact signals from Claude and Codex, not only the screen | Shadow mode is built |
| **Real sandbox** | True isolation, more than a separate copy | Later |
| **macOS / Linux** | Other systems | Later. Windows comes first. |

## Business plan (short)

- **Free:** the desktop app for one person.
- **Team (planned): $8 per active member per month**, or $80 per year.
- **Business (planned): $16 per member**, minimum 5 members, once SSO exists.
- Money starts with the hosted team relay ("Puppet Master Cloud"), around Q2 2027.
- Self-hosting the relay on Cloudflare's free plan will cost $0.
- Details: `D:\pm-night\BUSINESS.md`. Full plan: `D:\pm-night\PLAN-POST-MVP.md` and `D:\pm-night\ROADMAP.md`.
