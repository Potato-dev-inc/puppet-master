# Claude Code's fullscreen renderer and the readiness check (round 13, #2)

Claude Code 2.1.300 can draw its UI in a "fullscreen renderer" (`/tui fullscreen`, the `tui` setting, or
`CLAUDE_CODE_NO_FLICKER=1`; some machines start in it by default). On the dev PC it fell back to the classic
renderer ("fullscreen renderer didn't finish starting last time", `/tui default` keeps classic). The
readiness check (`tui/claude/screen.rs::input_box`) was written against classic screens only.

## What the fullscreen layout is (captured, not assumed)
Source: the Claude docs page "Fullscreen rendering", then real captures of 2.1.300.
- Alternate screen buffer (`ESC[?1049h`) and mouse reporting; no scrollback.
- Banner at the top, the conversation under it, then blank rows, then the input box PINNED to the bottom
  rows: rule, `❯` row, rule, footer. A longer draft grows the box upward.
- The effort indicator (`◐ medium · /effort`) is its own row above the box (classic: in the footer).
- The `/` command list is drawn above the box (selected row `  ❯ /model`, indented).
- Dialogs (permission, `/model`) replace the box. `/diff` opens a panel beside the conversation (it stops
  above the box). Ctrl+O draws the transcript with no box at all.
- The prompt row is `❯` + a no-break space (the classic one is a plain space).

## How it was captured (nothing on the user's account, no window)
- `ptycap` (tests/corpus/tools/ptycap/src/main.rs): runs a command in a ConPTY, sends keys at given times,
  saves the raw bytes (and timed events). No window opens.
- A scratch `CLAUDE_CONFIG_DIR` (never `~/.claude`) with onboarding and folder trust pre-set, a fake
  `ANTHROPIC_API_KEY`, `CLAUDE_CODE_NO_FLICKER=1`, and for turns `ANTHROPIC_BASE_URL` pointing at
  `tests/corpus/tools/fake-messages-api.mjs`, a local stand-in for the Messages API (a Bash tool call, then a text
  reply). The UI is the real one; no network or account is used. `--permission-mode default` shows the
  permission dialog.
- Unset `CLAUDE_CODE_CHILD_SESSION` when capturing from inside another Claude Code (else a "transcript saving
  is off" notice appears).

## Result
The recogniser already reads this layout correctly, so no recogniser change was needed. The captures are
pinned as fixtures (`tui/claude/fixtures/fullscreen_2_1_300_live_*.ansi`) with tests
(`tui/claude/fullscreen_tests.rs`) and one corpus cast (`tests/corpus/claude/2.1.300/fullscreen_task_into_pinned_box.cast`):
empty box (120 and 52 columns) = Empty; typed text, a three-row draft, text under the `/` list = HasText;
a turn running (spinner row, `esc to interrupt`) = Working; permission dialog, `/model` picker and transcript
mode = Unknown (parks, nothing is typed); `/diff` panel beside the chat = Empty; a finished turn = Empty.
Synthetic (labelled in the test): the faint next-prompt suggestion spliced into the box, because a real
suggestion needs a real model turn.

## Not covered
- A real model-written suggestion and real tool output in fullscreen (the stand-in API only makes a Bash call).
- Panes read through scrollback (`read_terminal_buffer`): the alternate screen has none, so a transcript read
  of a fullscreen Claude only sees what is on screen.
