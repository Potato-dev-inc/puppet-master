//! The pure validator (SPEC 5.4, 7.4). No I/O, no processes, no network (VAL-8): it takes a parsed
//! `Definition` plus the facts it cannot know itself (source, code-profile ids, raw file size)
//! and returns every problem it finds, so the definitions list can show all of them at once.

use regex::RegexBuilder;

use super::schema::{DefError, Definition, Kind, ModelSource, OptionalSegment, Pick, Selector};

pub const MAX_DEFINITION_BYTES: usize = 64 * 1024;
pub const MAX_REGEXES: usize = 200;
pub const MAX_ARGV_ELEMENTS: usize = 64;
pub const MAX_STRING_BYTES: usize = 4 * 1024;
pub const REGEX_SIZE_LIMIT: usize = 256 * 1024;

const RESERVED_IDS: &[&str] = &["unknown", "all", "any", "auto", "default", "none", "shell"];
const INTERPRETERS: &[&str] = &[
    "sh", "bash", "zsh", "fish", "dash", "cmd", "powershell", "pwsh", "python", "python3", "node",
    "deno", "bun", "ruby", "perl", "php", "osascript", "wscript", "cscript", "mshta", "env",
];
const DENIED_ENV: &[&str] = &[
    "PATH", "PATHEXT", "LD_PRELOAD", "LD_LIBRARY_PATH", "NODE_OPTIONS", "PYTHONPATH", "PYTHONHOME",
    "RUBYOPT", "PERL5OPT", "COMSPEC", "SHELL",
];
const HEADLESS_WHEN: &[&str] =
    &["model", "resume", "read_only", "not_read_only", "allow_shell", "trust_workspace"];
const TUI_WHEN: &[&str] = &["model", "read_only", "resume"];
const HEADLESS_PLACEHOLDERS: &[&str] = &["task", "project", "model", "session", "reasoning"];
const TUI_PLACEHOLDERS: &[&str] = &["project", "model", "session"];
const WINDOWS_EXTENSIONS: &[&str] = &["exe", "cmd", "bat", "ps1", "com"];
const CAPABILITIES: &[&str] = &[
    "codebase-reasoning", "implementation", "review", "debugging", "terminal-ops", "ui-orchestration",
];

/// Where a definition came from; project and extra sources are never trusted with interpreters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Bundled,
    User,
    Project,
    Extra,
}

pub struct ValidationContext<'a> {
    pub source: Source,
    /// Ids of the compiled code profiles (reserved, VAL-1).
    pub code_ids: &'a [&'a str],
    /// Byte length of the TOML text the definition was parsed from (FMT-6).
    pub raw_len: usize,
}

struct Errors(Vec<DefError>);

impl Errors {
    fn add(&mut self, code: &'static str, path: &str, message: impl Into<String>) {
        self.0.push(DefError::new(code, path, message));
    }
}

/// Every problem found; an empty list means the definition may be registered.
pub fn validate(def: &Definition, ctx: &ValidationContext<'_>) -> Vec<DefError> {
    let mut e = Errors(Vec::new());
    let mut regexes: Regexes = Vec::new();

    if ctx.raw_len > MAX_DEFINITION_BYTES {
        e.add("AGENT_DEF_TOO_LARGE", "", format!("definition is {} bytes, limit {MAX_DEFINITION_BYTES}", ctx.raw_len));
    }
    check_agent(def, ctx, &mut e);
    check_binary(def, &mut e);
    check_env(def, &mut e);
    check_headless(def, &mut e, &mut regexes);
    check_tui(def, &mut e, &mut regexes);
    check_model_and_context(def, &mut e, &mut regexes);
    check_features(def, &mut e);
    check_run_options(def, &mut e);
    check_mcp_host(def, &mut e);
    check_regexes(&regexes, &mut e);
    e.0
}

