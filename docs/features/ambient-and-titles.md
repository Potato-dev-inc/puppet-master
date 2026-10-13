# Ambient state and auto-titles

Branch `feat/ambient`. Idea 40 in `D:\pm-night\IDEAS.md`.

## 1. What and why

**Personas:** all three. A developer glances at the tray to see whether anything needs them. A vibecoder with five
panes stops reading `claude · 3f9a02bc`. A non-technical user sees "auth refactor" instead of an id.

**Story:** the tray icon is a 3x3 dot matrix that mirrors the whole app. Needs you is a plus with a bright centre,
working is a sweep, idle is four dim corners, error is an X. The tooltip says "Puppet Master — 2 need you · 3
working". The cockpit's taskbar button carries a count badge. Every pane gets a human title from its first task
("Codex · auth refactor"), shown in the sidebar, the pane header, pop-out window titles, the tray Panes menu, the
Ctrl+K palette and the phone tabs. A person can rename any pane.

**Agent-agnostic:** state comes from `PaneStatus` only (`running | waiting_input | idle | error`). Titles come from
text that was typed into, or delegated to, a pane. No code looks at an agent id except to print its label.

## 2. Design

### Backend (`packages/app/src-tauri/src/ambient/`)

| File | Role |
|---|---|
| `glyph.rs` | Pure. `Aggregate` (counts), `pattern_for` (priority), `levels` (3x3 alphas), `Ticker` (frame state machine), `tooltip`. |
| `raster.rs` | Pure. `render_rgba(levels, size, Ink)` (4x4 supersampled circles), `badge_rgba(count, size)` (disc + 3x5 digit font). Test-only PNG writer. |
| `tray_icon.rs` | The thread. Sleeps on a condvar; 250 ms per frame only while a sweep animates. `RegGetValueW` reads `SystemUsesLightTheme` (no new crate). |
| `title.rs` | Pure. `derive_title`, `sanitize`, `LineCapture` (rebuilds the typed line from raw chunks). |
| `store.rs` | Process-wide map `pane id -> title / user_set / capture`. `PaneState::info()` reads it into `PaneInfo.title`. |
| `prefs.rs` | "Animated tray icon", persisted in `<app data>/ambient.json` (default on). |
| `mod.rs` | `init`, `window_title`, `announce_title_change`. |
| `commands/ambient.rs` | `pane_set_title`, `ambient_get_prefs`, `ambient_set_animated_tray`. |

**Aggregate priority** (`glyph::pattern_for`): any `waiting_input` gives needs-you; else any `error` gives X; else any
`running` gives the sweep (a static perimeter ring when the setting is off); else idle corners. Hidden warm-standby
panes are excluded.

**Animation:** 8 frames (one per perimeter cell, a bright head and a fading tail, like the DotSpinner sweep),
`FRAME_INTERVAL` = 250 ms. When nothing runs, `Ticker::next_wait()` is `None` and the thread blocks with a 30 s
safety refresh: zero timer CPU while idle. Each frame is a 32x32 RGBA icon pushed with `tray.set_icon`; unchanged
looks are skipped.

**Hook:** `bridge/sse.rs::push_pane_status_sse` calls `ambient::notify_changed()` (one line), and the
`pty://panes-changed` event does the same. The PTY reader is untouched.

**Light taskbar:** `SystemUsesLightTheme = 1` makes the dots black. Checked with `RegGetValueW` (one syscall) on
every state change and on the 30 s refresh.

**Taskbar badge:** Tauri's `WebviewWindow::set_overlay_icon` (which is `ITaskbarList3::SetOverlayIcon` on Windows)
on the cockpit window, with a white disc and the needs-you + error count (same number as the cockpit "Needs you"
group). I did not add the `windows` crate as a direct dependency: Tauri already wraps it. It is a no-op off
Windows.

