//! An optional JSONL transcript the fake writes under its working folder, the way a real CLI keeps its conversation, for
//! the tests of "the wizard finds the transcript". Off unless the script says `transcript = "<relative path>"` or the
//! environment variable `PM_FAKE_TUI_TRANSCRIPT` does (it wins). The path is relative to the working folder and may not
//! leave it. Shapes: `flat` (`{"role":"user","content":"..."}`, the default) and `nested`
//! (`{"type":"user","message":{"role":"user","content":[{"type":"text","text":"..."}]}}`).

use serde_json::{json, Value};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

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
        let relative = PathBuf::from(relative);
        if relative.components().any(|c| !matches!(c, Component::Normal(_))) {
            return None;
        }
        let shape = std::env::var("PM_FAKE_TUI_TRANSCRIPT_SHAPE").ok().or_else(|| script_shape.map(str::to_string));
        let shape = if shape.as_deref() == Some("nested") { Shape::Nested } else { Shape::Flat };
        Some(Transcript { path: base.join(relative), shape, calls: 0 })
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