fn check_agent(def: &Definition, ctx: &ValidationContext<'_>, e: &mut Errors) {
    let agent = &def.agent;
    if !valid_id(&agent.id) {
        e.add("AGENT_DEF_BAD_ID", "agent.id", "id must match ^[a-z][a-z0-9_]{1,31}$");
    } else if RESERVED_IDS.contains(&agent.id.as_str()) || ctx.code_ids.contains(&agent.id.as_str()) {
        e.add("AGENT_DEF_RESERVED_ID", "agent.id", format!("`{}` is reserved", agent.id));
    }
    let label_len = agent.label.chars().count();
    if label_len == 0 || label_len > 40 || agent.label.chars().any(char::is_control) {
        e.add("AGENT_DEF_INCONSISTENT", "agent.label", "label must be 1-40 characters without control characters");
    }
    if let Some(icon) = &agent.icon {
        if !(1..=4).contains(&icon.chars().count()) {
            e.add("AGENT_DEF_INCONSISTENT", "agent.icon", "icon must be 1-4 characters");
        }
    }
    if agent.description.chars().count() > 240 {
        e.add("AGENT_DEF_TOO_LARGE", "agent.description", "description is limited to 240 characters");
    }
    if agent.base.is_some() {
        e.add("AGENT_DEF_FIELD_RESERVED", "agent.base", "`base` is reserved in schema version 1");
    }
    if matches!(agent.kind, Kind::NativeProtocol | Kind::Shell) {
        e.add("AGENT_DEF_FIELD_RESERVED", "agent.kind", "kind must be `cli` or `generic` in schema version 1");
    }
    if agent.allow_interpreter && matches!(ctx.source, Source::Project | Source::Extra) {
        e.add("AGENT_DEF_SHELL_INTERPRETER", "agent.allow_interpreter", "allow_interpreter is rejected for project and extra sources");
    }
    if matches!(agent.kind, Kind::Generic) && def.headless.is_some() {
        e.add("AGENT_DEF_INCONSISTENT", "headless", "a generic agent is visible-pane only: remove [headless]");
    }
    if def.headless.is_none() && def.tui.is_none() && !matches!(agent.kind, Kind::Generic) {
        e.add("AGENT_DEF_INCONSISTENT", "", "a definition needs a [headless] or a [tui] table");
    }
}

fn valid_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    (2..=32).contains(&bytes.len())
        && bytes[0].is_ascii_lowercase()
        && bytes[1..].iter().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_')
}

fn bare_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains(['/', '\\', ':', '\0'])
        && name != "."
        && !name.contains("..")
        && !name.chars().any(char::is_control)
}

fn check_binary(def: &Definition, e: &mut Errors) {
    let binary = &def.binary;
    if binary.names.is_empty() || binary.names.len() > 6 {
        e.add("AGENT_DEF_BAD_BINARY", "binary.names", "names must hold 1-6 entries");
    }
    for (i, name) in binary.names.iter().enumerate() {
        let path = format!("binary.names.{i}");
        if !bare_name(name) {
            e.add("AGENT_DEF_BAD_BINARY", &path, "binary names must be bare file names (no path separators, `..` or drive letters)");
            continue;
        }
        let stem = name.to_ascii_lowercase();
        let stem = stem.rsplit_once('.').map_or(stem.as_str(), |(s, ext)| {
            if WINDOWS_EXTENSIONS.contains(&ext) { s } else { stem.as_str() }
        });
        if INTERPRETERS.contains(&stem) && !def.agent.allow_interpreter {
            e.add("AGENT_DEF_SHELL_INTERPRETER", &path, format!("`{name}` is a shell or interpreter; needs agent.allow_interpreter"));
        }
    }
    for (i, ext) in binary.windows_extensions.iter().enumerate() {
        if !WINDOWS_EXTENSIONS.contains(&ext.as_str()) {
            e.add("AGENT_DEF_BAD_BINARY", &format!("binary.windows_extensions.{i}"), "extension must be one of exe, cmd, bat, ps1, com");
        }
    }
    if binary.shim_native_globs.len() > 4 {
        e.add("AGENT_DEF_BAD_BINARY", "binary.shim_native_globs", "at most 4 globs");
    }
    for (i, glob) in binary.shim_native_globs.iter().enumerate() {
        let absolute = glob.starts_with(['/', '\\']) || glob.contains(':');
        if glob.is_empty() || absolute || glob.split(['/', '\\']).any(|part| part == "..") {
            e.add("AGENT_DEF_BAD_BINARY", &format!("binary.shim_native_globs.{i}"), "globs must be relative and stay inside the shim directory");
        }
    }
    if binary.install_hint.chars().count() > 200 {
        e.add("AGENT_DEF_TOO_LARGE", "binary.install_hint", "install_hint is limited to 200 characters");
    }
    if binary.login_hint.chars().count() > 200 {
        e.add("AGENT_DEF_TOO_LARGE", "binary.login_hint", "login_hint is limited to 200 characters");
    }
    check_argv("binary.version_args", &binary.version_args, &[], e);
    if binary.allow_cmd_shim && def.headless.as_ref().is_none_or(|h| h.stdin.is_none()) {
        e.add("AGENT_DEF_INCONSISTENT", "binary.allow_cmd_shim", "a .cmd shim is only allowed when the task is delivered on stdin ([headless.stdin])");
    }
}

