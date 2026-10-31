# Potato Bot (Preview)

Potato Bot is a chat friend that lives in Puppet Master. You talk to it in a page of the desktop app. It has a mood, a
memory and a voice (Fish or Potato), and its own small brain. When you ask it to do real work on your computer it does not do
it: it asks you to start a **helper**, and nothing starts until you press **Start**.

It is off by default. Turn it on in **Settings > General > Labs > Potato Bot** (the setting is `potato_bot_enabled`). While it
is off there is no sidebar row, no palette entry, no page and no Settings section, and no brain can run.

## How to set it up

1. **Turn it on.** Settings > General > Labs > Potato Bot. A new section, **Potato Bot**, appears in Settings.
2. **Pick the talk brain** (Settings > Potato Bot > Talk brain):
   - **DeepSeek** (the default): pays DeepSeek per message, usually cents a day. It needs your DeepSeek key.
   - **OpenCode Go**: uses your OpenCode Go sign-in.
   - **Through my agents**: no extra cost. The chat is the companion's own chat; mood and personas are not available.
3. **Add your DeepSeek key** (Settings > Potato Bot > DeepSeek key). It is kept in your system keychain. The page only ever shows
   "set" and the last 4 characters (`•••• 1a2b`). **Check key** sends one tiny message and says `Works.` or one plain sentence
   about what is wrong.
4. **Pick the default persona** (Fish or Potato). You can change a bot's voice at any time in its chat; its name, thread, memory and
   mood stay.
5. **Set the daily limit** ($0.05 to $20, default $0.50). It is per bot and covers everything it says.
6. **Make a bot.** A bot is a companion with a brain. Open **Potato Bot** in the sidebar and press `Make <name> a bot` on a companion,
   or `New bot` to make a companion first.
7. **Node.js 20 or newer** must be on this computer. If it is not, the chat shows `Potato Bot needs Node.js 20 or newer.` with an
   **Install Node.js** button. It opens a visible PowerShell window that runs the same `winget` install the agent installer
   uses for its `node` tool; you answer the agreements there.

## Helpers: asking is not doing

When you write something like "add a readme to my project", the bot may ask for a helper. A card appears in the chat:

> Fish wants a helper: add a readme  
> Folder: demo-a  
> Agent: <the companion's agent>  
> **Start** · **Change folder** · **Not now**

- **Start** runs the bot's own agent in that folder, as a helper of that companion (the same rules as any companion helper:
  a 20-turn budget unless you set your own, never the companion's home folder, one line in the companion's feed).
- **Change folder** lists the projects you used recently, by name. Pick one, then press Start.
- **Not now** closes the card. The bot is told once and says so.

The folder is always matched **by name** against your recent projects. A path the bot writes is never used. If the bot named no
project the card proposes your most recent project; if it named one that is not in your list the card says `not chosen yet` and
you choose. Your home folder, a drive root, Desktop, Documents and the like, a folder that only holds other projects, and the
companion's own home folder are refused with a plain sentence.

When the helper ends, a row appears in the chat, `Helper finished in demo-a.` with an **Open** button for its pane
(`Helper could not finish in ...` or `Helper stopped in ...` when it did not finish). The bot gets one message about it
(including a short redacted summary), which gives it one turn to answer you. If the bot is paused or over its limit, the row still
appears and the bot is simply not asked. A helper's own questions and approvals go to your Inbox like any agent's; the bot never
answers them.

## Safety

- **Nothing starts without Start.** A helper request from anyone but you at this computer is refused with no card.
- **Caps.** At most 3 open helper requests per bot (a fourth is declined) and 10 helper starts per bot per day.
- **The key** lives only in the system keychain and in the brain's environment. It never reaches the page, the phone, a log or a
  command line. Settings are stored by the app in its own `potato/settings.json`, never in the settings file a phone can patch.
- **The brain cannot reach anything else.** It talks to the app over two local routes and has no admin powers; the token it is
  given works nowhere else.
- **Kill switch.** Turning Potato Bot off stops every brain, closes the routes and takes back open helper requests. The Stop
  button on a bot's page stops that one brain.
- **Redaction.** Text going to the brain, into the saved chat, onto the page and into helper summaries has secrets and your home
  path taken out.
- **No window.** The brain starts hidden; a helper opens its pane in the cockpit grid like any agent run you start.

## Limits

- The daily limit is judged by Puppet Master from the token counts the brain reports, with its own price table (an unknown model is
  charged at the highest price). At 80% a note appears; at 100% the bot stops until local midnight or until you raise the limit.
- 20 messages per minute per bot; 600 reply tokens (1500 with reasoning); at most 4 tool rounds per turn.
- Phase 1 has one home, the desktop app. A chat app (Discord, Telegram) is a later phase.

## Where it is in the code

Rust core and helpers: `packages/app/src-tauri/src/potato/` (`helpers.rs` is Start / Change folder / Not now, the caps and the
watch of a running helper); commands `commands/potato.rs`; the helper's run uses `agent_runs::run_agent` with the Rust-only field
`on_behalf_of_companion`. UI: `components/cockpit/potato/`, Settings section `components/settings/sections/PotatoSection.tsx`.
The brain: `packages/potato-brain/`.

Tests: `cargo test --lib potato`; `npx vitest run src/components/cockpit/potato src/components/settings`; e2e
`potato.dm-reply` and `potato.dm-reply-and-helper` (`npm run e2e -- --only "potato.*" --no-video --headless`).
