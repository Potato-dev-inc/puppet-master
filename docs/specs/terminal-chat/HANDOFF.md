# Terminal chat: a chat on top of a real terminal agent (the "session-file lane")

Status: plan, ready for an implementing agent. Prepared 2026-11-02.
Owner decisions still open are listed in section 9 with a default for each; start with the defaults.

## 1. Goal

Give any agent a **real chat** without ACP: the agent runs normally in a terminal pane (all its own tools,
permission prompts, memory, resume), and the chat shows its conversation from the agent's **session file**
and sends messages by typing into that terminal. To the person it looks and behaves like a Chat (ACP) session
(tool cards, diffs, plans, history, stop, hand-over to the terminal), with no adapter to install.

It slots into "Add chat" (`acp_overlay_resolve`) as a fourth route, ahead of the headless bridge:

1. native ACP (agent's own help) 2. official ACP registry adapter 3. **terminal chat (this plan)** 4. headless bridge.

## 2. What already exists (do not rebuild)

The Chat tab of an ordinary terminal pane is already most of this:

| Piece | Where | What it does today |
|---|---|---|
| Read side | `session_chat/` (`opening.rs`, `source.rs`, `tail.rs`, `watch.rs`, `store.rs`, `items.rs`, `declared/`, `screen/`, `screen_first.rs`) | follows the agent's session file (or the screen until the file appears) into `ChatItem`s: messages, `ToolCall` + `ToolResult` (with `detail`, diffs), `Plan`, `Notice`, usage. Built-in readers: `tui/claude/session_file.rs`, `tui/codex/session_file.rs`, `tui/opencode/transcript.rs`; any TOML agent with `[session_chat]` gets `declared/` |
| Write side | `pane_outbox/` + `commands/pane_outbox.rs` + `components/chat/ChatComposer.tsx` | the Chat tab's message box: queues a message and types it into the terminal only when `readiness::assess` says Ready, the input box is empty and the pane is idle; never overwrites typed text (`draft_guard.rs`); verifies after delivery (`confirm.rs`) |
| Questions | `components/chat/ScreenQuestionCard.tsx`, `session_chat/screen_first.rs` | shows "the agent is asking something" read off the screen, with **Open terminal** only (it does not answer) |
| Prompt parsing | `tui/claude/prompt.rs` + `prompt/question.rs`, `tui/codex/prompt.rs`; MCP `answer_prompt` (`agent_runs/routes/answer.rs`) | the app can already recognise a terminal permission/question prompt, list its options and press the right key for an agent-driven run |
| Old chats | `session_chat/history/`, `chat_index/` | the same readers, for sessions that are no longer open |
| Lane + conversation records | `acp/lane.rs`, `acp/conversations.rs`, `pty/registry/virtual_pane.rs`, `PaneInfo.lane` | per-agent "How to run", saved conversations, hand-over to a terminal and back |
| Add chat | `commands/acp_overlay.rs` (`acp_overlay_resolve/install/set/remove`), `AddChatDialog.tsx` | the route list this plan joins |
| Wizard transcript finder | `tui/declarative/wizard/transcript/` (SPEC 11.23) | writes `[session_chat]` for a new agent, so wizard-made agents become eligible automatically |

So the missing parts are small and mostly about **starting the pane as a chat, answering questions from the
chat, and making it a first-class lane**.

## 3. Design

### 3.1 A third lane: `terminal_chat`

- `RunLane` (acp/lane.rs) today: `Terminal | Acp`. Add `TerminalChat`: a normal PTY pane (not virtual) that
  opens on its **Chat tab** and is shown with a "Chat (terminal)" badge. Terminal tab stays one click away and
  is always valid (there is no hand-over: it already is the terminal).
- Eligibility (`terminal_chat::available_for(agent) -> Result<TerminalChatInfo, String>`): the agent's profile
  answers `session_chat()` (or the definition has `[session_chat]`), it can be started in a pane, and it has an
  input box the outbox understands (`readiness`). `TerminalChatInfo { label, reads_file: bool, can_resume: bool,
  can_answer_prompts: bool, note }` where `can_answer_prompts` is true when the profile has a prompt parser.
- "Hidden" is a view setting, not a different pane: the pane exists in the pane list as usual (so the rest of
  the app, the phone, MCP, panic stop all see it); the chat is just the default tab. No window games.
- Persistence: reuse `acp/conversations.rs` records (id = pane id for chat panes) **only if** that stays simple;
  otherwise store nothing new, because the session file + `chat_index` already make the conversation durable and
  "Continue" (resume args from `session_chat/history` `resume_plan`) already restarts it. Decide in phase 1
  (default: no new record; the saved pane + resume plan are enough).

### 3.2 Starting it

- `terminal_chat_start(agent, cwd)` Tauri command (desktop only, no bridge/MCP): spawns the normal pane for the
  agent (same path as `NewAgentMenu` today, `registry.spawnPane`), marks it `lane = "terminal_chat"` in the pane
  registry so the UI picks the Chat tab (`lib/chatTab.ts` `requestChatTab` already exists), returns the pane id.
- `NewAgentMenu` already has the per-agent Terminal | Chat switch; add the third value where `chat.agents` says
  it is available. "How to run" rows (`AgentLaneRows.tsx`) get the same choice.
- `run_agent` (MCP, `acp/chat/surface.rs`): `lane: "terminal_chat"` is **not** exposed in v1 (terminal runs by
  agents already have their own, richer path). Keep the tool surface unchanged.

### 3.3 Sending

No new transport: the Chat tab composer already sends through `pane_outbox`. Only changes:
- the composer's `blockedReason` for this lane comes from `pane_outbox::target_block` as today;
- stop button = Esc/Ctrl+C through the existing interrupt path (`pty` input), same as the terminal pane;
- slash menu: none in v1 (the terminal agent has its own; typing `/` just types it).

### 3.4 Answering questions from the chat (the main new feature)

Today `ScreenQuestionCard` can only say "open the terminal". Add an answerable card for agents that have a
prompt parser:
- New backend read: `terminal_chat_prompt(pane) -> Option<PromptView { prompt_id, text, options:[{id,name}],
  kind }>` built from the same parser `answer_prompt` uses (`tui/<agent>/prompt*.rs`, via the profile trait; no
  agent names in generic code). Returns `None` when the screen shows no recognisable prompt.
- `terminal_chat_answer(pane, prompt_id, option_id)`: presses the key sequence that option needs (reuse the
  code behind `agent_runs/routes/answer.rs`, extracted into a shared function if it is private). Refuse if
  `prompt_id` is stale (screen changed): the card re-reads and asks again.
- Safety: **the person's click is the only thing that answers** (same rule as ACP cards: parked, no timeout, never
  auto-answered, no agent/MCP route; no bridge route). `kind` of "trust this folder" and "allow once/always"
  prompts are shown in plain words with the agent's own option texts; never pick a default.
