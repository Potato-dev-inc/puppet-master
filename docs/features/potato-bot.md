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


## The Computer panel (Preview, needs the sandbox)

With both Labs switches on (`potato_bot_enabled` and `potato_sandbox_enabled`) a bot's chat has a **Computer** bar under its header. With
either switch off the bar is not drawn and every screen route answers 404.

- The words say who has the screen: `Fish is using it`, `You have control`, `Needs you`, or `Not in use`.
- **Show** opens the bot's screen (view only). **Take over** gives you the mouse and keyboard; **Hand back** gives the screen back;
  **Skip** turns a `Needs you` card down. **Retry** appears when the screen could not connect.
- A link to the screen lasts one hour, dies when the sandbox restarts, and a control link closes the second you lose the screen.
- The screen password never shows on the page. It travels only in the address of the frame, after a `#`, which no server or log sees.
- A paired phone can watch (view). Control from a phone needs your yes on this PC first; that yes is not built yet, so a phone gets
  `PHONE_NEEDS_DESKTOP_YES`.

Code: `potato/sandbox/screen.rs`, `bridge/routes/potato_screen.rs`, `components/cockpit/potato/ComputerPanel.tsx`.

## The bot's computer (preview, off by default)

A bot can have a small Linux computer of its own, a sandbox (a WSL 2 distro the app makes), with a screen, a mouse and a keyboard. It is away from your files. It needs two Labs switches: **Potato Bot** and **Sandbox** (`potato_sandbox_enabled`). Until a sandbox is set up and both are on, no bot is told it has a computer.

The bot gets five tools: `computer_observe` and `computer_act` (up to 24 ordered actions: click, move, drag, type, key, scroll, wait, focus an app), and `browser_navigate`, `browser_snapshot` and `browser_act` (the page browser: web addresses and element refs, up to 24 actions). Puppet Master checks every call; the brain only asks.

- **Protocol (v1, version safe).** A `send` item carries `capabilities: {sandbox: true|false}`. The brain offers the tools only on a turn that says true. A call is the brain event `sandbox_call {callId, conversationId, tool, args, image}`; the answer is the inbox item `sandbox_result {callId, ok, result | error {code, message, needsYou?}}`. An older brain ignores the field and the item; an older app ignores the event. The trust of a call is the one Puppet Master recorded when it sent the turn; the call cannot claim another.
- **Who may use it.** The person (at the PC or paired) may use the computer and local helpers. Anyone else may use ONLY the computer, never a helper (`rules::tool_access`).
- **The screen.** The bot holds the screen lease while it works (15 minutes from its last use, ended when it answers). If the person has the screen the call is `LEASE_HELD_BY_PERSON` (with `needsYou: true`: the bot says it is waiting). If the person took over and handed back, the next call is `LEASE_LOST` once ("look again before you act").
- **Web addresses.** `http` and `https` only, no name or password in the address, and only public hosts: not `localhost`, not a private, loopback, link-local or shared address in any spelling, not a name with no dot, `.local`, `.internal` and the like. A page inside the sandbox can be opened only by a test or dev build.
- **No saved logins, cards or pay tokens** in the page tools.
- **Daily cap.** 500 actions per bot per day (a setting, `sandboxActionsPerDay`, 10 to 5000). A call that would pass the cap is refused whole. The count is kept next to the money in `usage/YYYY-MM.json`.
- **A model that cannot see** (DeepSeek, today's default) gets words: the page text of the front Chrome tab, or a short note with the front window and whether the screen changed. A model that can see (the table in `core/models.ts`, or `FISHD_VISION=on`) gets the picture.
- **What comes back is data.** The brain fences it, hides keys in it, cuts long results, trims old page state, and stops a turn that makes the same call six times in a row.

Code: `potato/sandbox_calls.rs` (the dispatcher), `potato/sandbox/browser.rs` (the page tool's arguments and the checks), `packages/potato-brain/src/core/sandbox-tools.ts` (the tools). Tests: `cargo test --lib potato`; `node --test test/sandbox-*.test.ts` in `packages/potato-brain`.
