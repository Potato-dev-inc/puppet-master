# Text your agents (Telegram)

Code: `packages/app/src-tauri/src/channels/` (the engine, policy, rendering and pairing, all generic), with
`channels/telegram/` (the Bot API client and the long-poll thread). Bridge route `bridge/routes/channels.rs`
(phone switch-off only). Tauri commands `commands/channels.rs`. UI `components/settings/blocks/TelegramSetup.tsx`,
`TelegramPairing.tsx`, `components/mobile/TelegramOffRow.tsx`, `lib/telegram-api.ts`, `lib/telegram-bridge.ts`.

Name: "Telegram" in the UI. "Text your agents" is the idea. The code is named `channels`, so another chat app can be added later.

Status: **early**. The engine, the Bot API client, pairing, the rules and the Tauri commands are in the tree and
tested with a fake bot. The Settings block and the phone row are written but **not placed on any screen yet**, so on this
branch you cannot finish the setup from the app. It has not been run live with a real bot.

## Read this first: Telegram can read your messages

Telegram is a third party. Everything the bot sends and receives is stored on Telegram's servers. It is **not
end-to-end encrypted**, so Telegram can read the text of every message.

Puppet Master hides secrets, e-mail addresses and folder paths before it sends anything. It does not hide the text
itself. Agent questions, agent replies, your commands and your answers reach Telegram as plain text.

Do not use this for work you must keep private.

## What and why

Text your agents from your phone when you are away. You can see what needs you, answer a question, or send a short
message to an agent. It is off by default. It talks to one chat, which is yours.

Starting and stopping agents, and changing settings, stay on the PC.

## What Telegram can see

| Sent to Telegram | Never sent |
|---|---|
| Cards that need you: the question (up to 600 characters), the approval text with its risk word, and the buttons | Terminal bytes or screens |
| Agent replies in a conversation you opened from Telegram (cleaned, up to 1,500 characters each) | File contents and raw diffs |
| "Run finished" or "run failed", only if you turn on **Tell me when a run finishes** | Full folder paths (a path is cut to its last name) |
| Pane titles, agent names and status words, when you send `/panes` | Pane ids and other internal ids |
| Your commands, your text to agents, your taps on buttons | The bot token |
| The one-time pairing code, when you type it in the chat | Your settings file |

Telegram also knows things Puppet Master does not send: your chat id, your Telegram name and username, the bot's name,
and the time of each message.

**Before anything is sent, it is cleaned:** bidi and control characters are removed, known secrets are masked, e-mail
addresses are masked, paths are cut to their last names, and the text is cut to 1,500 characters. Messages are plain
text (no bold, no links). Link previews are off, so Telegram does not open links from your agents. Messages are sent
with "protect content", which asks Telegram's apps not to forward or save them. That is a request, not a guarantee.

**Short messages only** (off by default) sends no agent text at all. It only says that something needs you.

**What cleaning cannot catch:** a secret with no known shape, such as a short password an agent prints. Puppet Master
cannot know it is secret.

## Setup

These are the steps the Settings block will show. The block is not on screen yet (see Status).

1. **Make a bot.** In Telegram, open @BotFather, send `/newbot`, pick a name and a username that ends in `bot`, and copy
   the token.
2. **Check the token.** Puppet Master asks Telegram who the bot is. The token goes to the Windows Credential Manager
   (or the macOS Keychain or Linux keyutils). It never goes to a file. If the store cannot be used, saving is refused.
   If the bot already sends its messages to a website (a webhook), Puppet Master refuses it. Make a new bot for it.
   Puppet Master never deletes a webhook on your bot.
3. **Make a code.** Puppet Master shows an 8-character code for 10 minutes. In Telegram, open `@your_bot` and send
   `/start CODE`. The code is typed in the chat, never put in a link. Five wrong codes burn it, and so does **Not me**.
