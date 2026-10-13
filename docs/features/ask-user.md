# ask_user: agents ask the human a question

Branch `feat/ask-user`. One new MCP tool, `ask_user`, plus the questions store, bridge routes, Tauri commands,
cockpit Inbox cards and a phone list.

## 1. What and why

**Story.** An agent (or an orchestrator driving subagents) needs a decision only the person can give. Today it
stalls at a terminal prompt nobody sees. With `ask_user` it asks a structured question, the question shows up at
the top of the Inbox (desktop) and in the All tab of the phone PWA, and the answer returns as the tool result.

**Personas.** A developers (an orchestrator escalates a choice), B vibecoders (plain buttons instead of a
terminal), C non-technical (a clear question with a Yes/No, from their phone). It is also the foundation for
recipe "human gates".

**Agent-agnostic.** It is an MCP tool: any MCP-capable agent gets it. No `TuiProfile` or TOML change, and no
agent-name string is matched anywhere. The asker shows as the `agent` name the agent passes, else the pane name,
else "an agent".

## 2. Design

### Tool
`ask_user` (catalog entry in `tool_registry/catalog/questions.rs`, `POST /questions/ask`, `ReadOnly`, visible in
agent, shell and both modes).

| Input | Notes |
|---|---|
| `question` (required) | max 2000 chars |
| `choices` | `[{id,label}]`, max 6 |
| `allow_free_text` | default true when there are no choices, false otherwise |
| `context` | short text under the question, max 1000 chars |
| `timeout_s` | default 600, max 3600 |
| `urgency` | `normal` (default) or `high` |
| `agent`, `pane_id` | optional display name; optional pane to list it under |

Result: `{answered:true, choice_id?|text?, answered_by:"desktop"|"phone"}` or
`{answered:false, reason:"timeout"|"cancelled"}` (plus `question_id`). Dismissing a question, or the agent
aborting the call, is `cancelled`.

### Blocking pattern (same as `wait_agents`)
The shim (`bin/puppet_master_mcp/ask_user.rs`) does NOT hold one HTTP request open. The first
`POST /questions/ask` creates the question and blocks up to 4 s; the shim repeats `{question_id, wait_ms}` until
resolved, emitting `notifications/progress` between polls (when the client sent a progress token). Consequences:
- a bridge reconnect between polls loses nothing (`bridge_down` is retried until the deadline);
- cancelling the tool call (`REQUEST_CANCELLED`) sends `{question_id, cancel:true}` so the card leaves the Inbox;
- if the app restarted, the bridge answers `QUESTION_NOT_FOUND` and the shim reports `cancelled`.

**Persistence.** Questions live in memory: they survive an MCP/bridge reconnect while the app runs, and do not
survive an app restart (deliberate: an answer typed hours later for a vanished agent is worse than asking again).
Resolved questions are kept 10 minutes for idempotent re-answers.

### Rust
- `questions/mod.rs`: `QuestionStore` (mutex + condvar), validation, states `pending | answered | cancelled |
  timed_out`. First answer wins; a later answer returns the stored resolution with `accepted:false` (HTTP 200).
  Invalid choice id, free text on a choices-only question, empty or over-long text (4000) are 400. Max 50 pending.
- `questions/routes.rs`: AppHandle-free handlers: `GET /questions`, `POST /questions/{id}/answer`
  (`{choice_id}` or `{text}`), `POST /questions/{id}/dismiss`, `POST /questions/ask`. `bridge/router.rs` mounts
  them with one call. After any change it fires `questions://changed` (Tauri) and an SSE event
  `event: questions` (also reaches `/events/poll` and the WebSocket).
- `commands/questions.rs`: `questions_list`, `question_answer`, `question_dismiss`.
- **Auth.** Only `/questions/ask` is a catalog route, so `mcp_sessions::authorize` refuses `GET /questions`,
  `answer` and `dismiss` for any request carrying an MCP session header: an agent cannot answer its own question
  through the shim. The paired phone uses the normal bearer-token gate (`authorize_bridge_request`) and is
  recorded as `answered_by:"phone"`.
  Limitation: a process on the same machine that talks to the loopback bridge without a session header is trusted
  like the desktop (the existing trust model, same as pane typing).

### Frontend
- `lib/questions.ts` (types, `askerName`, `isLive`), `lib/cockpit-api.ts` (commands and event),
  `hooks/useQuestions.ts` (load, refresh on event, 15 s safety poll), wired into `useCockpit` as `c.asked`.
- `components/cockpit/QuestionCard.tsx` + `styles/cockpit-questions.css`: "Question from <agent>", choices as
  buttons (first is primary), a text field when free text is allowed, Dismiss, Open terminal when the pane is
  known. Rendered at the top of `Inbox.tsx`; counted in the Inbox badge and hero number; a pane with an open
  question is listed under "Needs you" in `Sidebar.tsx` (`groupPanes(list, questionPanes)`).
- Phone: `components/mobile/QuestionsList.tsx`, mounted as the `header` of `AgentsOverview` (the All tab). It
  polls `GET /questions` every 3 s while the tab is shown and answers through `BridgeClient`. The phone terminal
  and its input path are untouched. The SSE `questions` event is emitted for clients, but the PWA does not
  subscribe yet (polling is enough).
- Harness: `dev/scenarios/cockpit-questions.ts` (`questions` added to `HarnessScenario`, mocked
  `questions_list` / `question_answer` / `question_dismiss`).

OSS: none needed.

