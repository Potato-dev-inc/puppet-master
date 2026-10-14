<!-- DRAFT for the founder to review and publish. Remove [CONFIRM] markers before publishing. -->

# Tunnel access check: post-mortem (2026-10-14)

## What happened

This issue was rated critical. Puppet Master runs a local service for the desktop app and its companion tools. Remote access, through a tunnel, a reverse proxy, or a paired phone, goes through the same service. A check decides whether a request needs a paired-device token.

In affected versions, that check could be fooled. A request that came in through a tunnel or proxy could be treated as coming from the app itself, and skip the token. A web page open in your browser could make the same requests while the app was running.

A request treated this way could type into agent panes, run shell commands, start agents, and read pane output. It could also pair a new device, which would keep access after the tunnel was closed.

The release tunnel's proxy had a separate bug that kept it from reaching the local service in those versions. Fixing that bug is part of this change.

We found this during a security review. [CONFIRM: credit to the reporter, if any, and the disclosure timeline.]

## Who was affected

Anyone running an affected desktop build, from 0-1-1 until the fixed release. [CONFIRM: last affected release.] People who never set up a tunnel were still exposed through a web page open in their browser.

Pairing tokens were not exposed by this bug. An attacker could still have paired a device of their own.

[CONFIRM: whether any misuse was seen. The internal record does not say.]

## What we did

The check now requires a paired-device token for any request that came through a forwarder, arrived under a web address other than the local one, or came from a web page. If the check cannot tell where a request came from, it asks for the token.

We added end-to-end tests for the tunnel path and the direct path. Before the fix, 10 of 13 cases failed. All 13 pass now.

The fix is in commits a9f7fc3 and 9055cae on branch fix/tunnel-auth. [CONFIRM: release that includes the fix, and whether it is merged to main.] [CONFIRM: whether to name the branch and commits publicly.]

## What you should do

1. Update to [CONFIRM: fixed release version] or later, then restart the app.
2. Open the mobile pairing settings, revoke every paired device, and pair your phone again. If you cannot tell which devices are yours, revoke all of them.
3. Stop the tunnel and start it again. A quick tunnel gets a new random address each time. If you use a custom domain or a reserved ngrok domain, rotate it.
4. Check your pane history for commands you did not type. Check the event log for shell commands you did not start.
5. Point tunnels and reverse proxies at the PWA server, not at the app's internal local service port.

## What we're changing

These items are planned and not done yet.

- A per-launch secret shared by the desktop app, its window, the command-line tool, and the agent connector, so another program on your machine cannot pass as the app. We plan to log this first and enforce it later.
- Paired devices with limited permissions that expire after 30 days without use.
- A tamper-evident audit log of approvals, pairings, and shell commands.
- A narrower cross-origin header on responses from the local service.
