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
| `--agent <name>` | agent profile | candidates for read-only: `plan`, `ask` (not verified read-only) |
| `--auto` | auto-approve all permissions | `when="allow_shell"` -> `--auto` (needs verification, assumption 4) |
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

## Not verified (must be checked in PLAN P0-5 / P2-14)

- Whether shell/edit tools in `kilo run` need `--auto` (a `read` tool ran without any flag; `bash`/`edit` untested).
- Whether `--agent plan` (or `ask`) is truly read-only. Until proven, `read_only_headless = false`.
- The visible TUI: banner and footer text, the working indicator (`esc interrupt` is an OpenCode convention), the
  permission dialog wording and keys, the empty-input marker. The `[tui]` regexes in the draft are guesses and
  must be replaced with real PTY captures.
- Behaviour when `kilo` is launched with `-s` in TUI mode, and with `--prompt`.
- The exit code on an `error` event.

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
