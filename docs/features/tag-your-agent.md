# Tag Your Agent (Telegram groups)

Code: `packages/app/src-tauri/src/channels/engine/group.rs` (the group path), `channels/tag_config.rs` (saved settings),
`channels/tag_prompt.rs` (group text as fenced data), `channels/tag_log.rs` (kept answers), `channels/setup_tag.rs` (the
person's actions), `channels/host_live.rs` (`ask_group`). Tauri commands `telegram_tag_*` in `commands/channels.rs`.
UI `components/settings/blocks/TelegramTagBot.tsx`, `lib/telegram-api.ts`.

Status: **phase 1, early**. Built and tested with a fake bot. Not run live with a real bot or a real group yet.

## What and why

Someone tags your bot in a Telegram group you approved ("@mybot is this true?"), or replies to one of the bot's
messages. The bot answers in the thread. The answer comes from **your own agent on your own PC**, through the same
read-only one-shot ask the Ask bar and the side question use. Nothing is written, no pane is touched, and no command or
approval card is ever handled from a group.

It is a Grok-style bot, but with your agents, running locally.

It is **off by default**. With the switch off, groups are dropped exactly as before this feature existed.

## Read this first

- Telegram can read every message in the group and every answer the bot posts. See the warning in `telegram.md`.
- The agent can **read** files, and anything other people trigger must run in a sandbox, which does not exist yet. So for
  now **a group open to "anyone" never gets a project folder**: it always uses the empty `tag-bot` folder. This is
  enforced four times: the settings file drops a folder from an "anyone" group on load and on save (and logs why), the
  setup commands refuse the combination, the Settings page disables the folder field, and the ask path checks again
  when it runs. Only a group where **only you** may tag can have a project folder.
- A question asked with a project folder shows up in **that project's Chat history** (the headless agent writes a session
  file for its folder), and the quick-ask path writes **operation snapshots** under `<project>/.puppet-master/`. So it
  is not "nothing is written" for such a group. The empty `tag-bot` folder keeps all of this out of your projects.
- Keep Telegram's **privacy mode on** (the BotFather default). The bot then only sees messages that tag it.
- Your PC must be awake and Puppet Master open. When it is not, the bot is silent, and messages that queued up for more
  than 10 minutes are dropped when it wakes.

## How to turn it on

1. Set up Telegram as in `telegram.md` (token, connect your chat, Telegram on).
2. Settings > Phone > Telegram > **Tag Your Agent in groups**: switch it on.
3. Add the bot to a group, then tag it once (`@yourbot hello`). The group appears under **Groups waiting for your yes**.
   The bot says nothing there yet.
4. Press **Approve**. Now tags from you are answered.
5. Optional, per group: **Who may tag** (Only me / Anyone in the group), **Agent**, **Folder** (only when only you may tag), **Style note**.

## The rules

| Rule | Value |
|---|---|
| Switch `tagBot.enabled` | off by default |
| Trigger | a message entity of type `mention` equal to `@yourbot` (any case), or a reply to one of the bot's own messages. Nothing else in a group is read. |
| Not approved group that tags the bot | remembered as **pending** (id and title only, 10 at most, newest kept, 10 new groups an hour). Nothing is sent. |
| Who may tag | `only_me` (the connected Telegram user, default) or `anyone` |
| Per-person cap | `anyone` groups: 5 answers an hour for each person (setting) |
| Daily cap | 30 answers a day for the whole bot (setting). Over it: one "I've hit today's limit" reply per group per day, then silent. |
| At once | 2 questions at a time. A third gets one "I'm busy" reply a minute. |
| Old messages | older than 10 minutes, or from before Telegram was turned on: dropped |
| Bots, anonymous senders | dropped |
| Commands | **never**. A group message does not reach command parsing, pairing or the approval policy. "@bot /stop" is just a question. |
| Context | the tagged text and the replied-to text, each cut to 2000 characters, redacted, and fenced as data with `context_pack::fence` |
| Agent | chosen per group from `tui::read_only_backends()` (installed ones). Generic code never names an agent. |
| Folder | only for `only_me` groups: the group's folder, if you set one and it is a real folder that is not your home folder (or a parent of it). Never for an `anyone` group. Otherwise an empty folder `tag-bot` under app data. Never the home folder. |
| Reply | goes through `channels::render` (secrets, e-mail and paths masked, plain text, 1500 characters) with `reply_to_message_id`; "typing…" is shown while it thinks |
| Failure or timeout | the fixed sentence "Sorry, I couldn't answer that right now." No error text, no path |

## Where answers are kept

Each answer (or failure) is kept for the Settings page: group title, who asked (display name), the question, the answer
and the agent. The last 50 are kept in `tag-bot-log.json` in the app data folder (redacted and cut), and the Recent
answers list shows the newest 5. The audit log gets a counts-only line (`channel.message`, `scope: group`): never the text.

## What changed outside `channels/`

Nothing except `commands/channels.rs` and `lib.rs` (the seven `telegram_tag_*` commands) and the Settings row in
`PhoneSection.tsx`. The bridge has no route for this feature.

## Known limits

- The caps live in memory. A restart of Puppet Master starts them again.
- If you rename the bot's username in BotFather, press **Check** again with the token so the new name is saved. Until
  then a tag with the new name is not recognised.
- Ignored groups come back as pending if someone tags the bot there again.
- Phase 1 is Telegram only and text only: no images, no voice, no inline mode.
