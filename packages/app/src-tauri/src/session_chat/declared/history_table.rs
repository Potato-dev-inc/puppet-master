//! The optional `[session_chat.history]` table: where an agent keeps ALL its old chats, so the importer can list
//! them (the pane's own `[session_chat]` only reads the newest file). Everything here is data and a load-time
//! check; nothing reads a folder. Absent table = no old chats for that agent.

use serde::{Deserialize, Serialize};

use super::table::{env_name, placeholders, pointer_problem, MAX_DIR_CHARS, MAX_GLOB_CHARS};
use crate::tui::declarative::schema::DefError;

/// Most folders in `roots`.
pub const MAX_HISTORY_ROOTS: usize = 4;
/// Deepest folder level below a root (0 = the root's own files).
pub const MAX_HISTORY_DEPTH: u32 = 4;
/// Most elements in `resume_args`, and longest element in characters.
pub const MAX_RESUME_ARGS: usize = 8;
pub const MAX_RESUME_ARG_CHARS: usize = 64;

const CODE_BAD: &str = "AGENT_DEF_SESSION_CHAT_BAD_HISTORY";
const CODE_INCONSISTENT: &str = "AGENT_DEF_INCONSISTENT";
const SESSION_PLACEHOLDER: &str = "session";

/// Where the chat's id comes from: its file name, or a JSON pointer read from the first lines.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryIdFrom {
    /// The file name without its extension.
    #[default]
    FileStem,
    /// The value at `id_pointer` in the first lines of the file.
    Pointer,
}

/// `[session_chat.history]`. The optional keys are left out of the fingerprint when unset, so a definition
/// without them keeps the fingerprint it was approved under.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryTable {
    /// 1 to 4 folders the CLI keeps its chats in. Each starts with `{home}` or `{env:NAME}`.
    pub roots: Vec<String>,
    /// File names to read, with `*` and `?`, ending in `.jsonl` or `.jsonl.zst`. Unset = `session_chat.file_glob`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_glob: Option<String>,
    /// Folder levels walked below a root, 0 to 4.
    #[serde(default = "default_depth")]
    pub depth: u32,
    #[serde(default)]
    pub id_from: HistoryIdFrom,
    /// The pointer of the chat id. Required with `id_from = "pointer"`, refused otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_pointer: Option<String>,
    /// Where the chat's folder is, looked up in the first lines. Optional.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd_pointer: Option<String>,
    /// Arguments that start the CLI on an old chat. Only `{session}` may be used. Empty = no resume option.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resume_args: Vec<String>,
}

fn default_depth() -> u32 {
    1
}

fn error(code: &'static str, path: &str, message: impl Into<String>) -> DefError {
    DefError::new(code, path, message)
}

/// Every problem with a `[session_chat.history]` table; empty means usable. `file_glob` is the effective glob
/// (the table's own, or `session_chat.file_glob` when it has none). Codes: `AGENT_DEF_SESSION_CHAT_BAD_HISTORY`
/// (each names its key) and `AGENT_DEF_INCONSISTENT` (`id_pointer` and `id_from` do not agree).
pub fn validate_history(history: &HistoryTable, file_glob: &str) -> Vec<DefError> {
    let mut errors = Vec::new();
    check_roots(&history.roots, &mut errors);
    let glob = history.file_glob.as_deref().unwrap_or(file_glob);
    check_glob(glob, &mut errors);
    if history.depth > MAX_HISTORY_DEPTH {
        errors.push(error(CODE_BAD, "session_chat.history.depth", format!("0 to {MAX_HISTORY_DEPTH} folder levels")));
    }
    check_ids(history, &mut errors);
    check_resume(&history.resume_args, &mut errors);
    errors
}

