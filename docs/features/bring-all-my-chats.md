# Bring all my chats: old chats from every agent in search

Branch `feat/bc-docs` (code from BC-12 to BC-23). Spec: `SPEC-n32` (planning file, not in the repo). This page says what
the code does today. Where the spec wanted more, the gap is listed at the end.

## What and why
Ctrl+Shift+F only finds chats that were open while the app ran. A new person has months of old chats in Claude Code,
Codex and OpenCode, and none of them show up. This feature reads the chats each agent kept on this PC and adds them to
the same search.

- Opt-in. Nothing in an agent folder is read until the person presses **Find my chats**.
- Local. Nothing is sent anywhere.
- Redacted before anything is saved. Secrets are masked, e-mail addresses are masked, the home folder shows as `~`.
- Tool output is not saved.
- One button removes it all.

## User story
1. Dana opens the palette and picks **Bring my old chats** (or opens Settings > General > "Bring my old chats").
2. The panel says: "Read my old chats on this PC. Nothing leaves this computer." Dana presses **Find my chats**.
3. The panel lists each agent it can read. Each row shows the chat count, the size, the dates and the folder it reads.
   Dana turns one agent off and keeps the time window at **12 months**.
4. **Add 538 chats to search** starts the run. A progress line and a bar show it. **Stop** keeps what was read so far.
5. Dana presses Ctrl+Shift+F and types "stripe webhook". A hit from an August Codex chat shows the tag **Old chat**.
6. A click opens the chat read-only, at that message. **Continue in a pane** starts the agent in the chat's folder with
   its own resume option.

## What the person sees (as built)

