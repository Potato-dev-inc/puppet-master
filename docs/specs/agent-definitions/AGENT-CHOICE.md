# Which real agent proves the feature: Kilo Code

Decision: **Kilo Code** (`@kilocode/cli`). Checked 2026-10-06 on the development machine (Windows 11).

## Detection

| Candidate | Check | Result |
|---|---|---|
| Kilo Code | `which kilo kilocode kilo-code` | found: `C:\Users\Ren-pc\AppData\Roaming\npm\kilo` and `kilocode` (npm shims: `kilo`, `kilo.cmd`, `kilo.ps1`, same for `kilocode`) |
| Kilo Code | `npm ls -g --depth=0` | `@kilocode/cli@7.0.50` |
| Command Code | `which command-code commandcode cmdcode` | NOT on PATH |
| Command Code | `npm ls -g --depth=0` | not listed |

Command Code is not installed, so its flags are not verified here and it is not the proof agent. To try it
later, install it per its own docs (typically `npm install -g command-code`), check `command-code --help` for
a headless/print mode and a JSON output format, and write a second definition; if it has no machine-readable
mode it is a TUI-only definition.

If Kilo were missing: `npm install -g @kilocode/cli` (the `install_hint` in the definition), then
`kilo --version` should print `7.0.50` or later. Kilo needs a provider/credential or a free model:
`kilo models` lists `kilo/...` models and `kilo auth list` shows credentials (this machine has 0 stored
credentials and one `OPENROUTER_API_KEY` environment variable).

## Why Kilo

- A real headless mode with a JSON event stream (`kilo run --format json`), a documented session resume
  (`-s <id>`), model selection (`-m provider/model`), and an auto-approve switch (`--auto`).
- Installed here, so every claim below was run, not read.
- It is an OpenCode derivative, so it exercises a realistic TUI without being one of the existing code profiles.
- It has a Windows packaging wrinkle (npm `.cmd` shims, native `.exe` one level down) that the definition format
  must handle to be credible.

## Relevant CLI flags (from `kilo --help`, `kilo run --help`, version 7.0.50)

Top level: `kilo [project]` starts the TUI. Other commands: `run`, `serve` (headless server), `acp` (Agent Client
Protocol server), `attach`, `models`, `agent` (manage agents), `session`, `export`, `import`, `stats`, `auth`,
`mcp`, `debug`, `db`, `upgrade`, `pr`.

`kilo run [message..]` (headless):

| Flag | Meaning | Use in the definition |
|---|---|---|
| `--format default|json` | `json` = raw JSON events, one per line | `["run","--format","json"]` |
| `-m, --model provider/model` | model for this run | `when="model"` -> `-m {model}` |
| `-s, --session <id>` | continue a session by id | `when="resume"` -> `-s {session}` |
| `-c, --continue` | continue the last session | not used (ambiguous with several runs) |
| `--fork` | fork when continuing | not used |
| `--agent <name>` | agent profile | `ask` is read-only, `plan` is NOT (T05 checks 4-5) |
| `--auto` | auto-approve permissions that would otherwise `ask` | not needed for shell/edit in the project dir; needed only for `ask`-level permissions (T05 checks 1-3) |
| `--dir <path>` | working directory | not needed; the profile sets the process cwd |
| `--variant` | provider reasoning effort | possible future `{reasoning}` mapping |
| `--thinking` | show thinking blocks | off |
| `-f, --file` | attach files | not used |
| `--attach <url>`, `--port` | attach to or start a server | not used |
| `--title`, `--share` | session title / sharing | not used |

TUI mode: `kilo [project]` with `-m`, `-s`, `-c`, `--agent`, `--prompt <text>`.

Primary agents listed by `kilo agent list`: `ask`, `code`, `compaction`, `debug`, `orchestrator`, `plan`,
`summary`, `title` (subagents: `explore`, `general`).

