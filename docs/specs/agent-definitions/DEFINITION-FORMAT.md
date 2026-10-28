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

> **Implementation status (T37, checked against the code).** Working: bundled (empty) and user sources, project and
> extra sources behind per-path consent (`agent-definitions.json`), validation with line/column errors, approval pinned
> to the fingerprint and resolved binary (with an argv diff on re-approval), reload with a diff, a hot-reload watcher,
> Settings > Agents (list, errors, Reload, Approve dialog with rendered argv, Revoke, Check with fixtures, Export/Import,
> generic add), `[mcp_host]` as a library writer (no route yet). The validate request body is
> `{"toml": ..., "source": ...}`; `rendered.headless_argv` omits the program name. The routes other than the list
> (validate, reload, approve, revoke, consent, ...) are refused for MCP sessions (403) and for anything that is not
> the local app (remote devices, tunnels, proxied or bearer requests).

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
`dispatchable` (default true), `read_only_headless`, `read_only_spawned_pane`, `reprints_on_resize` (default false: the TUI reprints its whole conversation on every resize, so viewers wipe scrollback at the resize point; the Agent Wizard MEASURES this in its capture and writes the key itself, a drafter does not guess it), `resumable_headless` (derived;
do not set it unless you want the check), `read_only_note`.
Only claim read-only if the CLI has a documented read-only mode AND you map it in argv with
`when = "read_only"`. You must also fill `read_only_note` with one sentence saying how.

### `[run_options]`
`allow_shell`, `shell_default_on`, `trust_workspace`, `allowed_commands`. Each only works if the matching
`when` segment exists (4.2) or, for `shell_default_on`, if the agent already runs shell commands unprompted.

### `[profile]`
`tested = ">=0.150, <0.170"`: the CLI versions the definition and its fixtures were checked on (a semver
range). Outside it Puppet Master runs the agent in strict mode: it stops closing start-up notices for the
person, shows an "Untested version" chip and keeps no warm copy. Optional; unset means no pin.

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
| `[tui.thinking]` `line_regex` | rows the agent prints while it thinks or works (`Thinking...`, a status spinner row, `Thought for 4s`), on screen during the turn or left in the transcript after it | one trimmed, LOWERCASED line |
| `[tui.summary]` `line_regex` | a row the agent prints ONCE, after a turn has ended (`Thought for 3s, 298 tokens`, `Worked for 9s`, a cost or elapsed-time line). A bullet or arrow glyph at the start of the row does not matter | one trimmed, LOWERCASED line (also tried without a leading glyph) |
| `[tui.menu]` `regex` | an open selector, so "wait for settled" does not call it idle | whole screen |
| `[tui.dialogs]` `strip_blocks` | removes boxed dialogs (trust/approval) from reply text | screen text |
| `[tui.footer_model]` `regex` | live model name, with a group named `model`. It may sit in a footer, a status line or the banner: Puppet Master tries the screen it sees, every look of the pane's whole life (a banner that scrolled away before this run began is remembered), and the scrollback. Prefer a row that STAYS on screen (a status line or footer) over the banner. A name cut at the window's edge (the match is the last thing on its row) never replaces a longer name that starts with it | whole screen (and the scrollback) |
| `[tui.classify]` `waiting_regex` | force a "waiting for input" status | last visible lines |
| `[[tui.prompt]]` | approval or trust prompts and how to answer them | whole screen |

Because chrome, thinking, summary and tool-activity lines are lowercased before matching, write those regexes in lowercase.

**The reply of a turn** is what follows the echo of THIS turn's task on the screen, with every chrome, thinking, summary and tool-activity row removed (rows up to the last tool-activity row are the tool's, not the answer). The banner above the echo, the previous turn and stale status rows can never be part of it. A reply taller than the pane is read from the scrollback, from the task's echo on (when the agent does not echo the task, from the first row that is new since just before it was typed). A shortened path in the reply (`.../x.py`) that the agent drew as an OSC 8 hyperlink comes back as the path of the link's `file://` target, only when the shown text is an ellipsised form of that very path. A reply that still holds anything but the answer is a table that is missing a row: the Agent Wizard's visible test fails it ("the reply is not clean") and names the extra rows.
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