**Auto-titles:** `pty/registry/pane_io.rs::write_input` (the choke point for the GUI, the bridge, `run_agent` and
`send_message`) calls `ambient::store::observe_input` before anything else (one line). `LineCapture` handles
backspace, Ctrl-U, Ctrl-W, Ctrl-C, ANSI/OSC escapes split across chunks, and pasted multi-line text. The first
submitted line that yields a title wins. `derive_title`: strip ANSI/control chars, drop leading filler ("please",
"can you", "I want you to", "go ahead and", ...) and trailing filler ("thanks"), keep the first sentence, cap at
40 characters on a word boundary with an ellipsis. Rejected as not-a-task: empty lines, slash commands, `!cmd`,
one-word answers ("y", "yes", "continue"), and a single long token with digits (looks like a secret). For
`run_agent`, the task text reaches `write_input` as the submitted line, so it is the title.

**Rename:** `pane_set_title` stores a user title (wins over auto); a blank name clears it and automatic titling
resumes. Cockpit: double-click the header title or `...` > Rename; Enter saves, Escape cancels.

**Where the title shows**

- Cockpit sidebar row, Inbox row, header, Ctrl+K palette: title is primary, "Agent label · project" secondary.
- Pop-out window title: `ambient::window_title` ("Codex · auth refactor"), set at creation and updated on change.
- Tray Panes menu: "Codex · auth refactor" (falls back to `agent · id8`).
- Phone: `tabLabels` uses the title; `PaneInfo` (shared zod schema) gained `title`.

**OSS:** nothing new. `tauri` tray + `set_overlay_icon`, `once_cell`, `parking_lot` (already dependencies). No
code copied from elsewhere.

## 3. Evidence

Commands run in `D:\pm-ambient`:

```
cd packages/app/src-tauri
AMBIENT_EVIDENCE_DIR=D:\pm-night\evidence\ambient CARGO_BUILD_JOBS=2 cargo test --lib ambient
  test result: ok. 37 passed; 0 failed
CARGO_BUILD_JOBS=2 cargo test --lib
  test result: FAILED. 1046 passed; 1 failed   (the known bridge::tests::definitions::approve_requires_app_session)
npm test --workspace=@puppet-master/app
  Test Files 50 passed (50)   Tests 398 passed (398)
npx tsc --noEmit -p packages/app      (clean, after `npm run build -w @puppet-master/shared`)
```

New Rust tests:

- Pattern selection and priority: `glyph::tests::needs_you_beats_error_beats_running_beats_idle`,
  `statuses_are_counted_and_unknown_ones_are_idle`, `animation_off_gives_a_static_ring_instead_of_a_sweep`,
  `pattern_shapes_match_the_cockpit_dot_spinner`, `the_sweep_head_walks_clockwise_round_the_perimeter`.
- Frame cycling and stop: `frames_cycle_and_wrap_while_running`, `animation_stops_when_nothing_runs` (no timer when
  idle / needs-you / error / animation off, stray tick is a no-op), `a_new_sweep_starts_at_frame_zero_but_a_running_one_keeps_its_place`.
- Tooltip: `tooltip_lists_needs_errors_and_working` ("Puppet Master — 2 need you · 3 working").
- RGBA per pattern at 16 and 32 px (`raster::tests::each_pattern_renders_dots_where_expected`, ink colour and
  transparency, bright centre, structural "golden" 32 px idle bitmap with symmetry and 4 blobs, badge digits).
- Titles: `title_tests::derives_titles_from_sample_lines` (34 sample lines: 22 titles, 12 rejections),
  40-char caps, ANSI/OSC/control sanitising, and `LineCapture` across chunks, split escapes, backspace (0x7f and
  0x08), Ctrl-U, Ctrl-W, empty Enter, rejected first line, multi-line paste, arrow keys, a 100 000 char paste.
- Store: `store::tests` (typing then Enter titles once; a user name wins and a blank clears it; task text; prune).
- Tray menu: `tray::tests::a_titled_pane_is_listed_with_agent_label_and_title`.