## Verified behaviour (commands actually run, from `C:\Users\Ren-pc\AppData\Local\Temp\kilotest`)

1. Success: `kilo run --format json -m kilo/kilo-auto/free "reply with exactly: pong"` printed three JSON lines:
   `step_start`, `text` (`part.text` = `"\n\npong"`), `step_finish` (`part.reason` = `"stop"`, `part.tokens`,
   `part.cost`). Every event has top-level `type`, `timestamp`, `sessionID` and a `part` object. The process exited
   on its own.
2. Resume: `... -s ses_ef089e9eeffeoUyY8n6lLbXVvm "what did I ask you to reply with before? also read a.txt"` on the
   same session answered "You asked me to reply with exactly: pong" and read the file: the session carried over. The
   event sequence was `step_start`, `text`, `tool_use` (with `part.tool`, `part.state.status`,
   `part.state.input/output`), `step_finish` (`reason` = `"tool-calls"`), `step_start`, `text`, `step_finish`
   (`reason` = `"stop"`). So the final answer is the LAST `text` event; the turn is over at the first
   `step_finish` with `reason = "stop"`.
3. Error: with no model flag, the default model was rejected by OpenRouter and Kilo printed
   `{"type":"error","timestamp":...,"sessionID":"ses_...","error":{"name":"APIError","data":{"message":"No endpoints
   found that support tool use. ...","statusCode":404,...}}}`. A second attempt of the same command had not
   exited after 60 s, so an error can leave the process running; the definition treats an `error` event as a finished
   turn and the runner must apply its own timeout.
4. Task starting with a dash: `kilo run --format json -m kilo/kilo-auto/free -- "-reply with exactly: dash"`
   ran the text as the task and answered `dash`. `--` is safe as `task_separator`.
5. The model is NOT echoed in the JSON stream (no model field in `step_finish`), so `[model] source = "none"`.
6. Windows binary: `where kilo` finds only `kilo`, `kilo.cmd`, `kilo.ps1`. The native executable exists at
   `C:\Users\Ren-pc\AppData\Roaming\npm\node_modules\@kilocode\cli\node_modules\@kilocode\cli-windows-x64\bin\kilo.exe`
   (also a `-baseline` variant). `tui::executable("kilo")` would fail on this machine because it only looks for
   `kilo.exe` on PATH; the definition's `shim_native_globs` solves that (SPEC 7.3).

## Verified in T05 (PLAN P0-5, run 2026-10-07, kilo 7.0.50, from `C:\Users\Ren-pc\AppData\Local\Temp\kilotest`)

All headless runs used `kilo run --format json -m kilo/kilo-auto/free -- "<task>"` plus the flags named. The raw
outputs are in the scratch dir (`*.jsonl` / `*.err`, one pair per check); they are not fixtures (T21 saves those).

1. Shell without `--auto`: task "run `echo hello-from-bash > bash_noauto.txt` with your bash tool". `tool_use`
   `bash` `status=completed`, `metadata.exit=0`, file created, exit code 0. Shell inside the project dir does NOT
   need `--auto`.
2. Edit without `--auto`: task "change a.txt with your edit/write tool". `write` first failed with Kilo's own
   "You must read file ... before overwriting it" (not a permission), then `read` + `write` completed and the file
   changed. Edit/write inside the project dir do NOT need `--auto`.
3. Permission `ask` without and with `--auto`: task "read C:\Windows\win.ini". Without `--auto`, stderr printed
   `! permission requested: external_directory (C:/Windows/*); auto-rejecting`, the tool event had
   `status=error`, `error="... The user rejected permission to use this specific tool call."`, the LAST `text` was
   `""`, the LAST event was `step_finish` with `reason="tool-calls"` (no `reason="stop"` at all), and the process
   exited 0. With `--auto` the read completed and the answer was correct. So `--auto` only changes rules whose
   action is `ask` (by default: `external_directory`, `doom_loop`, `read *.env*`); without it those are rejected,
   never prompted.
