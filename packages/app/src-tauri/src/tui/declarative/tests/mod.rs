mod conformance;
mod loader;
mod output;
mod profile;
mod resolve;
mod schema;
mod screen;
mod select;
mod template;
mod validate;

/// SPEC 7.2: the Kilo Code proof agent.
pub(super) const KILO: &str = r##"
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
"##;

/// SPEC 7.2b: minimal TUI-only agent.
pub(super) const MINIMAL: &str = r##"
schema_version = 1

[agent]
id = "mycoder"
label = "My Coder"
experimental = true

[binary]
names = ["mycoder"]
install_hint = "see https://example.invalid/mycoder"

[tui]
argv = []

[tui.busy]
working_regex = ["thinking\\.\\.\\."]

[[tui.prompt]]
kind = "command_approval"
detect_regex = "allow this command\\?"
choices = [
  { id = "allow_once", key = "y" },
  { id = "deny", key = "n" },
]
"##;