fn check_env(def: &Definition, e: &mut Errors) {
    for (key, value) in &def.env.set {
        check_env_key(key, &format!("env.set.{key}"), e);
        if value.len() > 1024 {
            e.add("AGENT_DEF_TOO_LARGE", &format!("env.set.{key}"), "env values are limited to 1 KiB");
        }
        if !placeholders(value).is_empty() {
            e.add("AGENT_DEF_BAD_PLACEHOLDER", &format!("env.set.{key}"), "env values take no placeholders");
        }
    }
    for (i, key) in def.env.unset.iter().enumerate() {
        check_env_key(key, &format!("env.unset.{i}"), e);
    }
}

fn check_env_key(key: &str, path: &str, e: &mut Errors) {
    let upper = key.to_ascii_uppercase();
    if DENIED_ENV.contains(&upper.as_str()) || upper.starts_with("DYLD_") {
        e.add("AGENT_DEF_DENIED_ENV", path, format!("environment key `{key}` is not allowed"));
    }
}

/// `{name}` tokens in `s`; other braces are literal text.
pub fn placeholders(s: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut rest = s;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) => {
                let name = &after[..close];
                if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphabetic() || c == '_') {
                    found.push(name);
                }
                rest = &after[close + 1..];
            }
            None => break,
        }
    }
    found
}

fn check_argv(path: &str, argv: &[String], allowed: &[&str], e: &mut Errors) {
    if argv.len() > MAX_ARGV_ELEMENTS {
        e.add("AGENT_DEF_TOO_LARGE", path, format!("at most {MAX_ARGV_ELEMENTS} argv elements"));
    }
    for (i, arg) in argv.iter().enumerate() {
        let here = format!("{path}.{i}");
        check_string(&here, arg, e);
        for name in placeholders(arg) {
            if !allowed.contains(&name) {
                e.add("AGENT_DEF_BAD_PLACEHOLDER", &here, format!("placeholder `{{{name}}}` is not allowed here"));
            }
        }
    }
}

fn check_string(path: &str, value: &str, e: &mut Errors) {
    if value.len() > MAX_STRING_BYTES {
        e.add("AGENT_DEF_TOO_LARGE", path, format!("strings are limited to {MAX_STRING_BYTES} bytes"));
    }
}

fn check_optional(path: &str, segments: &[OptionalSegment], when: &[&str], allowed: &[&str], e: &mut Errors) {
    for (i, segment) in segments.iter().enumerate() {
        let here = format!("{path}.{i}");
        if !when.contains(&segment.when.as_str()) {
            e.add("AGENT_DEF_BAD_WHEN", &format!("{here}.when"), format!("`{}` is not in the allowed set: {}", segment.when, when.join(", ")));
        }
        check_argv(&format!("{here}.args"), &segment.args, allowed, e);
    }
}

fn has_when(segments: &[OptionalSegment], when: &str) -> bool {
    segments.iter().any(|s| s.when == when)
}

type Regexes = Vec<(String, String)>;

fn add_regex(regexes: &mut Regexes, path: String, pattern: &str) {
    regexes.push((path, pattern.to_string()));
}

