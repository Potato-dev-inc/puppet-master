# Global Ask bar ("agents anywhere")

Branch `feat/ask-bar`, cut from `feat/loop-guard`. The MCP catalog is untouched.

## 1. What and why

Personas A (developers) and B (vibecoders): a question comes up while you are in the browser, an editor or a PDF. Today you open a vanilla agent app, wait for it to start, and paste. Here a Spotlight-style bar opens from anywhere in Windows, takes one line (with the clipboard attached if you want), asks your agent and shows the answer in the bar. If you click away first, the answer waits for you as a card in the cockpit Inbox.

User story: press `Ctrl+Alt+Space`, type "what does this error mean?", Enter. A dot spinner runs, then the answer appears with **Copy**, **Continue in a pane** and **Close**. Esc hides the bar.

Agent-agnostic: the bar is `quick_ask` with a `backend` hint, nothing more. The agents it lists are whatever the registry says can answer a headless read-only question (`tui::read_only_backends()` that are real agents, not shells) and are installed. "Ready" comes from the warm standby pool (`standby::ready_agents`). Labels come from the descriptor. No agent name appears in generic code.

It is OFF by default. Nothing is registered, no window is created, until the user turns it on in Settings > General > "Ask bar".

## 2. Design

### Rust (`packages/app/src-tauri/src/`)
- `askbar/mod.rs`: pure, injectable logic.
  - `Settings { enabled, shortcut, default_agent, last_agent }` persisted to `askbar.json` in the app data dir; `sanitized()` accepts only `SHORTCUT_CHOICES` (`Ctrl+Alt+Space` default, `Ctrl+Shift+Space`, `Alt+Space`, `Ctrl+Alt+K`).
  - `window_spec()`: label `askbar`, URL `index.html?askbar=1`, 640x64, grows to 640x420, frameless, transparent, always on top, skip taskbar, no shadow, created hidden and unfocused. `spotlight_position` (centred, a fifth down the monitor under the cursor), `should_hide_on_blur` (300 ms grace so the focus handover does not hide the bar).
  - `ShortcutRegistrar` + `sync_shortcut`: the shortcut is registered only when enabled, re-registered on a change, released when disabled.
  - `clip_text` / `compose_task`: clipboard cut to 8000 characters (characters, not bytes), appended as quoted context; blank clipboards attach nothing.
  - `run_ask(request, label, project, runner)`: builds the read-only `QuickAskRequest` and maps every outcome (answer, still running, needs input, empty answer, error via `agent_health::classify_failure`) to an `AskResult`. The runner is a closure, so tests need no `AppHandle`, as in the agent doctor.
  - `continue_request`: the `run_agent` body for "Continue in a pane" (visible, kept, not read-only, question plus clipboard as first task).
  - `Notices`: in-memory list (newest first, 20 max) of answers that nobody saw. It is deliberately separate from `feat/ask-user`'s question store (not on this branch).
- `askbar/glue.rs`: the Tauri side. `init` (at startup: load settings; only when enabled, register the shortcut and create the hidden window), `ensure_window` (builds hidden, `focused(false)`, hides on blur), `show`/`hide`/`toggle`/`resize`, `read_clipboard` (arboard), notice publishing and events.
- `commands/askbar.rs`: `askbar_settings_get/set`, `askbar_agents`, `askbar_clipboard`, `askbar_hide`, `askbar_resize`, `askbar_ask`, `askbar_continue`, `askbar_notices`, `askbar_notice_dismiss`, `askbar_notice_open`. `askbar_ask` runs `quick_ask` on a blocking thread; when it ends and the bar is not visible, it publishes a notice (event `askbar://notice`).
- Events: `askbar://shown` (bar opened by the shortcut: page resets and re-reads clipboard), `askbar://show-result` (Inbox "Open"), `askbar://notice`, `askbar://notice-removed`.
- Capabilities: `askbar` added to `capabilities/default.json`. No new JS plugin permission is used (clipboard is read in Rust, the window is driven by commands).
- `agent_runs.rs`: one word, `run_agent` added to the existing `pub use runtime::{...}` so the command can start the visible pane.

