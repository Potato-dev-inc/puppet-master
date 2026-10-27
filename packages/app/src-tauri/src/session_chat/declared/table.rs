//! The `[session_chat]` table of an agent definition: where a CLI writes its JSONL transcript and which JSON
//! pointers hold what the chat view needs. Everything here is data; `format` reads lines with it and `source`
//! finds the file. `validate_session_chat` is the load-time check, so a bad mapping fails when the definition
//! loads, never while a pane runs.

use serde::{Deserialize, Serialize};

use crate::tui::declarative::schema::DefError;

/// Longest pointer, in characters, and most segments in one.
pub const MAX_POINTER_CHARS: usize = 128;
pub const MAX_POINTER_SEGMENTS: usize = 8;
/// Most alternative pointers for one field.
pub const MAX_POINTERS_PER_FIELD: usize = 6;
/// Longest `dir` template and `file_glob`, in characters.
pub const MAX_DIR_CHARS: usize = 300;
pub const MAX_GLOB_CHARS: usize = 64;
const MAX_VALUES: usize = 16;
const MAX_VALUE_CHARS: usize = 64;
const MAX_TOOL_NAMES: usize = 32;

/// The placeholders of `dir` that may only open the template (each names an absolute folder).
pub const ROOT_PLACEHOLDERS: &[&str] = &["home", "cwd"];
/// The placeholders of `dir` that are one path segment, allowed anywhere after the root.
pub const SEGMENT_PLACEHOLDERS: &[&str] = &["cwd_slug", "cwd_dashes", "cwd_url", "cwd_name", "cwd_sha256"];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatFileFormat {
    /// One JSON object per line.
    #[default]
    Jsonl,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatPick {
    /// The newest file of the folder that was written since the pane started.
    #[default]
    NewestSinceStart,
}

/// One pointer, or several tried in order (the first that finds a value wins), for a CLI whose lines differ.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(from = "PointersRepr", into = "Vec<String>")]
pub struct Pointers(pub Vec<String>);

#[derive(Deserialize)]
#[serde(untagged)]
enum PointersRepr {
    One(String),
    Many(Vec<String>),
}

impl From<PointersRepr> for Pointers {
    fn from(repr: PointersRepr) -> Self {
        match repr {
            PointersRepr::One(pointer) => Pointers(vec![pointer]),
            PointersRepr::Many(list) => Pointers(list),
        }
    }
}

impl From<Pointers> for Vec<String> {
    fn from(pointers: Pointers) -> Self {
        pointers.0
    }
}

impl Pointers {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Where each field is in one JSON line (RFC 6901 pointers such as `/message/content`).
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChatMap {
    /// Who wrote the line; compared with `[session_chat.values]`. Required.
    pub role: Pointers,
    /// The message text: a string, or an array of parts (`{type = "text", text = ...}` or plain strings). Required.
    pub text: Pointers,
    #[serde(default)]
    pub tool_name: Pointers,
    #[serde(default)]
    pub tool_input: Pointers,
    /// The id of a tool call, so its result can be paired with it.
    #[serde(default)]
    pub tool_id: Pointers,
    /// Present on a line that is a tool result: the id of the call it answers.
    #[serde(default)]
    pub tool_result_for: Pointers,
    #[serde(default)]
    pub tool_output: Pointers,
    /// A flag on a result: `true`, a non-empty message or a number other than 0 means the tool failed.
    #[serde(default)]
    pub tool_error: Pointers,
    /// A string (RFC 3339) or a number (Unix seconds or milliseconds).
    #[serde(default)]
    pub timestamp: Pointers,
    #[serde(default)]
    pub model: Pointers,
    #[serde(default)]
    pub usage_input: Pointers,
    #[serde(default)]
    pub usage_output: Pointers,
    /// A field whose value (one of `values.turn_end`) says the turn ended.
    #[serde(default)]
    pub turn_end: Pointers,
}

/// The values of `map.role` and `map.turn_end` that matter, compared without regard to letter case.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChatValues {
    #[serde(default = "default_user")]
    pub user: Vec<String>,
    #[serde(default = "default_assistant")]
    pub assistant: Vec<String>,
    #[serde(default = "default_tool")]
    pub tool: Vec<String>,
    /// Required with `map.turn_end`.
    #[serde(default)]
    pub turn_end: Vec<String>,
}

fn default_user() -> Vec<String> {
    vec!["user".to_string()]
}

fn default_assistant() -> Vec<String> {
    vec!["assistant".to_string()]
}

fn default_tool() -> Vec<String> {
    vec!["tool".to_string()]
}

impl Default for ChatValues {
    fn default() -> Self {
        ChatValues { user: default_user(), assistant: default_assistant(), tool: default_tool(), turn_end: Vec::new() }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SessionChatTable {
    pub format: ChatFileFormat,
    /// The folder the CLI writes its transcripts in. Starts with `{home}`, `{cwd}` or `{env:NAME}`; later
    /// segments may use `{cwd_slug}` (every character that is not a letter or digit becomes `-`), `{cwd_dashes}`
    /// (only `/`, `\` and `:` become `-`), `{cwd_url}` (percent-encoded), `{cwd_name}` (the last folder's name)
    /// and `{cwd_sha256}` (hex digest of the folder path). The result must stay under the person's home folder
    /// or the pane's folder.
    pub dir: String,
    /// Which files of `dir` are transcripts: a name with `*` and `?`.
    #[serde(default = "default_glob")]
    pub file_glob: String,
    #[serde(default)]
    pub pick: ChatPick,
    pub map: ChatMap,
    #[serde(default)]
    pub values: ChatValues,
    /// Tool names that change files. Empty: guessed from the name and the input (`content`, `new_string`, ...).
    #[serde(default)]
    pub write_tools: Vec<String>,
    /// Tool names that start a helper agent.
    #[serde(default)]
    pub helper_tools: Vec<String>,
}

fn default_glob() -> String {
    "*.jsonl".to_string()
}

fn error(code: &'static str, path: &str, message: impl Into<String>) -> DefError {
    DefError::new(code, path, message)
}

/// Every problem with a `[session_chat]` table; empty means usable. Codes: `AGENT_DEF_SESSION_CHAT_BAD_DIR`,
/// `AGENT_DEF_SESSION_CHAT_BAD_GLOB`, `AGENT_DEF_SESSION_CHAT_BAD_POINTER` (each names the key) and
/// `AGENT_DEF_INCONSISTENT` (fields that do not fit together).
pub fn validate_session_chat(table: &SessionChatTable) -> Vec<DefError> {
    let mut errors = Vec::new();
    check_dir(&table.dir, &mut errors);
    check_glob(&table.file_glob, &mut errors);
    check_map(&table.map, &mut errors);
    check_values(table, &mut errors);
    for (key, list) in [("write_tools", &table.write_tools), ("helper_tools", &table.helper_tools)] {
        let path = format!("session_chat.{key}");
        if list.len() > MAX_TOOL_NAMES || list.iter().any(|name| name.trim().is_empty() || name.chars().count() > MAX_VALUE_CHARS) {
            errors.push(error("AGENT_DEF_INCONSISTENT", &path, format!("at most {MAX_TOOL_NAMES} tool names of 1-{MAX_VALUE_CHARS} characters")));
        }
    }
    errors
}

/// The placeholder names of `text` (`home`, `env:NAME`, ...), or what is wrong with its braces.
fn placeholders(text: &str) -> Result<Vec<&str>, String> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find(['{', '}']) {
        if rest.as_bytes()[open] == b'}' {
            return Err("a `}` without a matching `{`".to_string());
        }
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            return Err("a `{` without a matching `}`".to_string());
        };
        let name = &after[..close];
        if name.contains('{') {
            return Err("a `{` inside a placeholder".to_string());
        }
        found.push(name);
        rest = &after[close + 1..];
    }
    Ok(found)
}

/// `{env:NAME}`: the variable name, when `placeholder` is one with a plain name.
pub fn env_name(placeholder: &str) -> Option<&str> {
    let name = placeholder.strip_prefix("env:")?;
    let mut chars = name.chars();
    let first = chars.next()?;
    let plain = (first.is_ascii_alphabetic() || first == '_') && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
    (plain && name.len() <= 64).then_some(name)
}

fn is_root_placeholder(name: &str) -> bool {
    ROOT_PLACEHOLDERS.contains(&name) || env_name(name).is_some()
}

fn check_dir(dir: &str, errors: &mut Vec<DefError>) {
    const CODE: &str = "AGENT_DEF_SESSION_CHAT_BAD_DIR";
    let path = "session_chat.dir";
    if dir.trim().is_empty() || dir.chars().count() > MAX_DIR_CHARS || dir.chars().any(char::is_control) {
        errors.push(error(CODE, path, format!("must be 1-{MAX_DIR_CHARS} characters without control characters")));
        return;
    }
    if dir.split(['/', '\\']).any(|part| part == "..") {
        errors.push(error(CODE, path, "must not contain a `..` segment"));
    }
    let names = match placeholders(dir) {
        Ok(names) => names,
        Err(message) => {
            errors.push(error(CODE, path, message));
            return;
        }
    };
    for name in &names {
        if !is_root_placeholder(name) && !SEGMENT_PLACEHOLDERS.contains(name) {
            errors.push(error(
                CODE,
                path,
                format!("unknown placeholder {{{name}}}; allowed: {{home}}, {{cwd}}, {{env:NAME}}, {{cwd_slug}}, {{cwd_dashes}}, {{cwd_url}}, {{cwd_name}}, {{cwd_sha256}}"),
            ));
        }
    }
    let opens_with_root = names.first().is_some_and(|name| dir.starts_with(&format!("{{{name}}}")) && is_root_placeholder(name));
    if !opens_with_root {
        errors.push(error(CODE, path, "must start with {home}, {cwd} or {env:NAME}, so it stays under the home or project folder"));
    }
    if names.iter().skip(usize::from(opens_with_root)).any(|name| is_root_placeholder(name)) {
        errors.push(error(CODE, path, "{home}, {cwd} and {env:NAME} may only open the template"));
    }
}

fn check_glob(glob: &str, errors: &mut Vec<DefError>) {
    const CODE: &str = "AGENT_DEF_SESSION_CHAT_BAD_GLOB";
    let path = "session_chat.file_glob";
    let plain = |c: char| c.is_alphanumeric() || matches!(c, '.' | '_' | '-' | ' ' | '*' | '?');
    if glob.is_empty() || glob.chars().count() > MAX_GLOB_CHARS || !glob.chars().all(plain) || glob.contains("..") {
        errors.push(error(CODE, path, format!("must be 1-{MAX_GLOB_CHARS} characters: letters, digits, `. _ - *` and `?` (a file name, no folders)")));
    } else if !glob.contains('.') {
        errors.push(error(CODE, path, "name the transcripts' extension, for example \"*.jsonl\""));
    }
}

/// Why `pointer` is not a usable RFC 6901 pointer, if it is not.
pub fn pointer_problem(pointer: &str) -> Option<String> {
    if !pointer.starts_with('/') {
        return Some(format!("`{pointer}` must start with `/`, for example /message/content"));
    }
    if pointer.chars().count() > MAX_POINTER_CHARS || pointer.chars().any(char::is_control) {
        return Some(format!("`{pointer}` is longer than {MAX_POINTER_CHARS} characters or has control characters"));
    }
    let segments: Vec<&str> = pointer[1..].split('/').collect();
    if segments.len() > MAX_POINTER_SEGMENTS {
        return Some(format!("`{pointer}` has more than {MAX_POINTER_SEGMENTS} segments"));
    }
    if segments.iter().any(|segment| segment.is_empty()) {
        return Some(format!("`{pointer}` has an empty segment"));
    }
    let bad_escape = segments
        .iter()
        .any(|segment| segment.match_indices('~').any(|(at, _)| !matches!(segment.as_bytes().get(at + 1), Some(b'0' | b'1'))));
    bad_escape.then(|| format!("`{pointer}` uses `~` without 0 or 1 after it (write `~0` for `~` and `~1` for `/`)"))
}

fn check_pointers(key: &str, pointers: &Pointers, required: bool, errors: &mut Vec<DefError>) {
    const CODE: &str = "AGENT_DEF_SESSION_CHAT_BAD_POINTER";
    let path = format!("session_chat.map.{key}");
    if pointers.is_empty() {
        if required {
            errors.push(error(CODE, &path, "is required: a pointer such as /message/content"));
        }
        return;
    }
    if pointers.0.len() > MAX_POINTERS_PER_FIELD {
        errors.push(error(CODE, &path, format!("at most {MAX_POINTERS_PER_FIELD} pointers")));
    }
    let many = pointers.0.len() > 1;
    for (i, pointer) in pointers.0.iter().enumerate() {
        if let Some(problem) = pointer_problem(pointer) {
            let here = if many { format!("{path}.{i}") } else { path.clone() };
            errors.push(error(CODE, &here, problem));
        }
    }
}

/// Every field of the map: (key, pointers, required).
pub fn map_fields(map: &ChatMap) -> [(&'static str, &Pointers, bool); 13] {
    [
        ("role", &map.role, true),
        ("text", &map.text, true),
        ("tool_name", &map.tool_name, false),
        ("tool_input", &map.tool_input, false),
        ("tool_id", &map.tool_id, false),
        ("tool_result_for", &map.tool_result_for, false),
        ("tool_output", &map.tool_output, false),
        ("tool_error", &map.tool_error, false),
        ("timestamp", &map.timestamp, false),
        ("model", &map.model, false),
        ("usage_input", &map.usage_input, false),
        ("usage_output", &map.usage_output, false),
        ("turn_end", &map.turn_end, false),
    ]
}

fn check_map(map: &ChatMap, errors: &mut Vec<DefError>) {
    for (key, pointers, required) in map_fields(map) {
        check_pointers(key, pointers, required, errors);
    }
    let mut inconsistent = |key: &str, message: &str| {
        errors.push(error("AGENT_DEF_INCONSISTENT", &format!("session_chat.map.{key}"), message.to_string()));
    };
    if map.tool_name.is_empty() {
        for (key, pointers) in [("tool_input", &map.tool_input), ("tool_id", &map.tool_id)] {
            if !pointers.is_empty() {
                inconsistent(key, "describes a tool call, which needs map.tool_name");
            }
        }
    }
    if map.tool_result_for.is_empty() {
        for (key, pointers) in [("tool_output", &map.tool_output), ("tool_error", &map.tool_error)] {
            if !pointers.is_empty() {
                inconsistent(key, "describes a tool result, which needs map.tool_result_for");
            }
        }
    }
}

fn check_values(table: &SessionChatTable, errors: &mut Vec<DefError>) {
    let values = &table.values;
    for (key, list) in [("user", &values.user), ("assistant", &values.assistant), ("tool", &values.tool), ("turn_end", &values.turn_end)] {
        let path = format!("session_chat.values.{key}");
        let bad = list.len() > MAX_VALUES || list.iter().any(|v| v.is_empty() || v.chars().count() > MAX_VALUE_CHARS || v.chars().any(char::is_control));
        if bad || (key != "turn_end" && list.is_empty()) {
            errors.push(error("AGENT_DEF_INCONSISTENT", &path, format!("1-{MAX_VALUES} values of 1-{MAX_VALUE_CHARS} characters")));
        }
    }
    let roles = [("user", &values.user), ("assistant", &values.assistant), ("tool", &values.tool)];
    for (i, (key, list)) in roles.iter().enumerate() {
        for (other, theirs) in roles.iter().skip(i + 1) {
            if let Some(shared) = list.iter().find(|v| theirs.iter().any(|w| w.eq_ignore_ascii_case(v))) {
                errors.push(error("AGENT_DEF_INCONSISTENT", &format!("session_chat.values.{key}"), format!("`{shared}` is also a value of `{other}`; a line has one role")));
            }
        }
    }
    match (table.map.turn_end.is_empty(), values.turn_end.is_empty()) {
        (false, true) => errors.push(error("AGENT_DEF_INCONSISTENT", "session_chat.values.turn_end", "map.turn_end needs the values that mean the turn ended")),
        (true, false) => errors.push(error("AGENT_DEF_INCONSISTENT", "session_chat.map.turn_end", "values.turn_end needs a map.turn_end pointer to read")),
        _ => {}
    }
}
