# Writing an agent definition (author guide)

Audience: you want Puppet Master to run a terminal coding agent that it does not know yet. You write one
`.toml` file; no code changes. The formal schema and requirements are in `SPEC.md` (section 7.1); this
guide is the practical version. Status: design target, matches SPEC schema version 1.

## 1. How it works in one minute

1. You put `myagent.toml` in a definitions folder (see 2).
2. Puppet Master validates it (Settings > Agents shows errors with the file and line).
3. You approve it once. The approval dialog shows the exact command line that will be run. Edit the
   command line later and it asks again.
4. `myagent` shows up in the spawn menu and as a valid `agent_type` for `run_agent`, `delegate_work`,
   `followup_task` and every other MCP tool. Check with `GET /agents/types`.

A definition can describe two ways to drive the agent:

- **headless**: Puppet Master runs one command per turn, reads machine-readable output, and gets the
  result, a session id (for follow-ups) and errors from it. Most reliable.
- **visible (TUI)**: Puppet Master starts the agent in a terminal pane, types the task, and reads the
  reply from the screen using the patterns you give it. Needed when the agent has no JSON mode, or when a
  human wants to watch.

You can provide either or both.

> **Implementation status (T26, checked against the code).** Working now: bundled (empty) and user sources,
> validation with line/column errors, approval pinned to the fingerprint and resolved binary, reload with a diff,
> Settings > Agents (list, errors, Reload, Approve dialog with rendered argv, Revoke). Not yet (T34): project and
> extra sources (the table above describes the target; `consent` answers 501 `AGENT_DEF_CONSENT_UNSUPPORTED`,
> and the Sources list shows them disabled), adding extra folders in Settings, the approval diff view
> and "Check with fixtures" in the UI (section 10 step 4: use `cargo test --lib tui::declarative`). The validate
> request body is `{"toml": ..., "source": ...}`; `rendered.headless_argv` omits the program name. The
> routes other than the list (validate, reload, approve, revoke, consent) are refused for MCP sessions and remote devices.

## 2. Where to put the file

