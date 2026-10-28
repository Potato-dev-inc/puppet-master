# Live Room

Branch `feat/live-room-docs` (task LR-14, SPEC-n32). Code: `packages/app/src-tauri/src/live_room/`,
`commands/live_room.rs`, `packages/app/src/components/cockpit/live/`, `relay/protocol/src/room.rs`,
`relay/server/src/rooms.rs` and `rooms/routes.rs`, `relay/viewer/`.

**Status.** This branch has the app side, the axum relay and the viewer page. The Cloudflare Worker part
(LR-11, `relay/worker/src/room.ts`) is on branch `feat/live-room-worker` and is not in this branch yet. Until
it is merged, a Live Room runs on the axum binary (`relay/server`) only.

## What and why

A person starts a Live Room and gets a link. Anyone with the link can watch the chat of the panes they picked,
live, in a browser. Viewers see what each agent says and does. They cannot type, and they cannot see the terminal.

Secrets, names and folders are hidden. The link stops by itself: at the time the person picks, after 20 quiet
minutes, when the panes close, or when the person presses Stop.

The app never opens a port for this. It only makes outbound HTTPS calls to a relay. Viewers talk only to the
relay. No sign-in and no team are needed. The relay is the address in Settings › General › Live Room, or the
team relay when that is set.

## Design