A startup question (an agent asking "do you trust this folder?" before it takes a task) needs its own prompt table,
or the pane reads as idle behind it and no approval card appears. Use `kind = "workspace_trust"`, a `detect_regex`
that matches the question line (not the folder path), `allow_once` for the row that means yes (an arrow menu whose
cursor already sits on it: `key = "enter"`; a numbered menu: `text = "1"`) and `deny` for the no row:

```toml
[[tui.prompt]]
kind = "workspace_trust"
detect_regex = 'do you trust the contents of this project'
choices = [
  { id = "allow_once", key = "enter" },
  { id = "deny",       key = "escape" },
]
```

The Agent Wizard's `verify` requires such a table for every `tui_trust` capture, and its visible test checks that the
startup question becomes a card.

**Permission questions: one table per kind.** A command run and a file write are usually different screens (a numbered
menu for one, an arrow menu for the other). Write a `[[tui.prompt]]` for each, with the kind that says what it asks:
`command_approval` for running a command, `file_edit` for writing or editing a file. The wizard captures both
(`tui_permission_command` for the command, `tui_permission` for the file) and `verify` requires a matching table for each capture.

**A card's id is the QUESTION's, not the screen's.** The id handed to a caller is made from the prompt kind, the
line(s) `detect_regex` matched and (without a named identity) the choices. A spinner, a timer or a blinking cursor
redrawing under the question never changes it; another question does. An id that is not the question's types nothing.

**A menu no table describes still gets a card.** When a pane is waiting and shows an arrow-key or numbered yes / no /
allow menu (or a `[y/n]` line) that no `[[tui.prompt]]` matches, Puppet Master publishes a generic card of kind
`unknown_question`, marked "recognised by shape, not by profile": one choice per option, in the order shown
(`option_1`, `option_2`, ... with the label as the screen words it). `answer_prompt` picks one by id, number or label
(`allow_once` / `deny` work when exactly one option plainly means that); an option that says always / don't ask again /
for this session needs `allow_broad`. Sign-in, payment, model and telemetry questions get no card. Writing the table
is still better: the card then has your kinds, keys and wording.

### 6.1 Rich Chat from a transcript file (`[session_chat]`)

The Chat tab of a visible pane reads the pane's screen by default. If your CLI also writes its conversation to a
JSONL file (one JSON object per line), declare where and what the fields are, and the Chat tab shows the same rich
view as for the built-in agents: messages, tool calls with the files they touch, results, token counts, the end of
each turn. Puppet Master only reads the file; it never writes it.

```toml
[session_chat]
format = "jsonl"                                   # the only format in v1
dir = "{home}/.acme/sessions/{cwd_dashes}"         # where the CLI writes (see the placeholders below)
file_glob = "*.jsonl"                              # default "*.jsonl"; a file name with * and ?
pick = "newest_since_start"                        # default and only value: the newest file written since the pane started
write_tools = ["write_file"]                       # optional: tools that change files (else guessed)
helper_tools = ["spawn_helper"]                    # optional: tools that start a helper agent

[session_chat.map]                                 # JSON pointers (RFC 6901) into ONE line
role = "/role"                                     # required
text = "/content"                                  # required: a string, or an array of parts
# optional: tool_name, tool_input, tool_id, tool_result_for, tool_output, tool_error,
#           timestamp, model, usage_input, usage_output, turn_end

[session_chat.values]                              # all optional
user = ["user"]                                    # default ["user"]
assistant = ["assistant"]                          # default ["assistant"]
tool = ["tool"]                                    # default ["tool"]
turn_end = ["end_turn"]                            # required when map.turn_end is set
```