### Example: an orchestrator using it
```text
orchestrator -> run_agent(agent_type="claude", task="Refactor the billing module")   # subagent works
subagent output: "I can drop the legacy table or keep it behind a flag"
orchestrator -> ask_user({
  question: "Drop the legacy invoices table, or keep it behind a flag?",
  choices: [{id:"drop",label:"Drop it"},{id:"flag",label:"Keep behind a flag"}],
  context: "14 rows still reference it. Staging only.",
  urgency: "high", timeout_s: 1800, agent: "Release orchestrator"
})
  (blocks; the card appears in the Inbox and on the phone)
<- {answered:true, choice_id:"flag", answered_by:"phone"}
orchestrator -> followup_task(handle, "Keep it behind a flag.")
```

### Overlaps with other branches (file level)
Shared files touched with a few appended lines: `lib.rs`, `commands/mod.rs`, `bridge/router.rs` (one call before
`Ctx`), `bridge/tool_names.rs`, `tool_registry/catalog/mod.rs`, `tool_registry/mode.rs` (two list entries),
`tool_registry/tests.rs` (the pin), `bin/puppet_master_mcp/{mod.rs,progress.rs,dispatch/coordination.rs}`,
`lib/cockpit-api.ts`, `lib/bridge.ts`, `lib/bridge-types.ts`, `hooks/useCockpit.ts`, cockpit `Inbox.tsx` /
`Sidebar.tsx` / `model.ts`, `PwaApp.tsx`, `mobile/AgentsOverview.tsx`, `dev/types.ts`, `dev/mock-tauri.ts`.
Expect textual conflicts with `feat/worktree-isolation` and `feat/account-profiles` (both edit the catalog,
router, `lib.rs`, `commands/mod.rs`, and likely `useCockpit.ts` / Inbox / Sidebar): keep both sides.
**The catalog pin `catalog_version_is_pinned` will conflict: after merging, recompute it once** (this branch
alone: `67-fec1b1eacb499499`).

## 3. Evidence

Rust commands run in `packages/app/src-tauri` with `CARGO_BUILD_JOBS=3`.

- `cargo test --lib`: `test result: FAILED. 1019 passed; 1 failed; 2 ignored` (the one failure is the known
  `bridge::tests::definitions::approve_requires_app_session`).
- New store tests (`cargo test --lib questions`): `11 passed`: create defaults, bad input, answer + idempotent
  second answer, invalid choice rejected (stays pending), free-text trim/cap, timeout, dismiss + idempotent,
  `wait_resolved` wakes from another thread, pending slice, own-timeout wake, and the router transcript below.
- **In-process router transcript with auth**: `questions::tests::router_transcript_list_answer_dismiss_with_auth`.
  It mirrors the order in `router.rs` (pairing gate, MCP session gate, routes) because `route` needs a Tauri
  `AppHandle`. Asserted steps: MCP session `POST /questions/ask` 200 `resolved:false` with an id; desktop
  `GET /questions` 200 lists it; remote without token 401; bad token 401; MCP session `GET /questions` 403; MCP
  session `answer` 403; phone bad choice 400 `INVALID_CHOICE`; paired phone answer 200
  `accepted:true, answered_by:"phone"`; desktop second answer 200 `accepted:false` with the first answer kept;
  agent poll returns `{resolved:true, answered:true, answered_by:"phone", choice_id:"sq"}`; dismiss 200; agent
  poll returns `reason:"cancelled"`; unknown id 404.
- **MCP-level test** (`cargo test --bin puppet-master-mcp`: `33 passed`): the real shim `call_tool` against an
  in-process loopback bridge serving the real route handlers:
  `ask_user_blocks_until_answered_and_returns_the_choice` (the call stays blocked, then unblocks with
  `choice_id:"yes", answered_by:"phone"`), `ask_user_returns_free_text_and_reports_dismissal`,
  `ask_user_times_out_and_cancelling_the_call_withdraws_the_question`.
- **Catalog pin**: `catalog_version_is_pinned` now `67-fec1b1eacb499499` (was `66-46d53ccddbd2b4dc`). Fail
  before / pass after: with `ask_user` added and the old pin the test failed with
  `left: "67-fec1b1eacb499499" right: "67-46d53ccddbd2b4dc"`; it passes with the new pin. The three MCP tests
  cannot pass before the change (`unknown tool: ask_user`).
- `npm test --workspace=@puppet-master/app`: `Test Files 49 passed (49)`, `Tests 383 passed (383)`. New:
  `QuestionCard.test.tsx` (choice, free text, choice + text, dismiss, asker fallback, liveness),
  `QuestionsList.test.tsx` (phone choice + dismiss), and a `groupPanes` case in `model.test.ts`.
- `npx tsc --noEmit -p packages/app`: no output (clean).
- Screenshot: `D:\pm-night\evidence\ask-user\cockpit-questions.png` (cockpit Inbox with a choices question from
  Claude Code and a free-text question from "Planner"; the badge counts both; taken after merging `7651d54`).
  Command: `node scripts/evidence/screenshot.mjs --out D:\pm-night\evidence\ask-user cockpit-questions`.

## 4. Limits / follow-ups
- No OS push notification yet (the `questions` SSE event is the hook; `feat/tunnel-push` could subscribe).
- The PWA polls every 3 s instead of subscribing to the SSE event; no phone screenshot (the harness covers the
  cockpit only).
- Questions are lost on an app restart (documented above; the agent sees `cancelled`).
- The legacy TS MCP package (`packages/mcp-server`) would reach `/questions/ask` through its generic registry path
  with a 30 s read timeout; only the Rust shim implements the long-poll loop.
- `urgency:"high"` only bolds the card title; it does not change sorting or notifications yet.
- Cap of 50 unanswered questions and 6 choices per question.
