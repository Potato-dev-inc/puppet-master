//! "How to fix it" advice (SPEC 6.2). A fixed table keyed by error code, refined by the TOML path
//! where that helps, so the same mistake always gets the same advice. No text from the draft or the
//! program is ever copied into advice.

/// One or two sentences saying what to change. Empty only for codes this table does not know.
pub fn fix_for(code: &str, path: &str) -> String {
    let by_path = |suffix: &str| path == suffix || path.ends_with(&format!(".{suffix}"));
    let text: &str = match code {
        "AGENT_DEF_INVALID_TOML" => {
            "Write valid TOML: quote every string, use single-quoted literal strings for regexes with backslashes, and close every bracket."
        }
        "AGENT_DEF_UNKNOWN_FIELD" => {
            "Remove that field or fix its spelling. Only fields from the format guide are allowed, in the tables the guide shows."
        }
        "AGENT_DEF_SCHEMA_UNSUPPORTED" => "Start the file with `schema_version = 1`.",
        "AGENT_DEF_BAD_ID" | "AGENT_DEF_RESERVED_ID" => {
            "Use an id of 2 to 32 lowercase letters, digits or underscores that starts with a letter, and is not a built-in agent name (claude, codex, opencode, cursor, cmd, powershell, bash)."
        }
        "AGENT_DEF_BAD_BINARY" => {
            "Use bare program names in `binary.names` (for example `mycoder`): no folders, no extension, no `..`."
        }
        "AGENT_DEF_SHELL_INTERPRETER" => {
            "The program must be the agent itself, not a shell or interpreter (bash, python, node, cmd, powershell ...)."
        }
        "AGENT_DEF_DENIED_ENV" => "Remove that environment variable from `[env]`; it is refused for safety.",
        "AGENT_DEF_BAD_PLACEHOLDER" => {
            "Use only {task}, {project}, {model}, {session} or {reasoning}. Deliver the task exactly one way: a {task} argument, `task_flag`, or `[headless.stdin]`, and give a positional task a `task_separator = \"--\"`. {task} is not allowed inside `[[headless.optional]]`."
        }
        "AGENT_DEF_BAD_WHEN" => {
            "`when` must be one of: model, resume, read_only, not_read_only, allow_shell, trust_workspace."
        }
        "AGENT_DEF_BAD_REGEX" => {
            "Use a simpler regex: Rust regex syntax has no look-ahead, look-behind or backreferences. Put the pattern in a single-quoted TOML string."
        }
        "AGENT_DEF_REGEX_TOO_LARGE" => "Make that regex smaller or split it; a regex may compile to at most 256 KiB.",
        "AGENT_DEF_TOO_LARGE" => "Make the file smaller: drop tables you do not need. The limit is 64 KiB.",
        "AGENT_DEF_FIELD_RESERVED" => "Remove that field; it is reserved for a later version.",
        "AGENT_DEF_INCONSISTENT" => {
            if by_path("path") {
                "Add `path = \"...\"`: the dotted path of the value inside one output line, for example path = \"part.text\"."
            } else if by_path("result") {
                "Add a `[headless.output.result]` table that says where the final answer is, with `where`, `path` and `pick`."
            } else if by_path("output") {
                "Add a `[headless.output]` table with a `format` and a `[headless.output.result]` selector."
            } else if by_path("max_by") {
                "Add `max_by = \"some.numeric.path\"` or use another `pick`."
            } else {
                "Change the field so it agrees with the rest of the file; read the Problem line for what it must agree with."
            }
        }
        "WIZARD_NO_HEADLESS" => {
            "Add a `[headless]` table (argv, task_separator) and a `[headless.output]` table. This wizard only writes headless definitions."
        }
        "WIZARD_NO_TUI" => {
            "Add a [tui] table (argv, [tui.ready], [tui.busy], [tui.chrome], [[tui.prompt]] ...). The visible capture was done, so this file must describe the visible pane."
        }
        "WIZARD_TUI_NO_READY" => {
            "Add [tui.ready] with a `regex` that matches the empty input box on the idle capture (for example its placeholder text) and a `not_regex` list for screens that must not count as ready."
        }
        "WIZARD_TUI_NO_BUSY" => {
            "Add [tui.busy] with `working_regex` patterns that match the working capture (a spinner row, a stop hint) and, if the idle screen has a marker of its own, `idle_regex`."
        }
        "WIZARD_TUI_NO_PROMPT" => {
            "Add a [[tui.prompt]] whose `detect_regex` matches the permission capture, with choices allow_once and deny (each a `key` or `text` that answers it), and a `not_when_regex` for what stays on screen after it was answered."
        }
        "FIXTURE_EXPECT_INVALID" => {
            "Fix or remove that expectation block: it may only use chrome_lines, not_chrome_lines, thinking_lines, summary_lines, tool_lines, model, menu and prompt_kind, and every row must appear on the capture it belongs to. For a dialog, make a [[tui.prompt]] match it."
        }
        "WIZARD_ID_TAKEN" => "Choose a different `agent.id`; this one already has a file. Add a suffix such as `_2`.",
        "WIZARD_BINARY_MISMATCH" => {
            "Put the command the person typed first in `binary.names` (other names may follow). Do not rename the program."
        }
        "WIZARD_BINARY_UNRESOLVED" => {
            "The program could not be found with these names. Use the exact command the person typed. For an npm wrapper, add `shim_native_globs` pointing at the real .exe (see the format guide, section 8)."
        }
        "WIZARD_TEST_WORD" => {
            "Remove that word. Write the result selector so it reads any answer from the output (by event type and field path), not the text of one particular answer."
        }
        "WIZARD_KIND" => "Remove `kind = \"generic\"` (or set `kind = \"cli\"`). A generic agent has no headless table.",
        "FIXTURE_FAILED" => {
            if path.starts_with("tui_") {
                "The screen was not read the way the capture proves it should be. Compare the regex named in the Problem with the screen shown: `ready` is [tui.ready], `busy` is [tui.busy], `prompt_kind` is [[tui.prompt]], chrome rows are [tui.chrome], rows printed while it thinks or works are [tui.thinking], a row printed once after a turn is [tui.summary], tool-call rows are [tui.tool_activity] (lowercase regexes)."
            } else {
                "The real or generated output was not read the way the fixture expects. Compare your `where`, `path` and `pick` with the real output lines shown, and fix the selector that is wrong."
            }
        }
        "VISIBLETEST_SPAWN_FAILED" => "The test pane could not be started. Check `[tui] argv` and `[binary]`; the program must start in a terminal with these arguments.",
        "VISIBLETEST_STARTS_FAILED" => "The program ended right after it started. Check `[tui] argv`: an argument may make it exit, or it needs one the capture did not use.",
        "VISIBLETEST_READY_FAILED" => {
            "The profile never read the screen as the empty input box. Compare [tui.ready] `regex` and `not_regex` with the idle capture and with the screen shown, and check [tui.busy] and [[tui.prompt]] do not match the idle screen."
        }
        "VISIBLETEST_DELIVERY_FAILED" => {
            "The task could not be typed and sent. Check `[tui.delivery]` (input_echo_regex must match the typed text in the input box, transcript_echo_regex the echoed message) and `clear_input_keys`."
        }
        "VISIBLETEST_BUSY_FAILED" => "The profile did not see the agent working. Make [tui.busy] `working_regex` match the working capture.",
        "VISIBLETEST_DONE_REPLY_FAILED" => {
            "The reply was not read from the finished screen. Make [tui.chrome], [tui.thinking] and [tui.tool_activity] (lowercase line regexes) cover the banner, footer, thinking / status and tool rows but not the reply row, and check [tui.busy] does not match the finished screen. The reply must come out as exactly the one answer row."
        }
        "VISIBLETEST_WORK_FAILED" => {
            "A task that makes the agent think and use a tool did not come out as exactly the one answer row. Rows it prints while it thinks or works (`Thinking...`, a spinner status, `Thought for 4s`) belong in [tui.thinking], a row printed once after the turn (`Thought for 3s, 298 tokens`, `Worked for 9s`) in [tui.summary], tool-call rows in [tui.tool_activity] (lowercase line regexes; none may match the answer row)."
        }
        "VISIBLETEST_LONG_REPLY_FAILED" => {
            "A reply taller than the pane was not read whole, or the model was not read once the banner had scrolled away. Make [tui.footer_model] match a row that stays on screen (a status line or footer, not only the startup banner), and allow for a name the window clips at its right end."
        }
        "VISIBLETEST_MODEL_FAILED" => {
            "The capture names the model but the profile read none. Add `[tui.footer_model] regex` with a named group `model`; it is matched against the whole screen (and the scrollback), so it may match a footer, a status line or the banner."
        }
        "VISIBLETEST_RESIZE_FAILED" => {
            "The pane was resized and the conversation showed again (or the viewer's grid differed from the PTY's). Do not guess `[features] reprints_on_resize`: the wizard measures it in the capture. Say so to the person if it is still wrong."
        }
        "VISIBLETEST_PERMISSION_FAILED" => {
            "The permission question did not become an approval card. Make a [[tui.prompt]] `detect_regex` match the permission capture, with choices allow_once and deny that each have a key or text."
        }
        "VISIBLETEST_PERMISSION_COMMAND_FAILED" => {
            "The command question did not become an approval card. A command run and a file write are usually different screens: write a [[tui.prompt]] (kind = \"command_approval\") whose `detect_regex` matches the tui_permission_command capture, with choices allow_once and deny that each have a key or text."
        }
        "VISIBLETEST_EXIT_FAILED" => "The pane did not close after the test.",
        "LIVETEST_NO_OUTPUT" => {
            "The program printed nothing on stdout. Check the arguments: the flag that runs one task and prints machine-readable output may be missing."
        }
        "LIVETEST_TIMEOUT" => {
            "The program did not finish in time. It may be waiting for input or for a permission; check the flags, or add a `[headless.output.done]` selector for the event that ends a turn."
        }
        "LIVETEST_SPAWN_FAILED" => {
            "The program could not be started with these arguments. Check `binary.names` and `shim_native_globs`."
        }
        "LIVETEST_PONG_FAILED" | "LIVETEST_SENTENCE_FAILED" | "LIVETEST_RESUME_FAILED" | "LIVETEST_BAD_MODEL_FAILED" => {
            "Read the real output below and change the selectors (or the arguments) so the result, session id and errors are found in it."
        }
        c if c.starts_with("CONFORMANCE_") => conformance_hint(c),
        _ => "",
    };
    text.to_string()
}