fn check_roots(roots: &[String], errors: &mut Vec<DefError>) {
    if roots.is_empty() || roots.len() > MAX_HISTORY_ROOTS {
        errors.push(error(CODE_BAD, "session_chat.history.roots", format!("1 to {MAX_HISTORY_ROOTS} folders")));
    }
    for (i, root) in roots.iter().enumerate() {
        let path = format!("session_chat.history.roots.{i}");
        if root.trim().is_empty() || root.chars().count() > MAX_DIR_CHARS || root.chars().any(char::is_control) {
            errors.push(error(CODE_BAD, &path, format!("must be 1-{MAX_DIR_CHARS} characters without control characters")));
            continue;
        }
        if root.split(['/', '\\']).any(|part| part == "..") {
            errors.push(error(CODE_BAD, &path, "must not contain a `..` segment"));
        }
        let names = match placeholders(root) {
            Ok(names) => names,
            Err(message) => {
                errors.push(error(CODE_BAD, &path, message));
                continue;
            }
        };
        // Exactly one placeholder, and it opens the folder: `{home}` or `{env:NAME}`. No `{cwd*}` at all.
        let opener = names.first().copied().unwrap_or("");
        let opens_ok = names.len() == 1 && (opener == "home" || env_name(opener).is_some());
        let after = format!("{{{opener}}}").len();
        let ends_ok = root.get(after..).is_some_and(|rest| rest.is_empty() || rest.starts_with(['/', '\\']));
        if !opens_ok || !root.starts_with(&format!("{{{opener}}}")) || !ends_ok {
            errors.push(error(CODE_BAD, &path, "must start with {home} or {env:NAME} and have no other placeholder"));
        }
    }
}

fn check_glob(glob: &str, errors: &mut Vec<DefError>) {
    let path = "session_chat.history.file_glob";
    let plain = |c: char| c.is_alphanumeric() || matches!(c, '.' | '_' | '-' | ' ' | '*' | '?');
    if glob.is_empty() || glob.chars().count() > MAX_GLOB_CHARS || !glob.chars().all(plain) || glob.contains("..") {
        errors.push(error(CODE_BAD, path, format!("must be 1-{MAX_GLOB_CHARS} characters: letters, digits, `. _ - *` and `?` (a file name, no folders)")));
    } else if !(glob.ends_with(".jsonl") || glob.ends_with(".jsonl.zst")) {
        errors.push(error(CODE_BAD, path, "must end in .jsonl or .jsonl.zst"));
    }
}

fn check_ids(history: &HistoryTable, errors: &mut Vec<DefError>) {
    let path = "session_chat.history.id_pointer";
    match (history.id_from, &history.id_pointer) {
        (HistoryIdFrom::Pointer, None) => {
            errors.push(error(CODE_INCONSISTENT, path, "is required with id_from = \"pointer\""));
        }
        (HistoryIdFrom::FileStem, Some(_)) => {
            errors.push(error(CODE_INCONSISTENT, path, "needs id_from = \"pointer\", or remove it"));
        }
        (_, Some(pointer)) => {
            if let Some(problem) = pointer_problem(pointer) {
                errors.push(error(CODE_BAD, path, problem));
            }
        }
        (_, None) => {}
    }
    if let Some(pointer) = &history.cwd_pointer {
        if let Some(problem) = pointer_problem(pointer) {
            errors.push(error(CODE_BAD, "session_chat.history.cwd_pointer", problem));
        }
    }
}

fn check_resume(args: &[String], errors: &mut Vec<DefError>) {
    let path = "session_chat.history.resume_args";
    if args.len() > MAX_RESUME_ARGS {
        errors.push(error(CODE_BAD, path, format!("at most {MAX_RESUME_ARGS} elements")));
    }
    let mut has_session = false;
    for (i, arg) in args.iter().enumerate() {
        let here = format!("{path}.{i}");
        // Counted first, so an element refused for its length does not also read as a missing {session}.
        has_session |= arg.contains("{session}");
        if arg.is_empty() || arg.chars().count() > MAX_RESUME_ARG_CHARS || arg.chars().any(char::is_control) {
            errors.push(error(CODE_BAD, &here, format!("1-{MAX_RESUME_ARG_CHARS} characters without control characters")));
            continue;
        }
        match placeholders(arg) {
            Ok(names) => {
                for name in &names {
                    if *name != SESSION_PLACEHOLDER {
                        errors.push(error(CODE_BAD, &here, format!("unknown placeholder {{{name}}}; the only one is {{session}}")));
                    }
                }
            }
            Err(message) => errors.push(error(CODE_BAD, &here, message)),
        }
    }
    if !args.is_empty() && !has_session {
        errors.push(error(CODE_BAD, path, "must use the {session} placeholder once"));
    }
}
