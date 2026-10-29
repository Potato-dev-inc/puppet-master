//! An optional JSONL transcript the fake writes under its working folder, the way a real CLI keeps its conversation, for
//! the tests of "the wizard finds the transcript". Off unless the script says `transcript = "<relative path>"` or the
//! environment variable `PM_FAKE_TUI_TRANSCRIPT` does (it wins). The path is relative to the working folder and may not
//! leave it. The path may hold `{arg:--flag}`: the value the program was started with after `--flag` (a Linux pane gets
//! `--session-id <uuid>` from the real profile); a flag that was not given means no transcript. `PM_FAKE_TUI_TRANSCRIPT_ROOT`
//! (an absolute folder) replaces the working folder as the base, so a fake Linux home can sit outside the project. Shapes: `flat` (`{"role":"user","content":"..."}`, the default) and `nested`
//! (`{"type":"user","message":{"role":"user","content":[{"type":"text","text":"..."}]}}`).

use serde_json::{json, Value};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

/// The arguments the program was started with that it does not know itself (set once by `main`).
static PROGRAM_ARGS: OnceLock<Vec<String>> = OnceLock::new();

/// Remember the arguments `{arg:--flag}` reads from.
pub fn set_program_args(args: Vec<String>) {
    let _ = PROGRAM_ARGS.set(args);
}

/// `{arg:--flag}` placeholders replaced by the value after that flag; `None` when a flag was not given or its value is
/// empty.
fn fill_args(path: &str, args: &[String]) -> Option<String> {
    let mut out = String::new();
    let mut rest = path;
    while let Some(at) = rest.find("{arg:") {
        out.push_str(&rest[..at]);
        let after = &rest[at + 5..];
        let end = after.find('}')?;
        let flag = &after[..end];
        let value = args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).filter(|v| !v.is_empty())?;
        out.push_str(value);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Some(out)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    Flat,
    Nested,
}

pub struct Transcript {
    path: PathBuf,
    shape: Shape,
    calls: u32,
}

impl Transcript {
    /// The transcript the script or the environment asks for, or `None` (also for a path that is absolute or has `..`).
    pub fn from_settings(script_path: Option<&str>, script_shape: Option<&str>, base: &Path) -> Option<Transcript> {
        let relative = std::env::var("PM_FAKE_TUI_TRANSCRIPT").ok().filter(|v| !v.is_empty()).or_else(|| script_path.map(str::to_string))?;
        let relative = PathBuf::from(fill_args(&relative, PROGRAM_ARGS.get().map(Vec::as_slice).unwrap_or(&[]))?);
        if relative.components().any(|c| !matches!(c, Component::Normal(_))) {
            return None;
        }
        let shape = std::env::var("PM_FAKE_TUI_TRANSCRIPT_SHAPE").ok().or_else(|| script_shape.map(str::to_string));
        let shape = if shape.as_deref() == Some("nested") { Shape::Nested } else { Shape::Flat };
        let root = std::env::var("PM_FAKE_TUI_TRANSCRIPT_ROOT").ok().filter(|v| !v.is_empty()).map(PathBuf::from);
        Some(Transcript { path: root.as_deref().unwrap_or(base).join(relative), shape, calls: 0 })
    }

    fn append(&self, line: Value) {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&self.path) {
            let _ = writeln!(file, "{line}");
        }
    }

    fn message(&self, role: &str, text: &str) -> Value {
        match self.shape {
            Shape::Flat => json!({ "role": role, "content": text }),
            Shape::Nested => json!({ "type": role, "message": { "role": role, "content": [{ "type": "text", "text": text }] } }),
        }
    }

    /// What the person typed.
    pub fn user(&self, text: &str) {
        self.append(self.message("user", text));
    }

    /// What the agent said.
    pub fn assistant(&self, text: &str) {
        self.append(self.message("assistant", text));
    }

    /// A tool call and its result (flat shape: two lines; nested: a `tool_use` and a `tool_result` block).
    pub fn tool(&mut self, label: &str, detail: &str) {
        self.calls += 1;
        let id = format!("call{}", self.calls);
        match self.shape {
            Shape::Flat => {
                self.append(json!({ "role": "assistant", "id": id, "name": label, "input": { "command": detail } }));
                self.append(json!({ "role": "tool", "tool_call_id": id, "output": detail }));
            }
            Shape::Nested => {
                self.append(json!({ "type": "assistant", "message": { "role": "assistant", "content": [{ "type": "tool_use", "id": id, "name": label, "input": { "command": detail } }] } }));
                self.append(json!({ "type": "user", "message": { "role": "user", "content": [{ "type": "tool_result", "tool_use_id": id, "content": detail }] } }));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fill_args;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|a| a.to_string()).collect()
    }

    #[test]
    fn a_flag_placeholder_takes_the_value_after_the_flag() {
        let given = args(&["--other", "x", "--session-id", "abc-123"]);
        assert_eq!(fill_args(".claude/projects/p/{arg:--session-id}.jsonl", &given).as_deref(), Some(".claude/projects/p/abc-123.jsonl"));
        assert_eq!(fill_args("plain.jsonl", &given).as_deref(), Some("plain.jsonl"));
    }

    #[test]
    fn a_flag_that_was_not_given_means_no_transcript() {
        assert_eq!(fill_args("{arg:--session-id}.jsonl", &args(&["--other", "x"])), None);
        assert_eq!(fill_args("{arg:--session-id}.jsonl", &args(&["--session-id"])), None, "no value after the flag");
        assert_eq!(fill_args("{arg:--session-id", &args(&["--session-id", "a"])), None, "an unclosed placeholder");
    }
}