| Piece | Where | What it does |
|---|---|---|
| Go live sheet | `components/cockpit/live/GoLiveSheet.tsx`, `LivePreview.tsx` | Pick panes, title, length and viewer limit. Shows the exact first snapshot viewers would get. |
| LIVE bar | `components/cockpit/live/LiveBar.tsx` | Always visible while live. Shows viewers, minutes left, Pause or Resume, Copy link, Stop. No close button. |
| Pane chip | `components/cockpit/live/LiveChip.tsx` | A LIVE chip in each shared pane's header. Pause, Resume, Stop. |
| Settings | `components/settings/blocks/LiveRoomSettings.tsx` | Relay address, default length, default viewer limit. |
| Commands | `commands/live_room.rs` | Preview, start, stop, pause, resume, status, copy link, config. Tauri commands only, not bridge routes and not MCP tools. |
| Service | `live_room/service.rs` | One room at a time. Runs the room on its own thread, `live-room`. Writes `active.json`. Stops the room on Stop, panic and app quit. |
| Publisher | `live_room/publisher.rs` | Reads the panes every second, builds frames, sends them, checks the stop rules. |
| View | `live_room/view.rs` | Turns chat items into viewer rows. Redacts, cuts paths, caps length. |
| Source | `live_room/source.rs` | Reads the panes' chat through the watchers. Holds each pane with a 90 s lease, renewed every 15 s. |
| Seal | `live_room/seal.rs` | AES-256-GCM per frame. Pads frames to fixed sizes. |
| Client | `live_room/client.rs` | Calls the relay, signed with the room key. Reads at most 4 KiB of any answer. |
| Config | `live_room/config.rs` | `live_room.json` in the app data folder. |
| Crash record | `live_room/active.rs` | `active.json` in the app data folder. Lets the next start end a room a crash left behind. |
| Relay types | `relay/protocol/src/room.rs` | Shared wire types, room id, read hash, frame checks. MIT. |
| Relay routes | `relay/server/src/rooms.rs`, `rooms/routes.rs` | In-memory rooms and the HTTP routes. See [`relay/README.md`](../../relay/README.md#live-rooms). |
| Viewer page | `relay/viewer/` | Plain TypeScript. Built to `dist/` (committed). Served by the relay at `/r/`. |

### The link

```
https://<relay>/r/#<room id>.<read token>.<content key>
```

- Everything secret is after the `#`. Browsers do not send that part to a server. So no request line, log or
  `Referer` carries it.
- The room id comes from a new Ed25519 key for each room. The app keeps the key. The relay stores the public part.
- The read token is 16 random bytes. The relay stores only its hash. A wrong token and an unknown room both get
  the same 404.
- The content key is 32 random bytes. It is used for AES-256-GCM. It is never sent to the relay and never written
  to disk.
- The app keeps the full link in memory only. The Go live sheet and the LIVE bar show a masked link. Copy link
  puts the full link on the clipboard.

### Encryption

Each frame is sealed with AES-256-GCM. The nonce is a per-room random part plus a counter, so it never repeats in
a room. Each frame's room, sequence number, kind and nonce are part of the authenticated data. A changed frame
does not open. Frames are padded to fixed sizes, so the relay sees only rough sizes.

The viewer opens frames with the browser's own WebCrypto. It drops any frame with a sequence number it has
already shown.

## What viewers see

Each text goes through the same clean-up before it is sent: secrets are masked, e-mail addresses are masked,
folder paths are cut to their last names, then the text is cut to its limit.

| Shown | Limit |
|---|---|
| Room title (default `Live Room`) | 60 characters |
| Pane: agent name, pane title, state (`working`, `idle`, `waiting`, `error`, `gone`) | 60 characters each |
| Pane line: the activity text, only while the pane is working or idle | 72 characters |
| Your messages (user chat) | 500 characters |
| Agent replies, as plain text (no markdown, no HTML, no links) | 1,500 characters |
| Tool calls: tool name, short summary, how many files, whether it writes | 160 characters for the summary |
| Tool results: ok or failed, and the number of output lines | the output text is never sent |
| Turn end: completed, interrupted or failed, and how long it took | none |
| Plan: up to 20 tasks, with their status | 160 characters per task |
| Notes: compacted, error and title notices. A pull request shows `Opened a pull request` and no link | 200 characters |

Pane ids are replaced by `p1`, `p2` and so on, in the order the person picked them. Item ids are a counter for the
room.

While a pane waits for the owner (a trust, permission, login or question card), its state is `waiting`. Its line
is always `Waiting for the owner`. Viewers never see the question, the command or the choices.

## What is never shared

- The terminal. Bytes and screen cells are never sent in this version.
- Tool inputs and tool outputs. A tool call shows its name and summary only.
- Cards: the question, the command, the choices and the risk of any trust, permission, login or `ask_user` card.
- Chat items read off the screen (for agents whose chat is read from the screen) while a card is open, and for
  2 seconds after it closes.
- Companion chats. Shell panes. Panes with no chat.
- Usage numbers, the model name and the CLI version.
- Absolute times. Viewers see the order of things, not clock times.
- Folder names, paths (only the file names), the home folder, account labels.
- Pane ids and item ids from the app.
- Anything paused. While a room is paused, new chat is read and thrown away. It is counted as "not shared".
- Anything the person did not pick. Panes that are not checked are not read.

Viewers also cannot send anything. The app reads nothing from the relay except two numbers: the viewer count and the
end time. The bridge (the local HTTP server the agents and phone use) has no Live Room route.

## Privacy text

This is the text for this page and the settings help. The settings screen shows a shorter line for now (see
Known gaps).

> A Live Room sends the chat of the panes you pick, with secrets, names and folders hidden, to your relay. It is
> encrypted on this PC; only people with the link can read it. Viewers can't type, can't see your terminal and
> can't see your questions. It stops when you press Stop, when time is up, or after 20 quiet minutes.

## Stopping

| Reason | When | What the person sees |
|---|---|---|
| Owner | Stop in the LIVE bar, the pane chip, the palette, or Panic | `Live Room stopped.` |
| Expired | The time the person picked is up | `Live Room ended: time is up.` |
| Idle | No new chat and no state change in any shared pane for 20 minutes | `Live Room stopped: nothing happened for 20 minutes.` |
| All panes closed | The last shared pane closed | `Live Room stopped: its panes are closed.` |
| Relay lost | Three failed relay calls in a row, over at least 30 seconds | `Live Room stopped here. The link ends by itself at <time>.` |
| Relay ended | The relay says the room is gone (for example, the relay restarted) | `Live Room ended by the relay.` |
| Clock jump | More than 5 minutes between two ticks (sleep, hibernate) | `Live Room stopped while this PC was asleep.` |
| App quit | The app exits | No message. The room is deleted on a best-effort basis (2 seconds). |

Stop wipes the room on the relay at once. The relay keeps the room's id as a tombstone, so the same id cannot be
registered again while the tombstone lasts. Each room has a new random key, so a stopped room's key is never used
again.

**Crash safety.** When a room starts, the app writes `active.json` in its data folder. It holds the relay address,
the room id, the room's signing key and the end time. It does not hold the content key or the read token. At the
next start, the app asks the relay to delete that room and removes the file. If the relay cannot be reached, the
file stays for the next start. A clean stop removes the file at once.

## Limits

| Limit | Value |
|---|---|
| Panes in one room | 6 |
| Rooms at once in the app | 1 |
| Room length (the person picks) | 30 min, 1 h (default), 2 h, 4 h, 8 h |
| Most viewers (the person picks) | 5, 10 (default), 25, 50 |
| Longest room the relay accepts | 8 hours |
| Idle stop | 20 minutes with nothing new |
| Snapshot | Sent at start, on resume, and at least every 60 seconds. Up to 60 rows per pane, at most 60 KiB |
| Delta frames | Up to 40 rows and 12 KiB each. At most 2 per second |
| Frame size on the relay | 24 KiB for a delta or state frame. 96 KiB for a snapshot |
| Owner frames per room on the relay | A burst of 10, then 2 a second. 7,200 a day |
| Frames kept per room on the relay | The newest 300 deltas or 2 MiB. Older ones are dropped |
| Room registrations per address | 10 an hour |
| Rooms open on one relay | 100 (more is refused with 503) |
| Room bytes on one relay | 256 MiB in all |
| Viewer calls per address | 120 a minute |
| Seats per address in one room | 4 |
| A viewer seat with no call | Frees its place after 40 seconds |
| Long poll | Up to 25 seconds |
| Tombstone (a stopped or ended room) | Answers 410 until 1 hour after its planned end, then 404 |

## Known gaps

- **"Go live…" does nothing in a pane window outside the cockpit.** The sheet opens only in the cockpit window. A
  pop-out pane window (the terminal window) runs a different app root. Its menu item fires an event that only the
  cockpit listens to, so nothing happens. Start the room from the cockpit window.
- **A secret registered after the room starts is not masked in that room.** The redaction list is taken at start.
  A new secret is masked in the next room only.
- **Free text is checked by patterns, not by meaning.** An unknown secret shape, a private project name or a
  branch name inside a sentence can get through. The Go live preview shows the first snapshot. Pause throws
  chat away while you check. Tool inputs and outputs are never sent.
- **Names are not removed from chat text by a rule.** Account labels and folder names are removed. A person's
  name in a message is not.
- **The Worker is not in this branch.** The Cloudflare Worker's Live Room routes and Durable Objects are in LR-11
  (`feat/live-room-worker`). Its tests run in a local Worker runtime (`wrangler dev`).
- **Worker tests do not cover two things.** The tombstone alarm that wipes a room's rows is not tested. A quiet
  viewer seat freeing its place after 40 seconds is not tested. The axum relay tests cover both.
- **The settings help line is short.** It says `Rooms are end-to-end encrypted. The relay sees only sizes and
  times.` It does not give the full privacy text above.
- **One room at a time, and the pane list is fixed.** A helper started after the room begins is not added.
- **Viewers see no terminal, no links and no markdown.** The page is read-only.
- **The viewer page is served by the relay.** A relay you do not trust could serve other page code (see the
  caveat below).

## A malicious relay

The relay serves the viewer page, so the relay also serves the code that reads the link's `#` part and decrypts
the chat. A relay that wanted to read a room could serve code that sends the key or the decrypted chat to
someone else. Viewers would not see any sign of it. The content key never reaches the relay through the normal
calls, but the viewer's own code does get the key.

So pick a relay you trust: your own, or Puppet Master's hosted relay when it exists. Pinning the viewer's bundle
hash in the app is planned for the hosted version. It is not in this build.

A relay can still do these things, whatever it is: see the room id, frame count, padded sizes, times, the viewer
count and the client addresses (the same as any web host sees). It can drop, delay or stop a room. It cannot
forge or change a frame without the key. The viewer drops frames that come out of order or repeat.

## How to test

### Automated

```sh
# App side (Rust). Run from packages/app/src-tauri, with your own CARGO_TARGET_DIR.
cargo test --lib live_room

# Relay: protocol unit tests, and the server's room tests over loopback HTTP.
cd relay
cargo test -p pm-relay-protocol
cargo test -p pm-relay --test rooms

# Viewer page (vanilla TypeScript).
cd relay/viewer
npm test
npm run typecheck

# Cockpit UI and the settings block (from packages/app).
npx vitest run src/components/cockpit/live src/components/settings/blocks/LiveRoomSettings.test.tsx src/lib/live-room.test.ts
npx tsc -p tsconfig.json --noEmit
```

What the Rust tests check:

- `live_room/view_tests.rs`: the privacy matrix. A planted env secret, an `sk-ant-…` style key, an e-mail, a
  user-folder path and a long opaque run are put in every chat item kind. None may appear in the frame JSON. Also:
  tool inputs and outputs are never sent, caps hold, and a waiting pane shows only the fixed line.
- `live_room/publisher_tests.rs`: each stop reason at its threshold, pause, screen hold-back, frame size limits,
  and no secret in frame plaintext.
- `live_room/guard_tests.rs`: no bridge, phone, tool or tunnel code knows about rooms. The tool catalog version is
  unchanged. The commands refuse any window that is not the cockpit or main (or settings, for settings).
- `live_room/seal.rs` tests: round trip for each frame kind, bucket sizes, nonces never repeat, a changed frame
  does not open.
- `relay/server/tests/rooms.rs`: register, snapshot, frames, viewer reads, wrong token gives 404, viewers cannot
  write, max viewers, seat expiry, stop and expiry answers, rate limits, the viewer page's headers.

### By hand (on one PC)

Use a relay you run yourself. Do not use a deployed relay for a test.

1. Start the relay on loopback: `cargo run --release -p pm-relay -- --listen 127.0.0.1:8787` (from `relay/`).
2. In Settings › General › Live Room, set the relay to `http://127.0.0.1:8787`. The app allows `http://` only on
   this computer.
3. Open a chat pane for an agent. In the chat header menu, pick `Go live…`. Check the preview: no secret, no e-mail,
   no folder name.
4. Press `Go live`. Press `Copy link`. Open the link in a browser on the same PC.
5. The viewer should show the chat within a few seconds. Check that there is no input box.
6. Press `Pause` in the LIVE bar. The viewer shows `Paused by the owner`. Press `Resume`. The viewer shows
   `Some steps were not shared`.
7. Press `Stop`. The viewer shows `This Live Room has ended` within one poll.
8. For a permission card: ask the agent for something that needs approval. The viewer shows
   `Waiting for the owner` and not the command. Do not answer the card for the viewer.

The viewer and the relay do not write a log line per viewer request. Check that in the relay's output.