- UI: `AnswerableQuestionCard` replacing the read-only one when `can_answer_prompts`; falls back to the old
  card (Open terminal) otherwise. Reuse the ACP permission card look (`AcpChatDock.tsx`).
- Phone: v1 shows the card read-only (as today); answering on the phone is phase 4 and needs its own review.

### 3.5 Wiring into Add chat

- New option kind `"terminal_chat"` in `acp_overlay_resolve` (between registry and bridge). Summary sentence:
  "Puppet Master runs <Agent> in a terminal and shows it as a chat. You get its tool cards and history, and you
  can answer its questions here. <It can continue the conversation later. | It cannot resume old sessions.>"
- `acp_overlay_install(agent, {kind:"terminal_chat"}, confirmed)` writes the choice (the lane value for that
  agent in `acp/lane.rs`'s store; **no overlay entry, no adapter**). `AddChatDialog.tsx` renders it like the
  others. Remove chat resets the lane to Terminal.
- Priority tweak: when an agent has `session_chat` and a prompt parser, `terminal_chat` outranks the headless
  bridge; the bridge stays for agents with no session reader.

### 3.6 Agents with no reader yet

The wizard transcript finder already infers `[session_chat]` during "Add with wizard". Add an
"Add chat" action for an **existing** TOML agent without `[session_chat]`: run the finder (it needs one real
visible turn, so it is an explicit "Teach it to read chats" flow, not silent). Out of scope for v1; list it as
phase 5 so the plan is complete.

## 4. Phases (each ends green and mergeable)

1. **Backend lane + start.** `RunLane::TerminalChat`, `terminal_chat::available_for`, `terminal_chat_start`,
   lane view fields, `PaneInfo.lane`. Tests with `pm-fake-tui` (the repo's fake terminal agent, see
   `tests/fake_tui_*.rs`): start opens a pane on the Chat tab, the fake writes a transcript, the chat shows it,
   a message sent through the outbox reaches the fake.
2. **UI start + lane choice.** `NewAgentMenu` three-way switch, `AgentLaneRows`, badge in `PaneView.tsx`, default
   tab. Vitest.
3. **Add chat route.** `acp_overlay_resolve` option + install/remove + dialog radio + wording. Rust + vitest.
4. **Answerable questions.** `terminal_chat_prompt` / `terminal_chat_answer`, card, stale-prompt handling, guard
   tests (generic code names no agent; no bridge/MCP route; only a person's call answers). Fake agent prompts for
   tests (extend `tools/pm-fake-tui`).
5. **Teach it to read chats** for existing agents without `[session_chat]` (wizard transcript finder reuse), and
   phone answering if the owner wants it.

## 5. Files to touch (anchors; re-check before editing)

- Rust: `acp/lane.rs`, `commands/acp_lane.rs`, `commands/acp_overlay.rs`, new `terminal_chat/` module (or
  `acp/terminal_chat.rs`; keep generic, no agent names, add to `acp/guard_tests.rs` file list), `tui/types.rs`
  (a `prompt_view`/`answer_prompt` trait method if the parsers are not already behind the profile trait),
  `agent_runs/routes/answer.rs` (extract the key-press part), `pty/registry/` (lane tag on a normal pane),
  `lib.rs` (register commands).
- TS: `lib/acpChat.ts`, `components/cockpit/NewAgentMenu.tsx`, `components/settings/blocks/AgentLaneRows.tsx`,
  `AddChatDialog.tsx`, `components/cockpit/PaneView.tsx`, `components/chat/ScreenQuestionCard.tsx` (+ a new
  answerable card), `lib/chatTab.ts`.
- Docs: `docs/REPO_MAP.md` 3.20 and the session_chat row, `docs/specs/agent-definitions/SPEC.md` (lane value).

## 6. Security and rules

- Answering a question or typing a message is always the person's own click (no timer, no auto-answer, no MCP
  or bridge route; same as ACP cards). `answer_prompt` for agents driving other agents stays as it is.
- Redact everything shown (`redact::json` path already used by the read side). Never log chat text.
- Generic code names no agent (`acp/guard_tests.rs`); agent facts live in `tui/<agent>/` or definitions.
- No new network access, no installs, no new dependencies.
- The pane is a normal pane: panic stop, approvals, worktree rules, the `no windows on primary screen` rule for
  agents, all keep applying unchanged.

## 7. Tests

- Rust: lane availability per profile (reader present/absent, prompt parser present/absent); start tags the
  pane; outbox delivery to `pm-fake-tui`; prompt view/answer round trip with a stale id refused; guard tests.
- e2e: `cargo test --features dev-fake-tui --test fake_tui_e2e` style test `terminal_chat.*` (see how
  `acp.chat-workspace` is wired in `packages/e2e`).
- Vitest: three-way lane switch, Add chat option and wording, answerable card (answer called with exact ids,
  stale refresh, fallback to read-only card), badge/default tab.
- Run before handoff: `cargo test --lib acp::`, `cargo test --lib commands::acp`, `cargo test --lib session_chat`,
  `cargo test --lib pane_outbox`, `npx tsc --noEmit -p .`, `npx vitest run src/components/chat
  src/components/cockpit src/components/settings src/lib`. Repo CI is manual-only (`D:/pm-night/local-ci.sh`).

## 8. Risks

- **Typing into a TUI** can race with the agent's own redraws; the outbox already handles ready/idle/empty-box
  checks and verifies delivery; keep using it and do not add a second path.
- **Prompt parsers drift** with agent versions (the Claude/Codex ones are tested against fixtures); when no
  prompt is recognised the old "Open terminal" card is the safe fallback, never a guess.
- **Two writers**: the person can type in the Terminal tab while the chat queues a message; `draft_guard`
  already refuses to type over text in the box.
- **Session file lag/format changes**: `screen_first.rs` already falls back to the screen until the file shows.
- **Resume**: only agents with `resume_args` (`session_chat/history/open.rs::resume_plan`) can continue a closed
  chat; others show "cannot resume" in the option text.

## 9. Open decisions (defaults to use unless the owner says otherwise)

1. Persist a conversation record for terminal chats? **No** (pane + session file + resume plan are enough).
2. Show the lane in "+" for every eligible agent? **Yes**, as the third value of the existing switch.
3. Phone answering of questions? **Not in v1** (read-only card on the phone).
4. Should `run_agent` (MCP) accept `lane: "terminal_chat"`? **No.**
5. Name shown to the person: "Chat (terminal)" **default**; avoid the word "ACP" for this lane.

## 10. Definition of done

- Start an eligible agent as Terminal chat: it opens on the Chat tab, shows history and tool cards from the
  session file, a typed message reaches the agent, Stop interrupts, a permission prompt appears as a card the
  person can answer, the Terminal tab shows the same live session.
- Add chat lists "terminal chat" for an eligible agent and the headless bridge only when it is the last route.
- All tests in section 7 pass, REPO_MAP updated, merged by the normal flow (`D:/pm-night/ship.sh`).
