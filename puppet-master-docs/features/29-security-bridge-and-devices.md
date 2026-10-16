# Security: the bridge and phone permissions

**Status:** Works today. The bridge lock and the 0.1.5 fix were checked live with real requests. Phone permissions were seen live. The local secret only writes a log today. It does not block anything yet.

## What it is
Puppet Master has a small local web service called the **bridge**. The app window, the MCP program and your phone all talk to the bridge. (MCP is a standard way for agents to use tools.) The bridge decides who may do what. This page explains three parts: the bridge lock and a past security bug that was fixed, the **local secret** and strict **CORS**, and the **device capabilities** (what each paired phone may do).

## Why it matters
The bridge can type into agents, run commands and start agents. If the wrong person or program reaches it, that is a serious problem. A phone you lost, or a web page open in your browser, must not be able to control your PC. The founder asked for a design where a lost or shared phone can do only what you allowed.

## How to use it
**Choose what a phone may do:**
1. Open **Settings**, then **Phone**.
2. To pair a new phone, use the pairing panel. Under **What can this phone do?** choose **Full control** (the default today) or **Watch and answer only**. A new choice makes a new code. The code or QR is made only when you click.
3. For a phone that is already paired, open **Paired phones**. Each phone has four switches: **Watch**, **Answer**, **Use the terminal**, **Start tasks**. A change works at once.
4. A phone shows "Active · expires if unused for 30 days". After 30 days without use it is signed out. You can click **Remove** or **Revoke** at any time.

**If you ever used an old version (0.1.1 to 0.1.4):** update to 0.1.5 or later (or any 0.2.x), restart the app, revoke all paired phones and pair again. Stop and restart your tunnel. Look at your pane history for commands you did not type.

## How it works (simple)
**What each ability allows**
| Ability | What a phone can do with it |
|---|---|
| Watch | See the agent list, live terminals, transcripts and results. Without Watch, the phone gets no terminal text and no chat. Run results and loop warnings are cut down. |
| Answer | Press **Allow once** or **Deny** on question cards, and answer questions. Needed for push Allow and Deny. |
| Use the terminal | Type text and keys into a live terminal. |
| Start tasks | Everything else that acts: start or close agents, run commands, change settings, Stop all agents, make an invite for another phone. |

A route nobody listed fails closed. A read needs Watch. A write needs Start tasks. The desktop app itself is never limited by these abilities. A phone opened on the same PC is not paired, so it is refused. The app says so in one line.

**The 0.1.5 fix and advisory GHSA-xg78-9222-r32r.**
- The bug: in versions 0.1.1 to 0.1.4, the bridge could take a request that came through a tunnel or proxy for a request from the app itself, so it skipped the token check. A web page open in your browser could do the same while the app ran. Such a request could type into panes, run shell commands, start agents, read pane output and even pair a new device. The internal write-up called it critical. The public advisory has severity **High**.
- The fix (released as 0.1.5 on 2026-10-15): a token is now needed for any request that has a forwarder header, a web address other than the local one, or a web page as origin. When the bridge cannot tell, it asks for the token. Tests for the tunnel path and the direct path failed before the fix and pass after.
- The advisory states that no use of the bug is known. This is what was known when it was published. It is not proof.
- Later, 0.2.0 also added the device abilities, the 30-day expiry and the local secret below.

**Local secret.** At each start the app makes a random key (32 random bytes, in hex) and saves it in a file named `puppet-master.bridge.secret` next to the bridge port file in the app data folder. The app window and the MCP program send it in the header `X-PM-Local-Secret`. Today, a local caller without the key is only **logged** (once for each route every 10 minutes) and still served. The plan is to refuse such calls later.

**CORS.** CORS is the browser rule about which web pages may call a service. The bridge no longer answers "any page" (`*`). It names only the app's own origins.

**Audit log.** A tamper-evident list of approvals and control changes is in Settings › General › Safety, with a "log checked" line.

## Limits and what is not done yet
- A new phone gets **Full control** by default today. The roadmap suggests Watch and Answer. This default needs the founder's decision.
- The local secret is not enforced. Any program on your PC that does not send the key can still call the bridge as the local app.
- A phone that only streams, with no other request for 30 days, is signed out. Phones normally make a request every 25 seconds.
- Real iPhone and Android phones, real push and a real Cloudflare tunnel were not tested. The phone card was tested in a phone-sized browser.
- If you point your own tunnel at the bridge port, put it on the app's phone web server instead.
- The 0.1.5 release has only the fix. It does not include device abilities. Those come with 0.2.x.

## For developers
- Pairing and abilities: `packages/app/src-tauri/src/mobile_pairing/` (`caps.rs` with `route_need`, `auth.rs` with `requires_pairing_token`, `local_secret.rs`, `mod.rs`).
- Gate: `bridge/http.rs` (`read_request`, `gate`), CORS in `bridge/http.rs` and `bridge/sse.rs`. Per-device event filtering: `bridge/sse.rs` and `caps::project`.
- Error codes: `DEVICE_NOT_ALLOWED` (403), `device_expired` (401), `LOCAL_SECRET_MISSING` (log only).
- Bridge ports: 17321 to 17399 on loopback. Pairing state lives in the app data folder.
- UI: `components/settings/blocks/PairedDevices.tsx`, `components/MobilePairingPanel.tsx`.
- Write-ups: `docs/security/2026-10-14-tunnel-auth-bypass.md` and `2026-10-14-postmortem-public.md`.

## Related
- [Loop guard and panic stop](27-loop-guard-and-panic-stop.md)
- [Flight recorder and diagnostics](28-flight-recorder-and-diagnostics.md)
- [Review, merge and pull requests](23-review-merge-and-pull-requests.md)