4. **Confirm on the PC.** The PC shows the Telegram name of the person who sent the code. Click **Yes, that's me**.
5. **Turn Telegram on.** After pairing it stays off until you turn it on. Then pick what it may do:
   **See** (watch), **Answer** (answer) and **Type** (send text to agents). Control is never allowed. The default is
   See, Answer and Type.
6. **Options.** **Tell me when a run finishes** and **Short messages only** are both off by default.

Turn it off from the PC, send `/stop` in the chat, or (from a paired phone) use **Turn Telegram off**. A phone can only
turn it off. **Disconnect** forgets the token and the chat, after asking you first.

## What the bot does

| Message | What happens |
|---|---|
| `/status` | Lists what needs you |
| `/panes` | Numbered list of your agents: heading, agent, status |
| `/to N your text` | Sends text to agent N from the last `/panes` list |
| `/c Name your text` | Talks to a companion |
| `/help` | Lists the commands |
| `/stop` | Turns Telegram off |
| Plain text | Goes to the agent or companion that the message replies to. Otherwise, to the one companion you have. With several, the bot asks which one. |

Agent text you send is limited to 1,000 characters. Control characters are removed, except line breaks and tabs.
A companion that is asleep gets "Mochi is asleep. Wake it in Puppet Master." Waking a companion is never done from chat.

Replies from your agents reach Telegram only inside a **conversation window**: a window you opened by writing to that
agent from Telegram. It lasts 15 minutes. A reply or a turn end in a conversation that you only had on the PC does not
reach Telegram. Two other things still come through when they happen: cards that need you, and "run finished" notices
if you turned them on.

## What you can answer from chat

| Card | From Telegram |
|---|---|
| An agent's question | Yes. Its choices are buttons. Free text is a reply, where allowed. |
| Approval, low risk | **Allow once**, then a confirm tap. **Deny** is one tap. No "don't ask again". |
| Approval, medium or high risk | No. Open Puppet Master. |
| Folder trust, or a sign-in screen | No. Only you, on the PC. |
| An agent asks for more control | No. Decide on the PC. |
| Budget spent, or usage limit | No. Open Puppet Master. |
| Info cards (done, stuck, error, review, setup) | Shown as text only, no buttons. |

This is enforced twice: in the Telegram layer (`channels/policy.rs`), and again inside `attention::answer` for answers
that come from Telegram. Each button is a single-use token that the bot looks up. Button data means nothing by itself.
An old or used button does nothing.

## Who may use it

- One paired chat and one Telegram user, in a private chat. Groups and channels are ignored.
- Strangers get **no reply**. The bot looks dead to them. Each one is counted, and an audit entry is written at most once
  an hour. The Settings block warns you at 50 strangers an hour.
- Messages sent before you turned Telegram on, or older than 10 minutes, are never acted on. One note says how many were skipped.
- Rate limits: 20 messages a minute in, 10 answers a minute, 20 messages a minute out, and one "slow down" note a minute.
  A second alert for the same item within 30 seconds is held, then summarised.
- If Telegram says another program is using the token (409), the poller stops. It starts again only when you act.
  The Settings block then says: make a new token in BotFather.
- Turning on and off, pairing, strangers, errors and messages are written to the audit log. The audit detail holds counts
  and ids, never message text.

## Decisions D-1 to D-13

These are the conservative choices from the feature spec. "In code" means the rule is in the tree and tested.

