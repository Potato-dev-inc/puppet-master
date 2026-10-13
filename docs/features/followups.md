# Follow-ups (branch `feat/followups`)

Four small items found during the night shift, cut from `integration/night` 18c3d0f. No MCP catalog change
(pin untouched). Backup branch: `backup/feat-followups-start`.

Personas: A (developers) get fewer secrets lying around in plain files; B and C get a phone that reacts to
an agent's question at once and rings when they are away. Nothing here is agent specific: the push label comes
from the registry, the rest is generic.

## 1. Push for `ask_user` questions

- `questions/routes.rs::ask` calls `push::notify_question(&question)` once, on creation only (the shim's repeat
  polls carry `question_id` and never reach it).
- `push/triggers.rs`: new `Kind::Question` (`"question"`). Title `<agent> asks`; body is the question through
  the same `sanitize` (control and bidi stripping) and `truncate_chars(120)` as every other push.
  Fallback body: "Has a question for you".
- `push/hub.rs::plan`: the payload is `{v, kind, title, body, tag, target:"questions"}`. No `act`, no
  `pane_id`, no `prompt_id`. It is `urgent`, and goes through the same gate: presence suppression (a phone that
  is looking is skipped and the push is released if it goes hidden within 30 s) and the per-key 30 s rate limit.
  The key is `q-<question id>`, so each question notifies once and different questions do not suppress
  each other. The gate's per-key memory is now bounded (1024 keys).
- Label: the pane's agent label from the registry when the question carries a `pane_id`, else the agent-given
  name (sanitised, 40 chars), else "An agent".
- Phone: `pwa/push-core.ts` parses `target`, `sw.ts` opens `/?view=questions` or posts a view message to an open
  window, `usePushPresence` switches to the All tab (where the question list is the first card).

## 2. Toolbelt secrets in the OS credential store

- `toolbelt/store.rs`: values of secret-looking env vars and headers are written to the credential store
  (`secrets::backend()`, the trust-fixes keyring helper) under `toolbelt/<vault>/<server id>/<env|headers>/<NAME>`.
  `toolbelt.json` keeps `pm-keyring://<that name>`. `vault` is a random id stored in the file.
- A value is read back from the store before the file is allowed to hold only the reference. If the store is
  unavailable or does not read back, the value stays in the file (never lost, never claimed secure).
- `load_servers` resolves references, so `sync` still writes the real value into each agent's own config
  (agents need it; the UI says so). A missing entry reads as empty; an unreadable store is an error, so a save
  can never drop a reference by accident.
- Removing a server or a variable, or changing the value, deletes or overwrites the store entry.
- One-time migration `store::migrate`, called from `secrets::migrate_on_startup` (one added line). Idempotent.
- UI copy: "kept in your system's secure storage and hidden here".

## 3. Phone questions: SSE instead of 3 s polling

- `bridge-sse.ts` and `BridgeEvent` gain `{type:'questions', revision}` (EventSource, WebSocket and long-poll all
  share `dispatchBridgeEvent`). `PwaApp` bumps a counter on the event (and on `resync`), `QuestionsList`
  refetches when it changes. The fallback poll is 30 s. No terminal, input or lease code touched.

## 4. `/agents/types` without the absolute definition path

- `bridge/routes/mcp.rs::types_route`: the full listing (with `definition.path`) only for the desktop app
  (`is_local_app`: local peer, no device token, and no MCP session). Everyone else gets `definition` with
  `fingerprint` and `state` only. The phone does not read `definition` at all (checked: only
  `agentRegistry.ts` types it as `unknown`). The Tauri `agents_list` command is unchanged.

## Evidence

Run on this branch, in `packages/app/src-tauri` with `CARGO_BUILD_JOBS=3`.

```
cargo test --lib      test result: ok. 1442 passed; 0 failed; 2 ignored
cargo test --bins     test result: ok. 33 passed; 0 failed   (puppet-master-mcp); 0 passed (main)
npx tsc --noEmit -p packages/app     exit 0
npm test --workspace=@puppet-master/app     Test Files 100 passed (100), Tests 863 passed (863)
```

New tests:

- `push::hub::tests::a_question_notifies_once_with_no_action_buttons_and_opens_the_question_list` (payload keys
  are exactly `body, kind, tag, target, title, v`: no `act`; second plan of the same question is
  `RateLimited`), `a_question_is_held_back_while_the_phone_is_looking`,
  `push::triggers::tests::question_copy_is_the_truncated_sanitised_question`.
- `questions::tests::a_new_question_triggers_one_push_and_polling_or_cancelling_triggers_none` (route level).
- `toolbelt::tests::secrets_in_keyring::*` (5 tests with `MemoryBackend`: migration with read-back, store
  unavailable or lossy keeps the file value, save/update/cleanup, missing entry and unreadable store, sync still
  writes the real value) and the changed `store_validates_masks_and_keeps_secrets_on_edit`.
- `bridge::tests::agents_types_hides_definition_paths_from_callers_without_an_app_session` and the extended
  `definitions::agents_types_includes_declarative_after_approval` (real approved definition: app session has the
  path; MCP session, remote phone and local-with-device-token do not).
- Vitest: `bridge-sse.questions.test.ts`, `QuestionsList.test.tsx` (revision refetch, no 3 s poll, 30 s
  fallback), `pwa/push-core.test.ts` (target, no buttons, deep link).

Failing before, passing after:

- Rust, reproduced by reverting the behaviour (removing the `notify_question` call, `redact` forced off, and
  secrets forced to stay plain) on top of the new tests: 9 failed, 15 passed, including
  `a_new_question_triggers_one_push...`, `agents_types_includes_declarative_after_approval`,
  `store_validates_masks_and_keeps_secrets_on_edit` and four `secrets_in_keyring` tests. Restored: all pass.
- Vitest, with the pre-change `bridge-sse.ts`, `QuestionsList.tsx` and `push-core.ts` checked out from
  18c3d0f: 8 failed, 10 passed (`bridge SSE questions event`, the three `refetching` tests, four push-core
  tests). Restored: all pass.

## Overlaps and limits

- File-level overlap with other branches: `push/*`, `questions/routes.rs`, `bridge/routes/mcp.rs`,
  `bridge/tests.rs`, `toolbelt/store.rs`, `toolbelt/tests.rs`, `secrets.rs` (one line), `PwaApp.tsx`,
  `sw.ts`, `bridge-sse.ts`.
- A question push held back while the phone was looking is still released if the phone goes hidden within
  30 s, even if the question was answered meanwhile (the stale tap just opens an empty list).
- Toolbelt secret names depend on the vault id in `toolbelt.json`: copying the file to another machine does not
  carry the secrets (they stay in that machine's credential store).
- The phone has no test for the real service-worker click path (the logic is in `push-core.ts` and tested).