### Frontend (`packages/app/src/`)
- `lib/askbar.ts` (types, `AskbarApi`, Tauri implementation, `useAskNotices`, chip label helpers), `lib/askbar-text.ts` (splits an answer into text and fenced code blocks; the app has no markdown renderer, so answers are plain text with code blocks in mono).
- `components/askbar/AskBar.tsx` (+ `AskBarApp.tsx`, `styles/askbar.css`): empty, running (DotSpinner), answer and error states; agent chip and menu with "Ready" marks; clipboard chip ("Clipboard · 1.2k chars", toggle, "8k of 52k chars" when cut); Enter sends, Esc hides.
- `components/cockpit/AskNotices.tsx` (+ `styles/ask-notices.css`): "Answer from Codex" cards in the Inbox with Open (shows the answer in the bar) and Dismiss.
- `components/settings/tabs/AskBarSettings.tsx`: enable switch, shortcut list, agent ("Last used" or a specific agent).
- `main.tsx` route `?askbar=1`; harness route `askbar` and scenarios `askbar-empty`, `askbar-answer`.

### OSS
- `tauri-plugin-global-shortcut` 2 (MIT/Apache-2.0), already on the base branch.
- `arboard` 3, `default-features = false` (MIT/Apache-2.0), new: reads the clipboard text from Rust so the window needs no clipboard permission and no WebView2 permission prompt.

## 3. Overlaps with other branches (file level)
- `feat/loop-guard` (base) and anything that touches the same registration points: `lib.rs` (a `mod askbar;` line, eleven command lines, one init line), `commands/mod.rs` (two lines), `Cargo.toml` (`arboard`), `Cargo.lock` (arboard and its Windows dependencies; take either side and re-run cargo).
- `capabilities/default.json`: any branch adding a window label (the `windows` array).
- `agent_runs.rs`: one-word export edit (`run_agent`); conflicts only with a branch editing that same `pub use` line.
- `feat/ask-user`: no shared files expected. Both add an Inbox surface, so `Inbox.tsx` will conflict textually (this branch adds one import and one `<AskNotices />` line after the stuck cards). Its question store could later replace `askbar::Notices`.
- `feat/ui-monochrome` / any branch editing the harness: `dev/harness.tsx`, `dev/types.ts` (route union), `dev/mock-tauri.ts` (askbar cases appended), `main.tsx` (route line), `settings/tabs/GeneralTab.tsx` (one line), `agent_runs/quick.rs` is read only (not edited).
- `tray.rs`: not touched (no tray item for the bar).

## 4. Evidence
Commands run in `D:\pm-ask-bar`, 2026-10-14.