| Where | What it does |
|---|---|
| Palette row "Bring my old chats" | Opens the panel in a sheet (`components/history/ImportHost.tsx`). |
| Settings > General, row "Bring my old chats" | Shows "538 chats" or "Not added". **Add or update** opens the panel. **Remove imported chats** asks first, then removes them. |
| The panel (`BringChatsPanel.tsx`) | Intro, Looking, the agent list with switches, the time window, Add, Reading (bar, Stop), the result, Look again, Remove imported chats. |
| Index off | The panel says "Chat search is off. Turn it on to bring your old chats." with a **Turn on** button. |
| Search hit | An imported hit has the tag "Old chat". Choosing it opens the read-only viewer, not the closed-pane card. |
| Read-only viewer (`ImportedChatSheet.tsx`) | The chat in the normal chat view, with no message box. A banner says "Old <agent display name> chat from 14 Aug. Read-only." (the name comes from the agent registry, for example "Claude Code"; the date is the chat's own last message time) **Continue in a pane** is disabled, with a reason, when the chat cannot be continued. |

The time window choices are 30 days, 90 days, 12 months and All. The default is 12 months.

An agent with no reader is greyed out with "Can't read its old chats yet". An agent from a TOML definition starts
switched off and says "From your agent definition".

## Agents that have a reader today

| Agent id | Where its chats are | Resume option | Notes |
|---|---|---|---|
| `claude` | `{home}/.claude/projects/<folder>/<session>.jsonl`, and `<CLAUDE_CONFIG_DIR>/projects` when that variable is set | `--resume <session>` | `agent-*.jsonl` (sidechains) and anything under a folder below the project are skipped. |
| `codex` | `{home}/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` (or `.jsonl.zst`), and `<CODEX_HOME>/sessions` | `resume <session>` | When a chat has a plain file and a `.zst` copy, the plain one is read. The id and folder come from its `session_meta` line. |
| `opencode` | `<data home>/opencode/opencode.db` (SQLite). The data home is `XDG_DATA_HOME`, then `~/.local/share` | `--session <session>` | Top-level sessions only; child sessions are skipped. The database is opened read-only. |
| Any TOML agent with `[session_chat.history]` | The folders the table names | The table's `resume_args` | Off by default. See "How to add history to a TOML agent" below. |

**About OpenCode.** The table and column names in the reader were taken from the `opencode` 1.19.0 program that is
installed on the development PC (its migration SQL and schema). They were not checked against a live database. No
OpenCode session was run to make one. If the real layout is different, the reader says "can't read" for that agent and
does not show wrong text.

Cursor and other agents without a reader are listed as "Can't read its old chats yet". Puppet Master does not guess
their folders.

## Privacy

This table follows section 4 of the spec. The status column says what the code does on this branch.

| Threat | Guard in the code | Status |
|---|---|---|
| Tests or an agent read the person's real agent folders | Every store reads through `HistoryEnv`. `from_process()` is empty under test. `PUPPET_MASTER_HISTORY_HOME` replaces the home and blocks every other variable. The guard test `session_chat/history/guard.rs` fails when old-chat code calls `home_dir(` or `env::var` directly, or when a fixture has a user folder path. | Done. The guard covers `session_chat/history/`, `chat_index/import/` and `tui/*/history.rs`. |
| Secrets in old chats end up in the index | Strict redaction (secrets, long opaque runs, e-mail addresses, the home folder as `~`) runs in one function before any row is written. Tool output is never kept. Tests plant fake keys and an e-mail in a fixture and check the database and the write-ahead log bytes. | Done. |
| Old chat text reaches the companion, MCP, a teammate or the phone | Imported rows use the pane id `import:<agent>`. The companion's recall (`search_in`) never matches them. The MCP tools and team code do not search chats. The import commands are desktop-only; a guard test keeps them out of `bridge/`, `tool_registry/`, `mcp_shim/`, `companion/`, `team/` and `mobile_pairing/`. | **Gap.** The phone's search route `GET /chats/search` (`bridge/routes/chats.rs`) calls the all-chats search. It does not skip imported rows, so an old chat hit can reach a paired phone with the `watch` cap. No test covers this. See "Known gaps". |
| A TOML definition points the reader at a sensitive folder | Roots must start with `{home}` or `{env:NAME}`. Paths are checked under home, `..` is refused, symlinks that leave the root are skipped. Only `.jsonl` and `.jsonl.zst` files are read. Only the declared pointers are read. The definition must be approved first, and its agent starts off. | Done. |
| A built-in store lives outside home (`CLAUDE_CONFIG_DIR`, `CODEX_HOME`) | Only built-in readers name these variables. The value comes from `HistoryEnv::var`. The folder is shown on the screen. | Done. TOML definitions cannot use it. |
| A resume id reaches a command line | Every id from a file name, a pointer or a database must match `^[A-Za-z0-9_-]{8,80}$` before it is stored or used. The argument list is built in one place: `resume_plan`. | Done. |
| "Continue" types text or answers a prompt | It starts the pane with the agent's own resume arguments. Nothing is typed. Trust, permission and login questions stay with the person. | Done. |
| Prompt injection inside an old chat | Old text is data in a search row or a read-only view. Nothing runs or sends it. Imported text is never put into a prompt. | Done. |
| Huge or hostile stores | Caps: 2000 chats per agent, 64 MiB per file, 3000 items per chat, 20 000 folder entries, 8 s for a listing, 256 MiB decompressed per chat. Imported items have their own cap of 150 000. Each chat is one transaction. The writer sleeps 2 ms between chunks. Stop works at any chunk. The OpenCode read stops at its byte cap. | Done, with one difference: the spec also asked for a SQL `LIMIT` on every OpenCode query and a 1 MiB cap per row. The code does not have those two. |
| A half-written file | A line that does not parse gives no items. A file that changes while it is read only changes its stamp, so the next "Look again" reads it again. | Done. |
| Paths leak (account name) | No file path is stored. The folder is kept as a name in the index. The folder path (with `~`) is kept only in the local `import_chats` table, for resuming. Progress and errors carry counts and agent ids only. | Done. |
| Surprise: it starts without the person | `chat_import_state` reads no agent folder. Nothing is read until **Find my chats**. There is no background import. | Done. (Opening the index file is not reading an agent folder.) |
| The person wants it gone | **Remove imported chats** deletes the imported rows and the per-chat records, then truncates the write-ahead log. **Clear search index** also removes them. Turning the index off stops a running import. | Done. |

## How it fits together (files)

| Part | File (under `packages/app/src-tauri/src` unless noted) |
|---|---|
| The rules every store follows (`SessionHistory`, `HistoryEnv`, caps, ids) | `session_chat/history/mod.rs`, `env.rs`, `limits.rs`, `ids.rs` |
| The generic JSONL store (walk, stamp, read) | `session_chat/history/file.rs`, `file_walk.rs`, `file_head.rs` |
| The read-only viewer and the resume plan | `session_chat/history/open.rs` |
| TOML `[session_chat.history]` | `session_chat/declared/history_table.rs`, `declared/history.rs`; the table is in `docs/specs/agent-definitions/DEFINITION-FORMAT.md` 6.1 |
| Per-agent readers | `tui/claude/history.rs`, `tui/codex/history.rs`, `tui/opencode/history.rs` |
| The importer (catalog, scan, worker, progress, prefs) | `chat_index/import/` (`plan.rs`, `run.rs`, `progress.rs`, `prefs.rs`) |
| Index rows and the import record | `chat_index/store.rs` (`replace_chat`, caps, dedupe), `chat_index/entry.rs` (`Strength::Strict`), `chat_index/imported.rs` |
| Tauri commands (desktop only) | `commands/chat_import.rs`, registered in `lib.rs` |
| Screens | `packages/app/src/components/history/` (`BringChatsPanel.tsx`, `ImportedChatSheet.tsx`, `ImportHost.tsx`), `components/settings/blocks/BringChatsSettings.tsx`, `lib/chatImport.ts`, `lib/chatTab.ts`, `styles/history.css`, `styles/history-viewer.css` |
| Palette and hits | `components/cockpit/Palette.tsx` (row and hit choice), `ChatHits.tsx` (the tag) |

The Tauri commands are `chat_import_state`, `chat_import_look`, `chat_import_start`, `chat_import_cancel`,
`chat_import_dismiss`, `chat_import_clear`, `chat_import_open` and `chat_import_resume_plan`. Progress arrives as the
event `chat-import-progress`, at most four times a second and at every phase change.

The one store the importer writes to is the existing `chat-index.sqlite`. Imported items sit in the same table with
`imported = 1`. Live items keep their own cap (50 000), and an import can never push them out.

## How to add history to a TOML agent

Use this when your agent writes its chats to JSONL files and you want them in "Bring my old chats". The table is
optional. Without it the agent shows "Can't read its old chats yet".

The table only works inside a definition that also has `[session_chat]` and `[session_chat.map]`. The old-chat reader uses
the same map as the open chat.

```toml
[session_chat.history]
roots = ["{home}/.acme/sessions"]     # 1 to 4 folders; each starts with {home} or {env:NAME}
file_glob = "*.jsonl"                 # optional; must end in .jsonl or .jsonl.zst
depth = 2                             # optional; folder levels below a root, 0 to 4; default 1
id_from = "pointer"                   # "file_stem" (default: the file name) or "pointer"
id_pointer = "/session_id"            # required with id_from = "pointer", refused otherwise
cwd_pointer = "/cwd"                  # optional: the chat's folder, read from its first lines
resume_args = ["--resume", "{session}"]   # optional; only {session}; up to 8 elements of up to 64 characters
```

Then:

1. Check the folder on your own PC first. The roots are the folders the agent writes its chats to, for example
   `C:\Users\you\.acme\sessions`.
2. Save the definition and approve it in Settings > Agents. A changed `[session_chat.history]` table asks for approval
   again, like any other change to a definition.
3. Open **Bring my old chats**. The agent appears as "From your agent definition", switched off. The panel prints the
   folder it reads. Switch it on, then press **Find my chats**.

Rules the loader checks (`AGENT_DEF_SESSION_CHAT_BAD_HISTORY`, path such as `session_chat.history.roots.0`):

- No `..` in a root, and no placeholder other than `{home}` or `{env:NAME}`. `{cwd*}` is not allowed in a root.
- Every root starts with `{home}` or `{env:NAME}`.
- `file_glob` ends in `.jsonl` or `.jsonl.zst` and has no folder part.
- `depth` is 0 to 4.
- `resume_args` may use only `{session}`, and it must use it.
- `id_pointer` and `id_from = "pointer"` must go together (`AGENT_DEF_INCONSISTENT`).

If a chat id is not `[A-Za-z0-9_-]`, 8 to 80 characters long, the chat is counted as unreadable. The chat is not
imported and no command line is built from it.

## Tests

Counts are the number of test functions in the source on this branch. They were counted, not run, in this docs pass.
Treat them as floors. The full list of commands is in `docs/TEST-PLAN.md` section 1.

| Suite | Files | Count |
|---|---|---|
| Rust: history core, file store, viewer, guard | `session_chat/history/` (`tests.rs`, `file_tests.rs`, `open_tests.rs`, `guard.rs`, `env.rs`, `file_head.rs`) | 58 |
| Rust: TOML history table and reader | `session_chat/declared/history_table_tests.rs`, `declared/history_tests.rs` | 21 |
| Rust: index rows and importer | `chat_index/tests.rs` (`imported_tests`), `chat_index/import_api_tests.rs`, `chat_index/import/tests.rs`, `import/fixture_tests.rs` | 44 |
| Rust: Tauri commands | `commands/chat_import.rs` | 5 |
| Rust: readers | `tui/claude/history.rs` (6), `tui/codex/history.rs` (8), `tui/opencode/history_tests.rs` (17, plus 1 ignored `regenerate_fixture_db`) | 31 |
| Rust: conformance rule for old-chat stores | `tui/conformance.rs` | 3 |
| UI | `lib/chatImport.test.tsx`, `components/history/*.test.tsx`, `components/settings/blocks/BringChatsSettings.test.tsx`, `lib/chatTab.test.tsx` | 45 |
| Scripts | `scripts/make-fixture-history-home.test.mjs` (`node --test`) | 15 |

Fixtures are synthetic. Claude and Codex fixtures are under `tui/*/fixtures/history/`. The OpenCode fixture is
`tui/opencode/fixtures/history/opencode.db`. They use `C:\Users\you\...` and fake ids. `.gitattributes` keeps the text
fixtures LF and marks the `.db` as binary.

## Known gaps

- **Phone search (fixed).** `GET /chats/search` now calls `ChatIndex::search_live`, which skips imported rows
  (`Reach::LiveOnly`). `companion_recall` (`search_in`) is live-only too. The guard `all_chats_search`
  (`session_chat/history/guard.rs`) fails when a phone, tool, companion or team file calls an every-row search.
- **The first-start card is not on this branch.** The card ("Read my old chats on this PC...") is BC-13 on
  `feat/bc-card`. Until it is merged, nothing asks on first start. The panel is reached from the palette and Settings.
  `scripts/verify-bring-chats.mjs` waits for the panel at start, so on this branch open the panel first.
- **OpenCode is not proven on a live database.** See the note above the table.
- The Settings row shows the chat count only. It does not show the size.
- `chatImportDismiss` (the "Not now" call) exists in `lib/chatImport.ts`, but no UI calls it on this branch.
- No live run is recorded for this branch. Continue in a pane for Claude and OpenCode is covered by unit tests of the
  resume plan only. The live check (fixture home, its own profile and CDP port) is in `docs/TEST-PLAN.md` section 2.8.
- Not in v1 (planned as v2): a browse list of old chats, "continue on another agent with a brief", background refresh,
  Cursor and agy stores, phone access.

## Evidence
No live run is recorded for this branch in this docs pass. Record the live run (fixture home, the steps in
`docs/TEST-PLAN.md` section 2.8) as a note next to this file when it is done.
