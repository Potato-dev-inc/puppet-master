# OpenCode native worker (`opencode_native`)

How the orchestrator should treat an `opencode_native` pane.

## Mental model

```
Orchestrator (Cursor MCP)
        │ write_terminal_input  →  OpenCode REST (prompt_async)
        │ read_opencode_messages ←  GET /session/.../message, GET /question
        │ reply_opencode_question → POST /question/{id}/reply
        ▼
Puppet Master bridge
        ▼
opencode serve (local HTTP)  +  attach TUI (human visibility)
```

- **Prompts** go through the **API**, not PTY keystrokes.
- The **TUI** is for humans watching; it can show menus that **do not** map 1:1 to what the API session needs.
- **Never** use `press_key` to answer API yes/no questions — keys land in the compose box (“How can I help?”) and nothing happens.

## Sending work

```text
write_terminal_input { pane_id, text: "..." }
→ suggested_wait: wait_for_worker
wait_for_worker { pane_id }
```

`write_terminal_input` may include `model_id` / `model_provider` to override the session model for that prompt.

## Reading output

| Tool | Use |
|------|-----|
| `read_opencode_messages` | **Default** — assistant text, tool parts, `pending_question`, `request_id` |
| `read_opencode_worker_status` | Serve health, permissions count, session id, TUI attached |
| `inspect_agent_model` | `session_model` + `last_user_model` from session API |
| `read_terminal_buffer` | Debug only — TUI chrome, duplicates, stale menus in scrollback |

## Yes / no confirmation flow

Trigger example (worker asks via question tool):

```text
Ask me one yes/no question only: Should I proceed with the next action?
Do not run any tools until I answer.
```

Orchestrator steps:

1. `write_terminal_input` with the prompt above.
2. `wait_for_worker` → expect `reason: "tui_prompt"` and `agent_hint.action: "reply_opencode_question"`.
3. `read_opencode_messages` → `pending_question.request_id`, options (`Yes` / `No`).
4. `reply_opencode_question { pane_id, answer: "No", request_id }` — label must match an option.
5. `wait_for_worker` → `idle`.
6. `read_opencode_messages` → confirm `pending_question` is gone; read `last_assistant_text`.

### Trust API over buffer

After answering, `wait_for_worker` may still report `tui_prompt` because **old menu text remains in scrollback**. Trust `read_opencode_messages.pending_question` (null = answered).

## Permissions

When `reason: "permission"`:

```
reply_opencode_permission { pane_id, request_id, reply: "once" }
```

Use `read_opencode_worker_status` for `pending_permission_ids` if the hint omits `request_id`.

## Model switches

```
switch_agent_model { pane_id, model_id, model_provider? }
→ suggested_wait: wait_for_model
inspect_agent_model { pane_id }   # confirm session_model
```

Do not infer model from TUI footer alone.

## How waits work (not polling)

`wait_for_worker` **blocks** until Puppet Master wakes (PTY output, input written, question replied, events). On wake it **pulls** OpenCode `GET /question` — OpenCode does not push webhooks.

You do **not** need a hot loop of `read_opencode_messages`. One wait, one read, one reply.

## Spawning

```
spawn_agent { agent_type: "opencode_native", cwd: "<project>" }
→ suggested_wait
```

Or CLI: `npm run worker opencode_native` from repo root (dev).

## Read-only runs (`read_only: true`)

Supported for a pane the run **spawns itself** (no `pane_id`/`worker_id`). The app starts
`opencode serve` with `OPENCODE_CONFIG_CONTENT` defining a primary agent `pm-read-only`
(`permission: {"*":"deny", read/grep/glob/list/question/todowrite: "allow"}`) and
`default_agent: "pm-read-only"`. The agent never receives `bash`, `edit`, `write`, `webfetch`, `task`
or any MCP tool. Prompts, follow-ups and TUI input in that pane all use it. Adopting a shared pane
with `read_only: true` stays `READ_ONLY_UNSUPPORTED`; a read-only run never reuses a pane started
without the config, and other runs never land in a read-only pane. The receipt reports `enforced`.
The human in the attached TUI can still switch the agent to `build` (it is a visibility TUI).

### How this was verified (opencode 1.18.34, Windows, model `openrouter/anthropic/claude-haiku-4.5`)

Each run: `OPENCODE_CONFIG_CONTENT=<cfg> opencode serve --port N`, `POST /session?directory=<tmp>`,
`POST /session/{id}/prompt_async` asking it to (1) create `created.txt`, (2) run
`echo bashran > bash.txt`, (3) read `readme.md`, then list the directory and `GET /session/{id}/message`.

| Config | Result |
|--------|--------|
| `{}` (control) | `read`, `write`, `bash` tools all completed; `created.txt` and `bash.txt` exist |
| `permission: {edit:deny, bash:deny, webfetch:deny}` | no file created; only `read` completed. But the MCP tool `pencil_execute` (from the user's global config) was still offered and attempted to write, so a top-level deny list is not enough |
| `permission: {"*":deny, read/grep/glob/list/question/todowrite: allow}` | no files created, `read` completed, no MCP tool offered |
| same, plus a project `opencode.json` with `permission {bash:allow, edit:allow, "*":allow, "pencil_*":allow}` | `pencil_execute` offered again: a later project rule beats the config-level `"*":deny` |
| agent `pm-read-only` with the deny-all ruleset, selected per message (`agent` field) against that hostile project config | no files created, only `read`/`list` completed, no MCP tool offered |
| same agent as `default_agent`, no per-message `agent`, hostile project config | same result, so TUI and API prompts are both covered |

(The `opencode/*` free-tier models answered 403 from outside the OpenCode client, so the
OpenRouter provider already configured on this machine was used.)

## MCP catalog freshness

Local dev: prefer `packages/mcp-server/dist/legacy.js` in Cursor MCP config — it loads tools from `GET /mcp/tools` on connect. Stale `puppet-master-mcp.exe` may hide `read_opencode_messages` / `reply_opencode_question`. Toggle MCP after `npm run build:mcp`.

---

## Next steps for this project

Prioritized from current pain points and [ROADMAP.md](../../ROADMAP.md):

### Near term (0.2.x polish)

1. **Wake waiters on OpenCode question events** — today wait relies on PTY/event bumps; add explicit wake when `GET /question` would change (reduce latency / stale `tui_prompt` from buffer).
2. **Skip `buffer_tail` when API has `pending_question`** — smaller wait responses, fewer wasted orchestrator tokens.
3. **Default MCP launcher docs** — ship `legacy.js` as recommended dev path; reliable rust binary refresh on Windows (EPERM).
4. **`agent_hint` includes `request_id`** on `tui_prompt` — orchestrator can call `reply_opencode_question` without an extra `read_opencode_messages`.
5. **Integration test** — yes/no flow via MCP only (the manual test we ran).

### Medium term (0.3–0.35)

6. **Session context + pane digests** — orchestrator reads summaries instead of message history (`read_session_context`, `read_pane_digest`).
7. **Structured `delegate_task`** — intent + acceptance criteria in one call.
8. **Project rules/skills** — `.puppet-master/rules/` loaded into orchestrator prompt automatically.
9. **OpenCode watch polls questions** — extend `opencode/watch.rs` (currently 2s poll for permissions only) to emit SSE on pending questions.

### Longer term

10. **Desktop orchestrator UX** — sidebar shows pending questions with one-click reply.
11. **npm publish + CI** — MCP integration tests on Windows/macOS.
12. **Token/cost metadata** per orchestrator run.
