# pm-fake-tui

A scriptable **fake agent TUI** for Puppet Master's end-to-end tests. It runs on a real console (ConPTY when a pane spawns it), draws the screens of Claude Code or Codex as they were captured live, and plays a script. It spends no tokens and is deterministic, so a vendor quirk (a menu that opens 600 ms late, an Enter swallowed by a paste burst, a dimmed suggestion in the composer) is encoded once and kept.

It is a development tool. Nothing in the shipped app refers to it; the app side lives behind the `dev-fake-tui` cargo feature (`src-tauri/src/fake_tui/`) and is exercised by `src-tauri/tests/fake_tui_e2e.rs`.

```text
pm-fake-tui [--agent claude|codex|nimbus] [--script FILE.toml|FILE.json] [--check FILE]
```

`--script` falls back to the `PM_FAKE_TUI_SCRIPT` environment variable. With no script it is a bare agent that answers every prompt with "OK". `--check FILE` validates a script and exits. `PM_FAKE_TUI_LOG=<file>` (or `log = "<file>"` in the script) appends one JSON line per event: `start`, `ready`, `key`, `prompt`, `approval_shown`, `approval_answer`, `gate_shown`, `gate_answer`, `write_file`, `limit`, `interrupted`, `turn_done`, `exit`.

## What it draws

| Layout | Screens |
|---|---|
| `claude` (2.1.299 classic) | banner, `❯` composer between two rules (no-break space after the glyph), the rotating `Try "..."` hint, a dimmed suggestion, the activity row `✢ Generating…` with `esc to interrupt` in the footer, `✻ Churned for 3s · done 15:27`, the folder-trust dialog, the `Bash command` approval, the `Edit file` / `Create file` approval, the `Press Ctrl-C again to exit` hint, the usage-limit line and the "continuing automatically ... esc to cancel" wait |
| `codex` (0.162.1) | `>_ OpenAI Codex` banner (with `loading`), `›` composer with the dimmed "Ask Codex to do anything", the update menu (Update now / Skip / Skip until next version), the trust question, `• Working (0s • esc to interrupt)`, `• Failed (exit -1) <command>` with its reason, `Worked for 9s • 8:26 PM`, the `■` usage-limit line |
| `nimbus` (made up) | An agent no profile knows, for the Agent Wizard's end-to-end tests (`src-tauri/tests/fake_tui_wizard.rs`): a banner row starting `NIMBUS 0.4.2`, the folder question `Do you trust the files in this folder?` (`1. Yes, trust this folder` / `2. No, exit`; keys `1`, `2`, Enter), a boxed input row (a `+---+` rule above and below, `> Type a task...` inside it with a dimmed placeholder), a braille spinner row `<spinner> Generating...  3s  ctrl+c to stop`, `> <your message>` and `reply: <text>` rows, `tool:` rows, `done in 3s`, the question `Allow nimbus to create hello.txt? [y/n]` (keys `y`, `n`), and `/exit` quits. In a script, `{last_word}` inside `say` or `write_file` text is the last word of the prompt that started the turn. A `hold {text}` step shows rows and stops: no composer, no spinner, nothing moving, until any key ends the turn. |

Wording and glyphs are copied from the captures in `src-tauri/src/tui/{claude,codex}/fixtures/` and from `D:\pm-night\DESIGN-RELAY.md` §1.2 (limit texts). The app's tests read every screen back through the REAL `TuiProfile`, so a screen the profile stops recognising fails there.

## Keys