fn check_headless(def: &Definition, e: &mut Errors, regexes: &mut Regexes) {
    let Some(headless) = &def.headless else { return };
    check_argv("headless.argv", &headless.argv, HEADLESS_PLACEHOLDERS, e);
    check_optional("headless.optional", &headless.optional, HEADLESS_WHEN, HEADLESS_PLACEHOLDERS, e);
    for segment in &headless.optional {
        if segment.args.iter().any(|a| placeholders(a).contains(&"task")) {
            e.add("AGENT_DEF_BAD_PLACEHOLDER", "headless.optional", "{task} is not allowed in optional segments");
        }
    }
    if let Some(flag) = &headless.task_flag {
        check_string("headless.task_flag", flag, e);
    }
    if let Some(sep) = &headless.task_separator {
        check_string("headless.task_separator", sep, e);
    }
    let task_in_argv: usize = headless
        .argv
        .iter()
        .map(|a| placeholders(a).iter().filter(|p| **p == "task").count())
        .sum();
    let deliveries = task_in_argv
        + usize::from(headless.task_flag.is_some())
        + usize::from(headless.stdin.is_some());
    if deliveries > 1 {
        e.add("AGENT_DEF_BAD_PLACEHOLDER", "headless", "exactly one task delivery is allowed: one {task} in argv, task_flag, or [headless.stdin]");
    }
    // HDL-7: a positional task must be separated from flags (or delivered by flag/stdin).
    if deliveries == 0 && headless.task_separator.is_none() {
        e.add("AGENT_DEF_BAD_PLACEHOLDER", "headless", "a positional task needs `task_separator` (or use task_flag, a {task} placeholder, or [headless.stdin])");
    }
    if let Some(stdin) = &headless.stdin {
        check_string("headless.stdin.template", &stdin.template, e);
        let found = placeholders(&stdin.template);
        if found.iter().any(|p| *p != "task") || found.iter().filter(|p| **p == "task").count() != 1 {
            e.add("AGENT_DEF_BAD_PLACEHOLDER", "headless.stdin.template", "the stdin template must contain exactly one {task} and no other placeholder");
        }
    }
    if !(65_536..=67_108_864).contains(&headless.max_output_bytes) {
        e.add("AGENT_DEF_INCONSISTENT", "headless.max_output_bytes", "max_output_bytes must be within 65536..67108864");
    }
    let Some(output) = &headless.output else {
        e.add("AGENT_DEF_INCONSISTENT", "headless.output", "[headless] needs a [headless.output] table");
        return;
    };
    let plain = output.format == super::schema::OutputFormat::PlainText;
    match &output.result {
        Some(sel) => check_selector("headless.output.result", sel, true, e, regexes),
        None if !plain => e.add("AGENT_DEF_INCONSISTENT", "headless.output.result", "a result selector is required for json formats"),
        None => {}
    }
    for (name, sel, needs_path) in [
        ("session", &output.session, true),
        ("model", &output.model, true),
        ("done", &output.done, false),
        ("error", &output.error, false),
        ("rejected", &output.rejected, false),
    ] {
        if let Some(sel) = sel {
            let path = format!("headless.output.{name}");
            check_selector(&path, sel, needs_path, e, regexes);
            if name == "rejected" && sel.warning_code.as_deref().is_none_or(str::is_empty) {
                e.add("AGENT_DEF_INCONSISTENT", &format!("{path}.warning_code"), "rejected needs a warning_code");
            }
            if name != "rejected" && sel.warning_code.is_some() {
                e.add("AGENT_DEF_INCONSISTENT", &format!("{path}.warning_code"), "warning_code is only valid under rejected");
            }
        }
    }
}

fn check_selector(path: &str, sel: &Selector, needs_path: bool, e: &mut Errors, regexes: &mut Regexes) {
    if needs_path && sel.path.is_none() {
        e.add("AGENT_DEF_INCONSISTENT", &format!("{path}.path"), "this selector needs a `path`");
    }
    if sel.pick == Pick::MaxBy && sel.max_by.is_none() {
        e.add("AGENT_DEF_INCONSISTENT", &format!("{path}.max_by"), "pick = \"max_by\" needs `max_by`");
    }
    for (key, pattern) in &sel.where_regex {
        add_regex(regexes, format!("{path}.where_regex.{key}"), pattern);
    }
    if let Some(pattern) = &sel.regex {
        add_regex(regexes, format!("{path}.regex"), pattern);
    }
    for (key, value) in &sel.where_eq {
        check_string(&format!("{path}.where.{key}"), key, e);
        if let super::schema::Scalar::Str(s) = value {
            check_string(&format!("{path}.where.{key}"), s, e);
        }
    }
    check_string(&format!("{path}.join_with"), &sel.join_with, e);
}