fn conformance_hint(code: &str) -> &'static str {
    match code {
        "CONFORMANCE_FEATURE_FLAGS_AGREE_WITH_METHODS" => {
            "Only set `read_only_headless` when the program has a documented read-only mode that you map with `when = \"read_only\"`; otherwise leave it false."
        }
        "CONFORMANCE_EXTRACT_RESULT_REJECTS_GARBAGE_AND_FAILED_EXITS" => {
            "Make the result selector stricter: garbage text and failed runs must give no result. Use a JSON format with a `where` that matches only the real answer event, or anchor `result.regex`."
        }
        "CONFORMANCE_PROMPTS_ROUND_TRIP" => {
            "Every [[tui.prompt]] must answer what it detects: give it choices with allow_once and deny, each with a `key` or `text`, and make `detect_regex` specific (it must not match a plain sentence like \"Allow once?\")."
        }
        "CONFORMANCE_EMPTY_SCREEN_NEVER_BLOCKED" | "CONFORMANCE_BLANK_SCREEN_IS_UNKNOWN" => {
            "A blank screen must never read as a prompt, a dialog or an input box: make [tui.ready], [[tui.prompt]] and [tui.classify] regexes need real text (no pattern that matches nothing at all)."
        }
        "CONFORMANCE_IDLE_WINDOWS_ARE_SANE" => "Remove `idle_after_ms` and `quiet_idle_after_ms`, or set them in range (1500-5000 and 500 up to idle_after_ms).",
        "CONFORMANCE_SUBMIT_AND_STARTUP_DECLARATIONS_ARE_SANE" => "Remove the `[tui]` submit and startup settings that are out of range.",
        "CONFORMANCE_SCREEN_METHODS_SURVIVE_HUGE_INPUT" => "Simplify the screen patterns: one of them is too slow on a very large screen.",
        _ => "Change the file so this shared rule holds; the Problem line names what is wrong.",
    }
}