4. `--agent plan` is NOT read-only. `kilo debug agent plan` shows `edit * deny` (plan files excepted) but
   `bash * allow`. Task "create a file with write, else with bash": with `--auto` the `write` call was denied by a
   rule and `bash` then wrote `plan_auto_bash.txt`. Without `--auto` the model refused on its own (prompt-level
   restraint only), called `plan_exit` (which wrote a plan under `%USERPROFILE%\.local\share\kilo\plans\`), emitted
   `step_finish reason="stop"`, and then the process did NOT exit within 150 s (killed by `timeout`, exit 124).
5. `--agent ask` IS read-only by configuration. `kilo debug agent ask` shows tools `bash=false, edit=false,
   write=false, task=false, todowrite=false` and permission `* * deny` with allows only for read, grep, glob, list,
   question, webfetch, websearch, codesearch. The same create-a-file task with and without `--auto` made no tool
   call, created no file, ended with `reason="stop"` and exit 0. It can still reach the network (webfetch/websearch).
6. Exit code on an `error` event: never a usable signal. (a) No `-m` (default model -> OpenRouter `APIError` "No
   endpoints found that support tool use"), run twice: one `error` line on stdout, then the process never exited
   (both killed at 90 s, exit 124). (b) `-m nosuchprovider/nosuchmodel`: one `error` line
   (`error.name="UnknownError"`, `error.data.message="Model not found: ..."`), a Bun stack trace on stderr, exit 0.
   No orphaned `kilo.exe` remained after the `timeout` kills.
7. TUI `--prompt`: `kilo -m kilo/kilo-auto/free --prompt "reply with exactly: tuipong"` in a puppet-master PTY pane
   (120x30) auto-submitted the prompt and the reply `tuipong` rendered with no extra keypress. Captured screen text:
   reply footer `▣  Code · kilo-auto/free · 3.7s` (the model IS visible in the TUI, unlike the JSON stream); status
   line `Code  Auto Free Kilo Gateway  ...  /~\AppData\Local\Temp\kilotest`; bottom line
   `tab agents  ctrl+p commands    • Kilo 7.0.50` (with leading spaces); a right-hand sidebar with the session
   title, `Context`, token count, `% used`, `$0.00 spent`, `LSP`. Ctrl+C quit the TUI.
8. TUI and headless share sessions: the TUI session above (`ses_ee9bedb4cffe36fb8JcCNtSkYr`, listed by
   `kilo session list`) was resumed headlessly with `-s` and answered `tuipong`.

## Assumptions (not checked; T23 must confirm with real PTY captures)

- TUI launched with `-s <id>` opens that session. Not run: the coordinator forbade further visible panes during
  T05. Evidence for: headless `-s` works and the session store is shared with the TUI (check 8).
- The TUI working indicator (`esc interrupt` is an OpenCode convention), the permission dialog wording and keys,
  and the empty-input marker. Not captured. The draft `[tui.busy]` and `[[tui.prompt]]` regexes stay guesses.
- The TUI permission dialog only appears for `ask`-level rules (check 3), so a T23 capture must provoke one, for
  example by reading a file outside the project dir.

## Contradictions with the draft `kilo.toml` / SPEC 7.2 and SPEC 10 (edits needed; SPEC not edited here)

1. SPEC 10 assumption 4 is false in both halves: shell does not need `--auto` (checks 1-2), and `plan` is not
   read-only (check 4). Edit SPEC 10 to say so.
2. `allow_shell` -> `--auto` does not control shell. Omitting `--auto` leaves bash/edit/write enabled in the
   project dir; it only stops `ask`-level actions (outside-dir access, `.env` reads, doom-loop). SPEC 7.2's comment
   `allow_shell = true # maps to --auto` and SEC rules that treat `--auto` as the shell switch should say that
   `--auto` = "approve `ask`-level permissions"; the only way to disable shell is a restricted agent.
3. Read-only is possible via `--agent ask` (check 5): add `[[headless.optional]] when = "read_only"`,
   `args = ["--agent", "ask"]` and only then flip `read_only_headless = true` (SEC-8 needs that segment). Never use
   `plan` for read-only. Note at approval time that `ask` can still use the network.
4. `[headless.output.done]` (`step_finish` + `reason="stop"`) is not always emitted: a turn whose tool call was
   auto-rejected ends with `reason="tool-calls"` and a clean exit (check 3). The runner must also treat process
   exit as end of turn, and must not wait for exit after the `done` event either (check 4: `stop` then no exit).
5. `[headless.output.result] pick = "last"` can yield `""` (check 3). Prefer the last non-empty `text`, or
   surface the stderr `auto-rejecting` line as the reason when the result is empty.
6. Error handling: the draft's "an `error` event ends the turn, the runner applies its own timeout and kills the
   process" stands; exit code must not be used for success/failure (check 6: 0 or never).
7. `[tui.chrome] line_regex = ["^tab agents"]` misses the real footer because the line starts with spaces (check
   7): use `^\s*tab agents\s+ctrl\+p commands` or match on trimmed lines. `^ctrl\+[a-z] ` matched nothing
   observed. The sidebar lines (`Context`, `% used`, `$... spent`, `LSP`) are chrome too.

## Draft definition (`kilo.toml`)

Identical to `SPEC.md` section 7.2; repeated here so this file stands alone. Place it at
`%APPDATA%\com.puppetmaster.app\agents\kilo.toml`.

```toml
schema_version = 1

[agent]
id = "kilo"
label = "Kilo Code"
icon = "KC"
description = "Kilo Code CLI (@kilocode/cli), an OpenCode-derived agent with a JSON event stream."

[binary]
names = ["kilo", "kilocode"]
windows_extensions = ["exe", "cmd"]
shim_native_globs = ["node_modules/@kilocode/cli/node_modules/@kilocode/cli-windows-x64*/bin/kilo.exe"]
version_args = ["--version"]
install_hint = "npm install -g @kilocode/cli"

[features]
dispatchable = true
read_only_headless = false
resumable_headless = true

[run_options]
allow_shell = true

[headless]
argv = ["run", "--format", "json"]
task_separator = "--"

[[headless.optional]]
when = "model"
args = ["-m", "{model}"]

[[headless.optional]]
when = "resume"
args = ["-s", "{session}"]

[[headless.optional]]
when = "allow_shell"
args = ["--auto"]

[headless.output]
format = "json_lines"

[headless.output.result]
where = { type = "text" }
path = "part.text"
pick = "last"

[headless.output.session]
where = { "~sessionID" = true }
path = "sessionID"
pick = "first"

[headless.output.done]
where = { type = "step_finish", "part.reason" = "stop" }

[headless.output.error]
where = { type = "error" }
path = "error.data.message"

[model]
source = "none"

[tui]
argv = []
idle_after_ms = 2500
quiet_idle_after_ms = 1200
clear_input_keys = "\u0015"

[[tui.optional]]
when = "model"
args = ["-m", "{model}"]

[[tui.optional]]
when = "resume"
args = ["-s", "{session}"]

[tui.busy]
working_regex = ["esc\\s+interrupt"]

[tui.chrome]
line_regex = ["^ctrl\\+[a-z] ", "^tab agents"]

[[tui.prompt]]
kind = "command_approval"
detect_regex = "permission required"
choices = [
  { id = "allow_once", key = "enter" },
  { id = "deny", key = "escape" },
]

[context]
smartness = 6
strengths = ["implementation", "debugging"]
notes = ["Model is chosen as provider/model, for example kilo/kilo-auto/free; the JSON stream does not echo it."]
```

Test model for live runs: `kilo/kilo-auto/free` (free tier; listed by `kilo models`). The default model on this
machine fails with the OpenRouter 404 above, so always pass `model` in live proofs.