fn check_tui(def: &Definition, e: &mut Errors, regexes: &mut Regexes) {
    let Some(tui) = &def.tui else { return };
    check_argv("tui.argv", &tui.argv, TUI_PLACEHOLDERS, e);
    check_optional("tui.optional", &tui.optional, TUI_WHEN, TUI_PLACEHOLDERS, e);
    if let Some(argv) = &tui.read_only_argv {
        check_argv("tui.read_only_argv", argv, TUI_PLACEHOLDERS, e);
    }
    let mut many = |path: &str, patterns: &[String]| {
        for (i, p) in patterns.iter().enumerate() {
            add_regex(regexes, format!("{path}.{i}"), p);
        }
    };
    if let Some(ready) = &tui.ready {
        if let Some(r) = &ready.regex {
            many("tui.ready.regex", std::slice::from_ref(r));
        }
        many("tui.ready.not_regex", &ready.not_regex);
    }
    if let Some(busy) = &tui.busy {
        many("tui.busy.working_regex", &busy.working_regex);
        many("tui.busy.idle_regex", &busy.idle_regex);
    }
    if let Some(t) = &tui.chrome {
        many("tui.chrome.line_regex", &t.line_regex);
    }
    if let Some(t) = &tui.tool_activity {
        many("tui.tool_activity.line_regex", &t.line_regex);
    }
    if let Some(t) = &tui.menu {
        many("tui.menu.regex", &t.regex);
    }
    if let Some(t) = &tui.classify {
        many("tui.classify.waiting_regex", &t.waiting_regex);
    }
    if let Some(dialogs) = &tui.dialogs {
        for (i, block) in dialogs.strip_blocks.iter().enumerate() {
            many(&format!("tui.dialogs.strip_blocks.{i}.start_regex"), std::slice::from_ref(&block.start_regex));
            many(&format!("tui.dialogs.strip_blocks.{i}.end_regex"), std::slice::from_ref(&block.end_regex));
        }
    }
    if let Some(footer) = &tui.footer_model {
        many("tui.footer_model.regex", std::slice::from_ref(&footer.regex));
        if !footer.regex.contains("(?P<model>") && !footer.regex.contains("(?<model>") {
            e.add("AGENT_DEF_INCONSISTENT", "tui.footer_model.regex", "the regex needs a group named `model`");
        }
    }
    if let Some(layout) = &tui.layout {
        match (layout.sidebar_min_cols, layout.sidebar_width) {
            (Some(min), Some(width)) if width >= 1 && width < min && min <= 1000 => {}
            _ => e.add(
                "AGENT_DEF_INCONSISTENT",
                "tui.layout",
                "sidebar_min_cols and sidebar_width are both required, with 1 <= sidebar_width < sidebar_min_cols <= 1000",
            ),
        }
    }
    if let Some(delivery) = &tui.delivery {
        match (&delivery.input_echo_regex, &delivery.transcript_echo_regex) {
            (Some(a), Some(b)) => {
                many("tui.delivery.input_echo_regex", std::slice::from_ref(a));
                many("tui.delivery.transcript_echo_regex", std::slice::from_ref(b));
            }
            _ => e.add("AGENT_DEF_INCONSISTENT", "tui.delivery", "input_echo_regex and transcript_echo_regex are both required"),
        }
    }
    for (i, prompt) in tui.prompt.iter().enumerate() {
        let path = format!("tui.prompt.{i}");
        many(&format!("{path}.detect_regex"), std::slice::from_ref(&prompt.detect_regex));
        if let Some(r) = &prompt.not_when_regex {
            many(&format!("{path}.not_when_regex"), std::slice::from_ref(r));
        }
        if !snake(&prompt.kind) {
            e.add("AGENT_DEF_INCONSISTENT", &format!("{path}.kind"), "kind must match [a-z_]+");
        }
        if prompt.choices.is_empty() {
            e.add("AGENT_DEF_INCONSISTENT", &format!("{path}.choices"), "at least one choice is required");
        }
        for (j, choice) in prompt.choices.iter().enumerate() {
            let here = format!("{path}.choices.{j}");
            if !snake(&choice.id) {
                e.add("AGENT_DEF_INCONSISTENT", &format!("{here}.id"), "choice id must match [a-z_]+");
            }
            if choice.key.is_some() == choice.text.is_some() {
                e.add("AGENT_DEF_INCONSISTENT", &here, "a choice needs exactly one of `key` or `text`");
            }
            if choice.id == "allow_all" && !prompt.broad {
                e.add("AGENT_DEF_INCONSISTENT", &here, "an allow_all choice needs `broad = true` (SEC-7)");
            }
            if let Some(text) = &choice.text {
                check_string(&format!("{here}.text"), text, e);
            }
            if let Some(key) = &choice.key {
                if let Err(message) = crate::pty::keys::sequence(key) {
                    e.add("AGENT_DEF_INCONSISTENT", &format!("{here}.key"), message);
                }
            }
        }
    }
}