- `cargo test --lib` (in `packages/app/src-tauri`, `CARGO_BUILD_JOBS=3`): `test result: FAILED. 1084 passed; 1 failed; 2 ignored`; the one failure is the known `bridge::tests::definitions::approve_requires_app_session`. Base branch: 1059 passed, so +25.
- New Rust tests (`cargo test --lib askbar` -> `25 passed; 0 failed`):
  - window config: `window_is_a_small_frameless_always_on_top_hidden_window` (640x64 / 420, hidden, not focused), `the_window_is_allowed_in_the_capabilities_file`, `position_is_centred_a_fifth_down_on_the_monitor`, `a_blur_during_the_focus_handover_does_not_hide_the_bar`.
  - shortcut: `it_is_off_by_default_and_registers_nothing`, `the_shortcut_is_registered_only_when_enabled_and_follows_changes` (fake registrar log: `+Ctrl+Alt+Space`, `-Ctrl+Alt+Space`, `+Alt+Space`, `-Alt+Space`), `a_shortcut_the_os_refuses_is_reported_and_not_remembered`, `settings_only_accept_the_choice_list_and_trim_agents`.
  - clipboard: `short_clipboard_text_is_kept_whole`, `long_clipboard_text_is_cut_to_8000_characters_not_bytes`, `an_empty_or_blank_clipboard_attaches_nothing`, `the_task_quotes_the_clipboard_as_context_and_never_exceeds_the_limit`.
  - `quick_ask` wiring: `an_ask_becomes_a_read_only_headless_quick_ask` (also run through the real `build_run_request`: headless, read-only, no shell), `a_finished_answer_comes_back_with_the_agent_label`, `a_blank_question_never_reaches_the_agent`, `every_failure_is_a_short_human_sentence` (running, needs input, empty, backend missing).
  - fake agent (the doctor's throwaway-script pattern, a real child process): `a_fake_agent_process_answers_through_the_bar_path` (answer path), `a_fake_agent_process_that_fails_gives_the_error_path` (exit 3, stderr).
  - agents and rest: `only_installed_capable_agents_are_listed_with_ready_marks`, `the_real_registry_offers_exactly_the_read_only_headless_agents`, `continue_in_a_pane_is_a_visible_kept_pane_with_the_question_as_first_task`, notices (`a_notice_is_needed_only_when_the_bar_was_hidden`, `notices_keep_newest_first_replace_by_ask_and_can_be_taken`, `notices_are_capped`), `the_wire_shape_matches_the_frontend_contract`.
- `npm test --workspace=@puppet-master/app`: `Test Files 55 passed (55)`, `Tests 418 passed (418)` (base 394, so +24): `AskBar.test.tsx` (9: empty state, agent picker with Ready marks and the chosen agent used, clipboard chip 1.2k / "8k of 52k" / toggle off sends null, running state with the DotSpinner, answer state with a code block and the three buttons, Continue in a pane then hide, error state with Try again, Esc hides), `askbar-text.test.ts` (5), `AskNotices.test.tsx` (6), `AskBarSettings.test.tsx` (4).
- `npx tsc --noEmit -p packages/app`: no output (clean).
- Fails before, passes after: `the_window_is_allowed_in_the_capabilities_file` with `capabilities/default.json` taken from the base commit (`git checkout 4e00703 -- ...`): `test askbar::tests::the_window_is_allowed_in_the_capabilities_file ... FAILED`, message `askbar missing from capabilities`, `0 passed; 1 failed`; restored: passes.
- Screenshots (headless harness, `node scripts/evidence/screenshot.mjs --out D:\pm-night\evidence\ask-bar askbar-empty askbar-answer`; the harness mimics the real 640x64 / 640x420 window): `D:\pm-night\evidence\ask-bar\askbar-empty.png` (agent chip with the ready mark, placeholder, clipboard chip) and `askbar-answer.png` (answer with a mono code block, Copy / Continue in a pane / Close). Both viewed and correct. The question, the answer and the clipboard size are fixtures.

## 5. Limits and follow-ups
- Not exercised in a real window: no GUI was launched, so the global shortcut, the transparent frameless window, blur-to-hide, focus on show, the clipboard read and the Inbox card were not seen live. The pure parts are tested; the Tauri glue (`askbar/glue.rs`) compiles but is untested.
- The window is created at startup only when the bar is enabled (a deliberate refinement of "created hidden at startup", so users who never enable it pay no webview). Turning it on creates it hidden at once.
- The shortcut is a fixed choice list, not a recorder (a key-capture control is the follow-up). A combination another app already owns fails to register; the failure is logged, not shown yet.
- An answer that was visible when the user clicked away is gone (hide on blur); only answers that arrive while the bar is hidden become Inbox cards. Pressing the shortcut starts a fresh bar.
- A question that takes longer than 90 s ends with "taking longer than expected", and the run keeps going; "Continue in a pane" starts a new pane rather than attaching to that run.
- The question runs read-only in the default project folder. The clipboard is attached by default (visible chip, one click to turn off); it may hold secrets, so the chip is always shown while text is attached.
- Answers are plain text with fenced code in mono; there is no markdown renderer in the app yet.
- Notices are in memory; they do not survive an app restart and are not pushed to the phone.