| Source | Folder | Default | Needs |
|---|---|---|---|
| Bundled | compiled into the app | on | nothing (trusted) |
| User | `<app data>/agents/*.toml` (Windows: `%APPDATA%\com.puppetmaster.app\agents\`) | on | approval |
| Project | `<project>/.puppet-master/agents/*.toml` | OFF | consent for that project + approval |
| Extra | folders you add in Settings > Agents | OFF | consent per folder + approval |

Only files directly in the folder are read (no subfolders), at most 64 per folder. Files in
`agents/drafts/` are never loaded.

If two files use the same id, the nearest source wins (project, then extra, then user, then bundled) and the
others show as `shadowed`. You cannot reuse a built-in id (`claude`, `codex`, `opencode`, `opencode_native`,
`cursor`, `cursor_agent`, `cmd`, `powershell`, `bash`).

## 3. The smallest useful file (visible only)

```toml
schema_version = 1

[agent]
id = "mycoder"            # lowercase letters, digits, underscore; 2-32 chars; starts with a letter
label = "My Coder"
experimental = true       # shown with an "experimental" badge

[binary]
names = ["mycoder"]       # bare program names, no paths, no extensions
install_hint = "npm install -g mycoder"

[tui]
argv = []                 # arguments after the program
```

That alone gives you a pane running `mycoder` and the shared screen heuristics. Add tables below as you
learn how the agent behaves on screen.

## 4. Field reference (condensed; full table in SPEC 7.1)

### `[agent]`
`id`, `label` (required); `icon` (1-4 chars), `description`, `kind` (`cli` default, or `generic`),
`experimental`, `allow_interpreter` (see 9).

### `[binary]`
- `names`: bare file names, tried in order. No `/`, `\`, `..`.
- `windows_extensions`: default `["exe","cmd","bat"]`.
- `shim_native_globs`: when the install is a `.cmd` wrapper (typical for npm), globs (relative to the
  wrapper's folder) that find the real `.exe`. See 8.
- `version_args`, `install_hint`.

Puppet Master looks for the program on `PATH` only. It never runs something found in the project folder or
the current directory.

### `[features]`
`dispatchable` (default true), `read_only_headless`, `read_only_spawned_pane`, `resumable_headless` (derived;
do not set it unless you want the check), `read_only_note`.
Only claim read-only if the CLI has a documented read-only mode AND you map it in argv with
`when = "read_only"`. You must also fill `read_only_note` with one sentence saying how.

### `[run_options]`
`allow_shell`, `shell_default_on`, `trust_workspace`, `allowed_commands`. Each only works if the matching
`when` segment exists (4.2) or, for `shell_default_on`, if the agent already runs shell commands unprompted.

### `[env]`
`set = { KEY = "value" }`, `unset = ["KEY"]`. Some keys are refused (`PATH`, `LD_PRELOAD`, `NODE_OPTIONS`,
`PYTHONPATH`, ...).

### 4.1 `[headless]` and placeholders
```toml
[headless]
argv = ["run", "--format", "json"]   # everything before the task
task_separator = "--"                 # inserted before the task, so a task starting with "-" is text
```
Placeholders (closed list): `{task}`, `{project}`, `{model}`, `{session}`, `{reasoning}`. Each is replaced
inside ONE argument, so a task with spaces or quotes is never split and never reaches a shell. Where the task
goes:

- default: appended as the last argument (after `task_separator` if set),
- `task_flag = "--prompt"`: appended as `--prompt <task>`,
- `[headless.stdin] template = "{task}"`: written to the process's standard input.

You must pick exactly one. A positional task with neither a separator nor a flag is rejected, because it
could be misread as an option.

### 4.2 Conditional arguments
```toml
[[headless.optional]]
when = "model"            # only when the caller asked for a model
args = ["-m", "{model}"]
```
`when` is one of `model`, `resume`, `read_only`, `not_read_only`, `allow_shell`, `trust_workspace`. Segments
are added in file order. A `resume` segment plus a `session` selector (5) makes follow-ups resume the same
conversation.

## 5. Output parsers (headless)

Set `[headless.output] format` to one of three kinds.

### `plain_text`
The result is the text the program printed (ANSI codes removed). Narrow it with `result.regex` (group 1, or a
group named `value`) and `result.pick` (`first` or `last` match).
```toml
[headless.output]
format = "plain_text"
[headless.output.result]
regex = "(?s)ANSWER:\\s*(.*)\\z"
```

### `json_single`
The output is one JSON object (or the last JSON object in the output). Select with a dotted path.
```toml
[headless.output]
format = "json_single"
[headless.output.result]
where = { type = "result", is_error = false }
path = "result"
[headless.output.session]
path = "session_id"
```

### `json_lines`
The output is one JSON object per line (an event stream). Every selector runs over the events in order; pick
`first`, `last`, `join`, or `max_by`.
```toml
[headless.output]
format = "json_lines"

[headless.output.result]          # the answer: the last text event
where = { type = "text" }
path = "part.text"
pick = "last"

[headless.output.session]         # the id for follow-ups: first event that has one
where = { "~sessionID" = true }   # "~" = the field exists
path = "sessionID"
pick = "first"

[headless.output.done]            # the turn is over as soon as this appears, even if the process lingers
where = { type = "step_finish", "part.reason" = "stop" }

[headless.output.error]           # an error event fails the run even if the exit code is 0
where = { type = "error" }
path = "error.data.message"
```

### Selector fields
`where` (exact matches on dotted paths), `where_regex` (regex match on a path), `path` (what to extract:
`a.b.0.c`, numbers index arrays), `pick` (`first`, `last`, `join` with `join_with`, `max_by` with
`max_by = "some.numeric.path"` to take the event with the largest number, for example a cumulative token
count or a sequence number), `trim`, `regex`.

Rules worth knowing:
- Non-zero exit codes never yield a result.
- If the process exits 0 but no result selector matched, the run fails with `AGENT_OUTPUT_UNPARSEABLE`;
  Puppet Master does not guess from raw text.
- Output is capped at `max_output_bytes` (8 MiB default).

## 6. Visible (TUI) tables

All patterns are Rust regular expressions (see 7). Tables you can add under `[tui]`:

| Table | What it does | Input the regex sees |
|---|---|---|
| `[tui.ready]` `regex`, `not_regex` | the empty input box is showing (agent can take a task); dialogs that mean "not ready" | last visible lines |
| `[tui.busy]` `working_regex`, `idle_regex` | the agent is working / provably not working | last 12 visible lines |
| `[tui.chrome]` `line_regex` | banners, footers, hints that are not reply text | one trimmed, LOWERCASED line |
| `[tui.tool_activity]` `line_regex` | rows like "ran command" printed above the reply | one trimmed, LOWERCASED line |
| `[tui.menu]` `regex` | an open selector, so "wait for settled" does not call it idle | whole screen |
| `[tui.dialogs]` `strip_blocks` | removes boxed dialogs (trust/approval) from reply text | screen text |
| `[tui.footer_model]` `regex` | live model name, with a group named `model` | whole screen |
| `[tui.classify]` `waiting_regex` | force a "waiting for input" status | last visible lines |
| `[[tui.prompt]]` | approval or trust prompts and how to answer them | whole screen |

Because chrome and tool-activity lines are lowercased before matching, write those regexes in lowercase.
Timing: `idle_after_ms` (default 2500, allowed 1500-5000) is how long the screen must hold still to count
as idle; `quiet_idle_after_ms` (500 up to `idle_after_ms`) applies when `busy` says nothing is working.

### Approval prompts
```toml
[[tui.prompt]]
kind = "command_approval"
detect_regex = "permission required"
choices = [
  { id = "allow_once", key = "enter" },
  { id = "deny",       key = "escape" },
]
```
`id` values `allow_once` and `deny` are always answerable. A choice `allow_all` needs `broad = true` on the
prompt and is only offered when the caller explicitly allows broad approvals; it is never chosen
automatically. `key` is a named key (`enter`, `escape`, `y`, `n`, ...); use `text` instead to type a string.
Add `not_when_regex` for the text that remains in the scrollback after the dialog was answered, otherwise the
same dialog is detected again.

## 7. Regex tips

- Engine: Rust `regex`. Linear-time and safe, but NO look-ahead/look-behind and NO backreferences.
- In TOML, use single-quoted literal strings to avoid doubling backslashes: `'esc\s+interrupt'`. In a
  double-quoted string write `"esc\\s+interrupt"`.
- Anchor chrome patterns (`^`, `$`) so they cannot swallow a reply line that merely contains the words.
- Prefer a few specific patterns over one clever one; each regex must stay small (compiled size limit 256 KiB,
  at most 200 per file).
- Matching is case-insensitive for `detect_regex`; chrome and tool-activity inputs are already lowercase.
- Test patterns against real captures (section 10), not memory. TUIs change between versions.
- Box-drawing and spinner glyphs are real characters in the capture; copy them from it.

## 8. Finding the program on Windows (npm-installed CLIs)

npm installs wrappers (`name.cmd`, `name.ps1`) and not an `.exe`. Wrappers cannot be launched safely with
arbitrary text arguments, so by default they are refused (`AGENT_DEF_SHIM_NEEDS_NATIVE`). Most npm CLIs ship a
real binary in a platform package next to the wrapper; point at it:

```toml
[binary]
names = ["kilo", "kilocode"]
windows_extensions = ["exe", "cmd"]
shim_native_globs = ["node_modules/@kilocode/cli/node_modules/@kilocode/cli-windows-x64*/bin/kilo.exe"]
```
Find the path with `dir %APPDATA%\npm\node_modules\<pkg>\node_modules` on your machine. The glob is relative
to the folder holding the wrapper. If an agent only has a wrapper, set `[binary] allow_cmd_shim = true` and
deliver the task through `[headless.stdin]`; this is refused for project and extra sources.

## 9. Things that are refused (on purpose)

- Shells and interpreters as the program (`bash`, `cmd`, `powershell`, `python`, `node`, ...) unless
  `allow_interpreter = true`, which is also refused in project and extra sources and shows a warning at approval.
- Paths in `binary.names`; `PATH`/`LD_PRELOAD`/`NODE_OPTIONS`-style environment keys.
- Unknown fields anywhere (a typo is an error, never ignored).
- Anything that is not an argument list: there is no shell string, no hook, no script field.
- Files over 64 KiB; more than 200 regexes; more than 64 arguments per command.

## 10. Testing your definition with fixtures

1. **Validate**: Settings > Agents shows errors, or `POST /agents/definitions/validate` with
   `{"toml": "...", "source": "user"}`. The response also shows the rendered argv for headless and visible
   mode (with `<task>`/`<project>` placeholders).
2. **Capture real output** (never hand-write it):
   - headless: run the exact rendered command in a terminal, redirect output to a file
     (`> headless_success.jsonl`), scrub secrets and personal paths;
   - visible: start the agent in a pane and use the pane's buffer (`read_terminal_buffer`) at each state:
     idle, working, permission dialog, finished reply. Save as `tui_idle.txt`, `tui_working.txt`,
     `tui_permission.txt`, `tui_reply.txt`.
   - Also capture an error (wrong model, no network) and a garbage case (random text).
3. **Describe expectations** next to each capture in `<name>.expect.toml` (every capture needs one; a
   capture without it, or an expect file without a capture, is reported as a failure). Headless captures
   (`headless*`, or `kind = "headless"`):
   ```toml
   result = "pong"          # extracted result; "" means none
   session = "ses_example"  # provider session; "" means none
   model = "provider/model" # optional
   done = true              # headless_result_ready
   exit = 0                 # optional exit code given to the parser (default 0)
   error_code = "AGENT_REPORTED_ERROR"   # classified failure, for captures that fail
   error = "Model not found: x."         # its message
   ```
   Screen captures (`tui*`, or `kind = "screen"`; a leading `# capture:` comment line is ignored):
   ```toml
   busy = true                      # true, false, or "unknown" (no footer: not busy, not idle)
   menu = false
   prompt_kind = "command_approval" # "" means no prompt
   model = "Nemotron 3 Ultra (free)"
   chrome_lines = ["9% used"]       # must be chrome rows
   not_chrome_lines = ["the answer"]
   ```
4. **Run the check**: Settings > Agents > Check with fixtures (set the folder, or leave it empty to use a
   folder named after the file next to it), or put the files under
   `packages/app/src-tauri/src/tui/declarative/fixtures/<id>/` next to `tests/agent-definitions/<id>.toml`
   and run `cargo test --lib tui::declarative::expect`. Only regular files directly inside the folder are read.
5. The shared conformance rules run on every definition automatically: unique id, parsers reject garbage and
   non-zero exits, every detected prompt can be answered, an empty screen is never "blocked", idle windows are in
   range. A failure names the rule.

## 11. Troubleshooting

| Symptom | Likely cause | Fix |
|---|---|---|
| State `invalid`, `AGENT_DEF_UNKNOWN_FIELD` | typo or a field from a newer schema | read the path in the error; check section 4 |
| `AGENT_DEF_BAD_REGEX` | look-around/backreference, unescaped `(` or `\` | use single-quoted TOML strings; simplify |
| `AGENT_DEF_UNAPPROVED` | first load or argv changed | open Settings > Agents, review the shown command, approve |
| `AGENT_DEF_APPROVAL_STALE` | you edited argv/binary/env after approving | approve again; the dialog shows the diff |
| `AGENT_DEF_BINARY_CHANGED` | a different executable now sits on PATH | verify it, approve again |
| `AGENT_DEF_SHIM_NEEDS_NATIVE` | npm `.cmd` wrapper, no `.exe` found | add `shim_native_globs` (section 8) |
| Agent listed but `installed: false` | not on PATH for the app (GUI apps may not see your shell PATH) | install it system-wide or fix PATH and restart the app |
| `AGENT_OUTPUT_UNPARSEABLE` | the selector no longer matches the output (CLI changed) | recapture output, fix `where`/`path` |
| `AGENT_REPORTED_ERROR` with a model/provider message | the CLI printed an error event (bad model, no credit) | fix the CLI configuration; try `model` explicitly |
| Result is empty or has banner text (visible) | chrome/tool-activity patterns missing | add lowercase `line_regex` entries from a real capture |
| Turn never finishes (visible) | `busy` pattern too eager, or screen keeps animating | tighten `working_regex`, raise `idle_after_ms` |
| Turn finishes too early (visible) | spinner frames look idle | add `working_regex` for the spinner; keep `idle_after_ms` >= 2000 |
| Dialog answered twice / re-detected | answered dialog text stays on screen | add `not_when_regex` |
| Follow-up starts a new conversation | no `resume` segment or no `session` selector | add both |
| `READ_ONLY_UNSUPPORTED` | `read_only_headless` not set | only set it if the CLI really enforces it (note required) |
| Old behaviour after saving | reload failed and kept the previous version | see the error list; press Reload |

## 12. Complete example (headless + visible): Kilo Code

See `SPEC.md` 7.2 for the full annotated file and `AGENT-CHOICE.md` for the verified CLI flags and the exact
JSON events it prints.