fn snake(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
}

fn check_model_and_context(def: &Definition, e: &mut Errors, regexes: &mut Regexes) {
    let model = &def.model;
    if let Some(r) = &model.probe_regex {
        add_regex(regexes, "model.probe_regex".to_string(), r);
    }
    if let Some(argv) = &model.probe_argv {
        check_argv("model.probe_argv", argv, &[], e);
    }
    match model.source {
        ModelSource::None => {}
        ModelSource::FooterRegex => {
            if def.tui.as_ref().is_none_or(|t| t.footer_model.is_none()) {
                e.add("AGENT_DEF_INCONSISTENT", "model.source", "footer_regex needs [tui.footer_model]");
            }
        }
        ModelSource::OutputPath => {
            if def.headless.as_ref().and_then(|h| h.output.as_ref()).is_none_or(|o| o.model.is_none()) {
                e.add("AGENT_DEF_INCONSISTENT", "model.source", "output_path needs [headless.output.model]");
            }
        }
        ModelSource::ProbeCommand => {
            if model.probe_argv.is_none() || model.probe_regex.is_none() {
                e.add("AGENT_DEF_INCONSISTENT", "model.source", "probe_command needs probe_argv and probe_regex");
            }
        }
    }
    let ctx = &def.context;
    if !(1..=10).contains(&ctx.smartness) {
        e.add("AGENT_DEF_INCONSISTENT", "context.smartness", "smartness must be 1-10");
    }
    for (i, s) in ctx.strengths.iter().enumerate() {
        if !CAPABILITIES.contains(&s.as_str()) {
            e.add("AGENT_DEF_INCONSISTENT", &format!("context.strengths.{i}"), format!("unknown capability `{s}`"));
        }
    }
    for (name, list) in [("notes", &ctx.notes), ("best_for", &ctx.best_for)] {
        if list.len() > 6 {
            e.add("AGENT_DEF_TOO_LARGE", &format!("context.{name}"), "at most 6 entries");
        }
        for (i, item) in list.iter().enumerate() {
            if item.chars().count() > 200 {
                e.add("AGENT_DEF_TOO_LARGE", &format!("context.{name}.{i}"), "entries are limited to 200 characters");
            }
        }
    }
}

fn check_features(def: &Definition, e: &mut Errors) {
    let f = &def.features;
    let headless_optional: &[OptionalSegment] = def.headless.as_ref().map_or(&[], |h| &h.optional);
    if !f.dispatchable && f.not_dispatchable_reason.trim().is_empty() {
        e.add("AGENT_DEF_INCONSISTENT", "features.not_dispatchable_reason", "required when dispatchable = false");
    }
    if f.read_only_headless && (def.headless.is_none() || !has_when(headless_optional, "read_only")) {
        e.add("AGENT_DEF_INCONSISTENT", "features.read_only_headless", "needs a [[headless.optional]] segment with when = \"read_only\" (SEC-8)");
    }
    if f.read_only_spawned_pane && def.tui.as_ref().is_none_or(|t| t.read_only_argv.is_none()) {
        e.add("AGENT_DEF_INCONSISTENT", "features.read_only_spawned_pane", "needs tui.read_only_argv");
    }
    if (f.read_only_headless || f.read_only_spawned_pane) && f.read_only_note.trim().is_empty() {
        e.add("AGENT_DEF_INCONSISTENT", "features.read_only_note", "required when a read-only flag is true (SEC-8)");
    }
    let derived_resumable = def.headless.as_ref().is_some_and(|h| {
        has_when(&h.optional, "resume") && h.output.as_ref().is_some_and(|o| o.session.is_some())
    });
    if let Some(explicit) = f.resumable_headless {
        if explicit != derived_resumable {
            e.add(
                "AGENT_DEF_INCONSISTENT",
                "features.resumable_headless",
                "must agree with the definition: resumable needs a when = \"resume\" segment and a [headless.output.session] selector",
            );
        }
    }
}