Typing goes into the composer (Backspace, Ctrl+U, bracketed paste work). In the `claude` layout a paste (a bracketed paste, or text arriving with no more than 50 ms between keys, as a program's write does) of more than 800 characters or more than `min(rows - 10, 2)` line breaks shows as `[Pasted text #N +M lines]`, as Claude Code 2.1.302 draws it; Enter sends the whole text, and Backspace removes the placeholder whole. Enter submits, except inside the **paste burst**: Codex reads an Enter within 120 ms of typed text as a line break (`paste_burst_ms`, `paste_burst_effect = "newline" | "drop"`). Esc or Ctrl+C interrupts a running turn (`interrupt = false`: the agent ignores it). In a Claude dialog Up/Down move the pointer, Enter confirms it, `1`-`9` select and confirm, Esc refuses. In Codex dialogs `y` / `a` / `n` answer too. Ctrl+C on an empty composer twice exits.

## Script format

```toml
agent = "claude"              # "claude", "codex" or "nimbus": which layout to draw
version = "2.1.299"           # banner (optional)
model = "Haiku 5.5 · Claude Pro"
boot_ms = 300                 # loading screen before anything else
paste_burst_ms = 120          # default: 120 for codex, 0 for claude
interrupt = true

[[startup]]                   # shown before the composer takes input, in order
gate = "update_menu"          # "update_menu" (codex) or "trust" (both layouts)
after_ms = 600                # appears this long after the previous screen
composer_first = true         # draw the composer meanwhile: the menu then opens OVER a composer that looked ready
honour_esc = true             # false: the screen ignores Esc

[[turn]]
on_prompt = "approve"         # substring of the submitted prompt, case-insensitive; omit = any prompt
steps = [
  { do = "think", ms = 600, verb = "Pondering" },
  { do = "approve", kind = "bash", command = "npm test",
    on_allow = [{ do = "say", text = "tests pass" }],
    on_deny  = [{ do = "say", text = "stopped" }] },
  { do = "write_file", path = "out.txt", content = "hi\n" },
  { do = "suggest", text = "commit this" },
]
```

Steps (`do = ...`): `think {ms, verb}` and `wait {ms}` stay busy; `say {text}`; `tool {label, detail, ms, failed}`; `approve {kind = "bash"|"edit"|"create", command|path, diff, auto_mode_option, on_allow, on_deny}` waits for a key (a refusal drops the rest of the turn); `write_file {path, content, append, quiet}` writes under the working directory; `suggest {text}` leaves a dimmed suggestion in the composer after the turn; `limit {text, reset, auto_continue}` prints the vendor's usage-limit message and ends the turn; `exit {code}` leaves the screen as it is (the host shell's prompt appears under it); `crash {message, code}` prints a panic line and exits 101; `screen {text}` adds literal rows.

**Nimbus-only options** (top level of the script), for the screen-read agent tests:

| Key | What it does |
|---|---|
| `backlog_rows = N` | N filler rows ("earlier session note 1..N") under the banner from the start, so the banner scrolls off and a long conversation overflows the pane |
| `status_model = "name"`, `status_model_when = "always"` or `"busy"` | a model name in the footer (`model: name` before the hints) or only on the working row |
| `footer_right = "text"` | text flush against the last column of the footer row (a clipped column shows at once) |
| `inline = true`, `reprint_on_resize = true` or `false` | paint like an inline TUI: the conversation is printed top-down into the scrollback and only the visible part is redrawn; on a resize `reprint_on_resize` clears the screen and prints the WHOLE conversation again (rows that overflow reach the history a second time), otherwise only the visible part is redrawn |
| `approvals = "menus"` | a command asks with a numbered menu (`Run this command?` / `> 1. Allow once` / `2. Always allow in this project` / `3. Deny`; digits answer) and a file write with an arrow menu (`Write <path>?` / `Yes, write it` ...; arrows and Enter); default `yn` is the one-line `[y/n]` |
| `[[startup]] animate = true` | the trust question redraws while it waits: a blinking block after the question and a spinner row with a seconds counter under it |

A turn that runs out of steps ends like a real one: a summary row, then the composer. A prompt no turn matches gets the first turn without `on_prompt`, else "OK".

## Layout of the code

`script.rs` (the format) · `keys.rs` (bytes to keys) · `engine.rs` (the state machine, no terminal, no real time) · `render.rs` (the screens) · `term.rs`, `runtime.rs` (ConPTY plumbing). Everything but the last two is unit-tested without a console: `cd tools/pm-fake-tui && cargo test`.

Windows only for now (it needs a ConPTY and VT console modes); other platforms build a best-effort `stty` fallback.