| # | Decision | Where | Status |
|---|---|---|---|
| D-1 | Token only in the OS credential store, under `channels/<profile or default>/telegram_bot_token`. No file fallback. Saving is refused if the store is not usable. | `channels/token.rs` | In code |
| D-2 | Never call `deleteWebhook`. Refuse a bot that already has a webhook. | `channels/setup.rs` | In code |
| D-3 | No `?start=` link with the code in it. The code is typed as `/start CODE`. | `channels/pairing.rs`, `channels/engine/pair.rs` | In code. The code still reaches Telegram as chat text. |
| D-4 | Pairing needs the code and a click on the PC. | `channels/pairing.rs` (`decide`) | In code |
| D-5 | Off after pairing. The person turns it on. | `channels/setup.rs` (`set_enabled`) | In code |
| D-6 | Plain text goes to one companion, or the bot asks which. Sleeping companions are reported. Waking is never from chat. | `channels/engine/run.rs` | In code |
| D-7 | Text to agents: 1 to 1,000 characters, control characters removed. Needs `type`. | `channels/commands.rs`, `channels/engine.rs` | In code |
| D-8 | Agent replies and turn ends reach Telegram only in a conversation the person opened from Telegram (15 minutes). A conversation that only happened on the PC does not leak. | `channels/refs.rs`, `channels/notify.rs` | In code |
| D-9 | Trust, consent, budget and limit cards are never answered from chat. Approvals only at low risk, with a confirm tap. Checked twice. | `channels/policy.rs`, `attention/answer.rs` | In code |
| D-10 | PC asleep or app closed: nothing answers. Old messages are skipped on start. Telegram keeps undelivered messages for a time that Telegram sets. | `channels/engine.rs` | In code |
| D-11 | Token used by another program (409): stop, show the conflict, ask for a new token. Never fight for updates. | `channels/telegram/poller.rs`, `channels/runtime.rs` | In code. The sentence is in the Settings block, which is not on screen yet. |
| D-12 | A paired phone can only turn Telegram off, with its own token. No route turns it on, pairs a chat, or changes what it may do. | `bridge/routes/channels.rs`, `channels/setup.rs` (`turn_off_from_phone`) | In code. The phone row is not on the phone yet. |
| D-13 | No MCP tool. Agents cannot send to Telegram. | `tool_registry/` (no reference to channels) | In code |

## Files

- `channels/mod.rs`: the layout of the module.
- `channels/config.rs`: the saved settings file (`telegram.json`): on/off, caps, options, last update id.
- `channels/token.rs`: the token in the credential store.
- `channels/policy.rs`: which cards may be answered from chat, and which capability each command needs.
- `channels/render.rs`: the one cleaning step for everything sent out.
- `channels/grants.rs`, `channels/rate.rs`, `channels/refs.rs`: button tokens, rate buckets, numbered lists and conversation windows.
- `channels/commands.rs`: reads the text the person sends. `channels/engine.rs` and `channels/engine/`: the rules for one batch of updates.
- `channels/pairing.rs`: the one-time code and the desktop click.
- `channels/notify.rs`, `channels/feed.rs`: what the chat hears about.
- `channels/telegram/`: the Bot API client (`api.rs`) and the long poll (`poller.rs`).
- `channels/runtime.rs`: owns the engine and the poller. `channels/setup.rs`: the person's actions. `channels/status.rs`: the status the Settings block shows.
- `channels/host_live.rs`, `channels/live.rs`: the real connection to the app. `channels/test_host.rs`, `channels/fake_bot.rs`: test-only fakes.

## Tests

- Rust: `cargo test --lib channels`. Every test uses fake hosts and a fake bot. No test talks to Telegram.
- Rust, the desktop guard for chat answers: `cargo test --lib attention::answer`.
- UI: `cd packages/app; npx vitest run src/lib/telegram-api src/lib/telegram-bridge src/components/settings/blocks/TelegramSetup src/components/mobile/TelegramOffRow`.
- Live case, when the Settings block is on screen: see `docs/TEST-PLAN.md` section 2.10.

## Not done yet

- **The Settings block and the phone row are not on any screen.** Nothing imports `TelegramSetup`, `TelegramPairing` or
  `TelegramOffRow`. The Tauri commands work, but the app has no place to call them from.
- **No live run.** No real bot has been paired. The fake Telegram server for the live check is not in the repo.
- **Retention on Telegram's side** (how long Telegram keeps undelivered messages) is Telegram's rule. It is not checked here.
- Discord and other chat apps are not built. The `channels` folder is ready for them.