Frontend tests (`Titles.dom.test.tsx`, `titles.test.ts`, `mobile-agent-tabs.titles.test.ts`): the sidebar shows the
title (primary) with agent and project (secondary), untitled rows unchanged, the palette row, `RenameTitle`
(double-click, Enter saves the cleaned text, Escape cancels, unchanged does nothing, blank clears), phone tab
labels.

**Fails before, passes after:** the sidebar test fails on the previous `Sidebar.tsx` and passes with the change:

```
git stash push -- src/components/cockpit/Sidebar.tsx
npx vitest run src/components/cockpit/Titles.dom.test.tsx
  x Sidebar pane titles > shows a titled pane by its title, with the agent and project folder beneath
  Tests 1 failed | 9 passed (10)
git stash pop   -> 10 passed
```

**Rendered tray icons** (white on a dark background, black on light, 32 px and 16 px, plus badges) are in
`D:\pm-night\evidence\ambient\`: `sheet.png` (idle, needs-you, error, `running_0..7`, ring), `<state>_32.png`,
`<state>_32_light.png`, `<state>_16.png`, `badge_1.png`, `badge_3.png`, `badge_12.png`. I looked at them: idle
corners, plus with a bright centre, X, and a sweep whose head walks clockwise.

**Screenshots** (harness, `scripts/evidence/screenshot.mjs`): `cockpit-titles.png` (titled sidebar rows, "Codex ·
billing-service" under "auth refactor", header), `cockpit-rename.png` (header in edit mode), `cockpit-palette.png`.

## 4. Limits and follow-ups

- The tray icon and the taskbar badge were not run on a real desktop (the lead rules forbid launching the app). They
  are covered by pure tests and the PNGs, but `tray.set_icon` at 4 fps and `set_overlay_icon` need a manual look.
- The overlay badge needs the cockpit window to exist; it is applied when the window is present and re-applied on
  the next change. A cockpit opened later shows the badge after the next state change or the 30 s refresh.
- Titles are derived from the first submitted line, so a pane whose first line is a login code or a one-word answer
  waits for the next real line. A password typed into an echo-off prompt looks like a normal line to PM; the
  single-token-with-digits guard covers API keys and tokens, not a short plain password. A pane launched with a
  command-line prompt argument (no typed line) gets no auto title until it is renamed.
- Titles are in-memory: they reset when the app restarts (panes do not survive restarts anyway).
- Tray menu and window titles use the agent label from the registry listing; a custom definition without a label
  falls back to the id.
- `PaneInfo.title` is serialized only when set, so older clients ignore it.

## 5. Overlap with other branches (file level)

Shared files touched (minimal lines): `src-tauri/src/lib.rs` (mod, init, 3 commands), `commands/mod.rs`,
`bridge/sse.rs` (one line in `push_pane_status_sse`), `pty/registry/mod.rs` (`PaneInfo.title` field plus `info()`
line, so every `PaneInfo { .. }` literal gained `title: None`: `agent_contexts/tests.rs`, `agent_runs/routes/tests.rs`,
`opencode/native/mod.rs`, `pty/registry/spawn.rs`, `shell_exec/tests.rs`), `pty/registry/pane_io.rs` (observe call plus
`announce_title`), `tray.rs` (`PaneRow` fields, label), `app_windows.rs` (one title line), `packages/shared/src/protocol.ts`
(`title` in `PaneInfoSchema`), `lib/tauri-types.ts`, `lib/cockpit-api.ts`, `hooks/useCockpit.ts` (`rename`),
cockpit `Sidebar.tsx`, `PaneView.tsx`, `Palette.tsx`, `Inbox.tsx`, `Icons.tsx`, `styles/cockpit.css` (appended),
`lib/mobile-agent-tabs.ts`, `settings/tabs/AppearanceTab.tsx` (one line), `dev/mock-tauri.ts`. Any branch that adds a
field to `PaneInfo` or a `PaneInfo` literal will conflict trivially in those literals. The MCP catalog is untouched.
