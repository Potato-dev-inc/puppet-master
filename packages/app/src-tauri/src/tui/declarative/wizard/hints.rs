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
        "WIZARD_ID_TAKEN" => "Choose a different `agent.id`; this one already has a file. Add a suffix such as `_2`.",
        "WIZARD_BINARY_MISMATCH" => {
            "Put the command the person typed first in `binary.names`. Do not rename the program."
        }
        "WIZARD_BINARY_UNRESOLVED" => {
            "The program could not be found with these names. Use the exact command the person typed. For an npm wrapper, add `shim_native_globs` pointing at the real .exe (see the format guide, section 8)."
        }
        "WIZARD_KIND" => "Remove `kind = \"generic\"` (or set `kind = \"cli\"`). A generic agent has no headless table.",
        "FIXTURE_FAILED" => {
            "The real or generated output was not read the way the fixture expects. Compare your `where`, `path` and `pick` with the real output lines shown, and fix the selector that is wrong."
        }
        "LIVETEST_NO_OUTPUT" => {
            "The program printed nothing on stdout. Check the arguments: the flag that runs one task and prints machine-readable output may be missing."
        }
        "LIVETEST_TIMEOUT" => {
            "The program did not finish in time. It may be waiting for input or for a permission; check the flags, or add a `[headless.output.done]` selector for the event that ends a turn."
        }
        "LIVETEST_SPAWN_FAILED" => {
            "The program could not be started with these arguments. Check `binary.names` and `shim_native_globs`."
        }
        "LIVETEST_PONG_FAILED" | "LIVETEST_RESUME_FAILED" | "LIVETEST_BAD_MODEL_FAILED" => {
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
        "CONFORMANCE_PROMPTS_ROUND_TRIP" => "Do not add `[[tui.prompt]]` entries in a headless definition.",
        "CONFORMANCE_EMPTY_SCREEN_NEVER_BLOCKED" | "CONFORMANCE_BLANK_SCREEN_IS_UNKNOWN" => {
            "Remove screen patterns (`[tui]` tables); this wizard writes headless definitions only."
        }
        "CONFORMANCE_IDLE_WINDOWS_ARE_SANE" => "Remove `idle_after_ms` and `quiet_idle_after_ms`, or set them in range (1500-5000 and 500 up to idle_after_ms).",
        "CONFORMANCE_SUBMIT_AND_STARTUP_DECLARATIONS_ARE_SANE" => "Remove the `[tui]` submit and startup settings.",
        "CONFORMANCE_SCREEN_METHODS_SURVIVE_HUGE_INPUT" => "Simplify the screen patterns or remove the `[tui]` tables.",
        _ => "Change the file so this shared rule holds; the Problem line names what is wrong.",
    }
}