fn check_mcp_host(def: &Definition, e: &mut Errors) {
    let Some(m) = &def.mcp_host else { return };
    if m.config_path.trim().is_empty() || m.config_path.contains("..") {
        e.add("AGENT_DEF_INCONSISTENT", "mcp_host.config_path", "must be a non-empty path under home or the project, without `..` (HST-3)");
    }
    if crate::mcp_install::declarative::key_segments(&m.key_path).is_err() {
        e.add("AGENT_DEF_INCONSISTENT", "mcp_host.key_path", "must be dotted segments of [A-Za-z0-9_-], for example mcpServers.puppet-master");
    }
    if m.entry.is_empty() {
        e.add("AGENT_DEF_INCONSISTENT", "mcp_host.entry", "must not be empty");
    }
    let mut strings = Vec::new();
    collect_strings(&toml::Value::Table(m.entry.clone()), &mut strings);
    for s in strings {
        for name in placeholder_names(&s) {
            if !crate::mcp_install::declarative::PLACEHOLDERS.contains(&name.as_str()) {
                e.add("AGENT_DEF_BAD_PLACEHOLDER", "mcp_host.entry", format!("unknown placeholder {{{name}}}; allowed: command, args, env"));
            }
        }
        let whole = s == "{args}" || s == "{env}";
        if !whole && (s.contains("{args}") || s.contains("{env}")) {
            e.add("AGENT_DEF_BAD_PLACEHOLDER", "mcp_host.entry", "{args} and {env} must be a whole value, not part of a longer string");
        }
    }
}

fn collect_strings(v: &toml::Value, out: &mut Vec<String>) {
    match v {
        toml::Value::String(s) => out.push(s.clone()),
        toml::Value::Array(a) => a.iter().for_each(|x| collect_strings(x, out)),
        toml::Value::Table(t) => t.values().for_each(|x| collect_strings(x, out)),
        _ => {}
    }
}

fn placeholder_names(s: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = s;
    while let Some(i) = rest.find('{') {
        let after = &rest[i + 1..];
        match after.find('}') {
            Some(j) => {
                names.push(after[..j].to_string());
                rest = &after[j + 1..];
            }
            None => break,
        }
    }
    names
}

fn check_run_options(def: &Definition, e: &mut Errors) {
    let r = &def.run_options;
    let headless_optional: &[OptionalSegment] = def.headless.as_ref().map_or(&[], |h| &h.optional);
    if r.trust_workspace && !has_when(headless_optional, "trust_workspace") {
        e.add("AGENT_DEF_INCONSISTENT", "run_options.trust_workspace", "needs a segment with when = \"trust_workspace\"");
    }
    if r.allow_shell && !r.shell_default_on && !has_when(headless_optional, "allow_shell") {
        e.add("AGENT_DEF_INCONSISTENT", "run_options.allow_shell", "needs a segment with when = \"allow_shell\" or shell_default_on");
    }
    if r.allowed_commands
        && def.tui.as_ref().is_none_or(|t| !t.prompt.iter().any(|p| p.kind == "command_approval"))
    {
        e.add("AGENT_DEF_INCONSISTENT", "run_options.allowed_commands", "needs a [[tui.prompt]] with kind = \"command_approval\"");
    }
}

fn check_regexes(regexes: &Regexes, e: &mut Errors) {
    if regexes.len() > MAX_REGEXES {
        e.add("AGENT_DEF_REGEX_TOO_LARGE", "", format!("{} regexes, limit {MAX_REGEXES}", regexes.len()));
    }
    for (path, pattern) in regexes {
        check_string(path, pattern, e);
        match RegexBuilder::new(pattern).size_limit(REGEX_SIZE_LIMIT).build() {
            Ok(_) => {}
            Err(regex::Error::CompiledTooBig(limit)) => e.add(
                "AGENT_DEF_REGEX_TOO_LARGE",
                path,
                format!("compiled regex exceeds the {limit} byte size limit"),
            ),
            Err(err) => e.add("AGENT_DEF_BAD_REGEX", path, err.to_string()),
        }
    }
}
