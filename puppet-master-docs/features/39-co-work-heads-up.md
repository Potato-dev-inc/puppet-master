# Co-work Heads-up (team)

**Status:** Preview (Labs, off by default). The main flow was checked live with two demo profiles, Maya and Leo, over a local relay. GitHub cards are tested by automated tests only, and need real GitHub.

## What it is

Two or more people each have their own Puppet Master (PM) and their own agents. Heads-up keeps them in sync. When Maya pushes a branch, PM asks her "Tell the team?". If she says yes, her teammates get a card in their Inbox. Leo can then let **his own agent** merge Maya's branch into his work in a separate copy, and fix conflicts. Leo reviews the result and applies it. Nothing is pushed, merged or run without a click from a person.

## Why it matters

Small teams that use agents often work on the same project. Someone pushes, and the others do not notice for hours. Then merge conflicts are harder. Heads-up tells people at the right time and does the boring part with an agent. A person still decides.

## How to use it

Turn it on and make a team (one person, the admin):

1. Open **Settings > General > Labs** and choose **Team**.
2. Under **Create a team**, fill in **Team name** and **Your name**. Then give a **Relay address** (a server that passes sealed messages, see [Team relay](40-team-relay.md)) or switch on **Use a shared folder instead** and give a **Shared folder**. Click **Create team**.
3. Make sure the switch **Use Team** is on.
4. Click **Invite someone**. Send the link and the code to your teammate in two different messages. The code works once, for 10 minutes.

Join (teammate):

1. Open **Settings > General > Labs > Team**. Under **Join with an invite**, fill in **Invite link**, **Code**, **Your name** and, if you like, **GitHub login**. Click **Ask to join**.
2. The admin sees "<name> wants to join" and approves by hand.

Daily use:

- After you push, a toast says **Tell the team?**. You see the exact card your teammates will get. Click **Tell**, or **Not now**. You can add a note. Unsent items wait in **Inbox > Team**.
- When a teammate pushes, a card appears in **Inbox > Team**, for example "Maya pushed feat/auth". Use **Pull now**, or **Queue my agent…**, or **Dismiss**.
- With **Queue my agent…**, choose the agent. It waits until its current turn is done, merges in a separate copy, and solves conflicts. The result card has **See changes**, **Apply to my branch** and **Discard**.
- **Apply to my branch** only fast-forwards your branch. It never pushes. It refuses on `main`, with uncommitted changes, or while an agent is working.
- For a GitHub pull request, the card can offer **Merge when green**. A sheet shows GitHub's own title, branch and commit. If you confirm, GitHub's own auto-merge is switched on. GitHub merges when its checks pass. PM never merges.
- Team cards also appear on the phone, in the Agents list.
- To leave, click **Leave this team**. The admin can remove a member, and the team key then changes.

## How it works (simple)

- Every message is signed by the sender and sealed (end-to-end encrypted). Only team members can read it. See [Team relay](40-team-relay.md).
- A teammate's message is only text that becomes a card. Nothing runs when it arrives.
- A queued merge starts an agent by itself only if that agent cannot run commands without asking: today Claude Code and Cursor Agent. For other agents, the job waits for you at **Start <agent>** and runs in a pane you can watch.
- Facts such as "checks passed" or "merged" come only from your own `gh` tool. A teammate's message that claims one is refused.
- Members and their keys are kept in a signed list that only the admin can change.

## Limits and what is not done yet

- Off by default. It was checked live only on one computer with two profiles. It was not checked with two real people over the internet, and not with real GitHub.
- There is **no hosted relay** from Puppet Master. You host a relay yourself, or use a shared folder. A shared folder has no access control. A removed member keeps access to the folder.
- One admin only. Leaving a team is local. A member cannot revoke their own lost device yet.
- No forward secrecy. Whoever gets a device's keys and the relay's stored messages can read what that device could read.
- No per-project filter. Every member sees the repository, branch and PR title of every project you announce.
- A new member does not get the last 7 days of messages (relay). On a shared folder, a new member can read what is still in the folder.
- GitHub cards need the `gh` tool and a login. GitHub names are trusted for attribution only after the admin clicks **Confirm GitHub**.
- A security review found no Critical or High issues. It found six Medium ones. The fixes are in the release branch and are tested by automated tests only.

## For developers

- Rust: `packages/app/src-tauri/src/team/` (`keystore.rs`, `roster.rs`, `secure.rs`, `crypto.rs`, `invite.rs`, `membership.rs`, `merge_green.rs`, `sources/github.rs`, `phone.rs`, `transport/`).
- Phone routes: `GET /team/cards` (needs Watch), and dismiss, pull, queue, apply, discard (need Answer or Control). Agent sessions are refused. File: `bridge/routes/team.rs`.
- Keys are in the OS credential store, per profile. Audit kinds: `team.sent`, `team.received`, `team.action`, `team.joined`, `team.role_changed`, `team.key_rotated`.
- UI: `components/settings/blocks/TeamSettings.tsx`, `InviteSheet.tsx`, `components/cockpit/team/`, `components/mobile/TeamGroup.tsx`, `lib/team.ts`.
- Agent switch for queued merges: `TuiProfile::headless_runs_commands_unattended`.

## Related

- [Team relay](40-team-relay.md)
- [Phone app](36-phone-app.md)
- [Agents controlling agents (MCP)](32-agents-controlling-agents-mcp.md)