**`dir`** must start with `{home}` (the person's home folder), `{cwd}` (the folder the pane was started in) or
`{env:NAME}` (an environment variable), and may then use one-segment placeholders: `{cwd_slug}` (every character that
is not a letter or digit becomes `-`: `C:\Users\you\proj` is `C--Users-you-proj`), `{cwd_dashes}` (only `/`, `\` and
`:` become `-`: `/home/you/proj` is `-home-you-proj`), `{cwd_url}` (percent-encoded), `{cwd_name}` (the last folder's
name) and `{cwd_sha256}` (hex SHA-256 of the folder path). The folder that comes out must be under the person's home
folder or the pane's folder, after symbolic links are followed; anything else is never listed or read. A `..` segment
is refused when the definition loads.

**What a line is.** The value at `map.role` is compared (letter case ignored) with `values`:

| Role | The line gives |
|---|---|
| `user` | what the person typed (`map.text`), or, when `map.tool_result_for` is present, a tool result |
| `assistant` | the answer text (`map.text`, with `map.model`), and a tool call too when `map.tool_name` is present |
| `tool` | a tool result when `map.tool_result_for` is present, else a tool call when `map.tool_name` is present |
| anything else | nothing (but see `turn_end` and the token counts) |

- `text` is a string, or an array of parts: plain strings and objects `{type = "text" | "output_text" | "input_text", text = ...}` (an object with no `type` counts); other parts (images, thinking) are left out.
- A tool call needs `tool_name`; `tool_id` pairs it with its result (`tool_result_for` holds that id). `tool_input` may be an object, a string that holds a JSON object, or a plain string (taken as the command). Its first of `command`, `cmd`, `file_path`, `path`, `pattern`, `query`, `url`, `description`, `prompt` is the one-line summary. The files a call touches are the input's `file_path`, `filepath`, `file`, `filename`, `target_file`, `notebook_path`, `path` or `target`. It **writes** when it touches a file and its name is in `write_tools`; with no `write_tools`, when its name has `write`, `edit`, `patch`, `create`, `replace`, `insert` or `save` in it, or its input has `content`, `new_string`, `old_string`, `new_text`, `patch`, `diff` or `edits`.
- A tool result's text is `tool_output` (or the line's `text`). It failed when `tool_error` is `true`, a number other than 0 (an exit code), or a message. Terminal colour codes are removed and the text is cut to 4000 characters.
- `timestamp` is a string (kept as is) or a number (Unix seconds, or milliseconds when it is that large).
- `usage_input` / `usage_output` give a token count item when either is a number.
- `turn_end` is the pointer of a field whose value (one of `values.turn_end`) says the turn ended: the answer on that line is marked final and a turn-end item follows. It is read on every line, so a line that is only an end marker works.

Several pointers may be given for one field (`text = ["/content", "/message/content"]`): the first that finds a value wins.

**Limits (not settable).** A line over 1 MiB is skipped; at most 8 MiB of a file is read per poll (a bigger backlog arrives over the next polls); a file over 128 MiB is not followed; at most 5000 folder entries are looked at; a pointer is at most 128 characters and 8 segments. The text that leaves goes through the same redaction as every other chat item.

**Checked when the definition loads, never later.** `AGENT_DEF_SESSION_CHAT_BAD_DIR` (a template that is not under home or the project, has `..`, an unknown placeholder or unbalanced braces), `AGENT_DEF_SESSION_CHAT_BAD_GLOB`, `AGENT_DEF_SESSION_CHAT_BAD_POINTER` (not a pointer, too long, a bad `~` escape; the error path names the key, `session_chat.map.text` or `session_chat.map.tool_error.1`), `AGENT_DEF_INCONSISTENT` (`tool_input` without `tool_name`, `tool_output` without `tool_result_for`, `turn_end` without its values or the other way round, one value for two roles), and the usual `AGENT_DEF_UNKNOWN_FIELD` / `AGENT_DEF_INVALID_TOML` for a misspelt or missing key. The table is part of the fingerprint: changing it asks for approval again.

**Old chats (`[session_chat.history]`, optional).** `[session_chat]` reads the pane's own newest file. To let Puppet Master
bring in the CLI's older chats too (the "Bring my old chats" screen), add this table next to it. Without it the agent has
no old chats in search. Puppet Master lists and reads these folders only after the person asks it to bring old chats in.

```toml
[session_chat.history]
roots = ["{home}/.acme/sessions"]          # 1 to 4 folders; each starts with {home} or {env:NAME}, has no other placeholder, no `..`
file_glob = "*.jsonl"                      # optional; default session_chat.file_glob; must end in .jsonl or .jsonl.zst
depth = 2                                  # optional; folder levels below a root, 0 to 4; default 1
id_from = "pointer"                        # "file_stem" (default: the file name) or "pointer"
id_pointer = "/session_id"                 # required with id_from = "pointer", refused otherwise
cwd_pointer = "/cwd"                       # optional: the chat's folder, read from its first lines
resume_args = ["--resume", "{session}"]    # optional: how to start the CLI on an old chat; only {session}; 1 to 8 elements of 1 to 64 characters
```

Checked when the definition loads: `AGENT_DEF_SESSION_CHAT_BAD_HISTORY` (the error path names the key, for example
`session_chat.history.roots.0`) and `AGENT_DEF_INCONSISTENT` (`id_pointer` that does not fit `id_from`). The table is
part of the fingerprint only when it is there, so adding or changing it asks for approval again.

The Agent Wizard fills this table by itself when it can: it finds the JSONL file the CLI wrote during its test turn, infers the pointers, checks them with this reader and writes the table under the comment `# found by the wizard: check before approving` (agent-wizard SPEC 11.23). It is part of the fingerprint like any other table, so you still approve it.

Until the file has something to show (a wrong folder, a CLI that writes nothing yet) the Chat tab is read off the screen, as for an agent with no table; when the file has items it switches to them, once, and the view is told where the items come from.

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

### 8.1 Editing, consent, sharing and `[mcp_host]`

- **Live edits.** Saving a definition reloads it within about a second (an `agent_definitions_changed` event updates
  Settings > Agents). A run that is already going is not affected. If the edit changed anything the fingerprint covers
  (argv, binary, env, `mcp_host`), the file shows `approval_stale` and the next run is refused until you approve it again.
  If an active file becomes invalid, the previous version stays registered and the error is listed.
- **Consent.** Project and extra folders are off until you consent to that exact folder in Settings > Agents. Consent
  only lets the folder be read; every file still needs approval.
- **Export / import.** Export writes one definition to a `.toml` you choose (and `<name>.fixtures.zip` if the file has
  an `<id>.fixtures/` folder next to it). Import checks a `.toml`, copies it to the user folder as `<id>.toml` and leaves
  it unapproved. It never overwrites a file and nothing is downloaded.
- **`[mcp_host]`** (optional) registers Puppet Master's MCP server in another tool's config:

  ```toml
  [mcp_host]
  config_path = "~/.mytool/mcp.json"      # under your home or the project; no `..`
  format = "json"                         # json | toml | yaml
  key_path = "mcpServers.puppet-master"   # dotted; only this key is written
  [mcp_host.entry]
  command = "{command}"
  args = "{args}"                         # whole value only
  env = "{env}"                           # whole value only
  ```

  The writer needs an approved definition, copies the file to `<file>.bak` before the first change, and a second run
  changes nothing. YAML entries are written as one-line flow mappings.

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
   ready = true                     # [tui.ready] says the empty input box is showing (false: it does not)
   menu = false
   prompt_kind = "command_approval" # "" means no prompt
   model = "Nemotron 3 Ultra (free)"
   chrome_lines = ["9% used"]       # must be chrome rows
   not_chrome_lines = ["the answer"]
   thinking_lines = ["Thinking... (3s)"]     # must be thinking rows (checked lowercased, like chrome)
   not_thinking_lines = ["the answer"]
   summary_lines = ["▸ Thought for 3s, 298 tokens"]   # must be post-turn summary rows (not_summary_lines: must not)
   tool_lines = ["tool: ls"]                           # must be tool-activity rows (not_tool_lines: must not)
   reply_after = "reply with exactly"                  # the rows under the last row holding this text (the task's echo) ...
   reply_rows = ["reply: the answer"]                  # ... less chrome, thinking, summary and tool rows, are exactly these
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
| Result has `Thinking...` / spinner / `Thought for 4s` rows (visible) | no `[tui.thinking]` | add lowercase `line_regex` entries for those rows |
| Result ends with a `Thought for 3s, 298 tokens` / `Worked for 9s` row printed after the turn (visible) | no `[tui.summary]` | add a lowercase `line_regex` for it (a leading bullet does not matter) |
| `resolved_model` is null although the screen names the model (visible) | no `[tui.footer_model]`, or its regex misses the row | add it with a `(?P<model>...)` group; match a status line or footer that stays on screen as well as the banner |
| Trust or approval answered with the id just read is refused as stale (visible) | the id was made from a screen that redraws | fixed in Puppet Master: ids are the question's; a `detect_regex` that matches the question line (not a spinner row) keeps it stable |
| Each command / file approval needs the pane taken over (visible) | the definition only describes one permission screen | add a `[[tui.prompt]]` per kind (`command_approval`, `file_edit`) |
| The conversation shows twice after a window resize (visible) | `reprints_on_resize` is false although the agent reprints | let the wizard measure it, or set `[features] reprints_on_resize = true` |
| Turn never finishes (visible) | `busy` pattern too eager, or screen keeps animating | tighten `working_regex`, raise `idle_after_ms` |
| Turn finishes too early (visible) | spinner frames look idle | add `working_regex` for the spinner; keep `idle_after_ms` >= 2000 |
| Dialog answered twice / re-detected | answered dialog text stays on screen | add `not_when_regex` |
| Follow-up starts a new conversation | no `resume` segment or no `session` selector | add both |
| `READ_ONLY_UNSUPPORTED` | `read_only_headless` not set | only set it if the CLI really enforces it (note required) |
| Old behaviour after saving | reload failed and kept the previous version | see the error list; press Reload |

## 12. Complete example (headless + visible): Kilo Code

See `SPEC.md` 7.2 for the full annotated file and `AGENT-CHOICE.md` for the verified CLI flags and the exact
JSON events it prints.

## 13. Complete example (visible + transcript): Acme

A fictional CLI "acme" that writes `~/.acme/sessions/<folder>/<id>.jsonl`. Its lines look like this (the repository's test
fixture, `src/tui/declarative/fixtures/acme/session.jsonl`, has a whole session):

```jsonl
{"ts":"2026-10-28T09:00:00.000Z","kind":"message","role":"user","content":"Add a greeting to main.py"}
{"ts":"2026-10-28T09:00:05.000Z","kind":"tool_call","role":"tool","call":{"id":"c2","name":"write_file","args":{"path":"main.py","content":"print('hello')\n"}}}
{"ts":"2026-10-28T09:00:05.500Z","kind":"tool_result","role":"tool","result":{"for":"c2","output":"wrote 1 line"}}
{"ts":1793178060,"kind":"message","role":"assistant","model":"acme-1","content":"Done.","stop":"end_turn","usage":{"in":1200,"out":85}}
```

The definition (`fixtures/definitions/acme-chat.toml`; a test keeps the `[session_chat]` tables below equal to the file):

```toml
[session_chat]
format = "jsonl"
dir = "{home}/.acme/sessions/{cwd_dashes}"
file_glob = "*.jsonl"
pick = "newest_since_start"
write_tools = ["write_file", "patch_file"]
helper_tools = ["spawn_helper"]

[session_chat.map]
role = "/role"
text = "/content"
timestamp = "/ts"
model = "/model"
tool_name = "/call/name"
tool_input = "/call/args"
tool_id = "/call/id"
tool_result_for = "/result/for"
tool_output = "/result/output"
tool_error = ["/result/is_error", "/result/exit_code"]
usage_input = "/usage/in"
usage_output = "/usage/out"
turn_end = "/stop"

[session_chat.values]
user = ["user"]
assistant = ["assistant"]
tool = ["tool"]
turn_end = ["end_turn"]
```
