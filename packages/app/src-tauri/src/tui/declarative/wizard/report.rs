//! Errors of the wizard: the HTTP-shaped `WizardError`, and `CheckError`, one problem found in a draft
//! written so an AI can act on it (SPEC 6.2). `render` turns a list of them into the text that goes
//! back into the next prompt verbatim.

use serde::Serialize;
use serde_json::{json, Value};

use crate::tui::declarative::service::ServiceError;

/// A failed request: HTTP status, wire code and a plain message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WizardError {
    pub status: u16,
    pub code: &'static str,
    pub message: String,
}

impl WizardError {
    pub fn new(status: u16, code: &'static str, message: impl Into<String>) -> Self {
        Self { status, code, message: message.into() }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(400, "INVALID_ARGUMENT", message)
    }

    pub fn to_json(&self) -> Value {
        json!({"error": self.message, "message": self.message, "code": self.code, "recoverable": false})
    }
}

impl std::fmt::Display for WizardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl From<WizardError> for ServiceError {
    fn from(e: WizardError) -> Self {
        ServiceError { status: e.status, code: e.code, message: e.message }
    }
}

/// One problem in a draft. `fix` comes from the fixed table in `hints`, so the same mistake always
/// gets the same advice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CheckError {
    /// `size`, `validate`, `wizard`, `conformance`, `fixture` or `livetest`.
    pub stage: &'static str,
    pub code: String,
    /// Dotted TOML path, a fixture file name, or "".
    pub path: String,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub message: String,
    pub fix: String,
    /// Real program output relevant to the problem (fixture and live-test failures only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub excerpt: Option<String>,
}

impl CheckError {
    pub fn new(stage: &'static str, code: impl Into<String>, path: impl Into<String>, message: impl Into<String>) -> Self {
        let code = code.into();
        let path = path.into();
        let fix = super::hints::fix_for(&code, &path);
        let message = message.into().split_whitespace().collect::<Vec<_>>().join(" ");
        Self { stage, code, path, line: None, column: None, message, fix, excerpt: None }
    }

    pub fn at(mut self, line: Option<usize>, column: Option<usize>) -> Self {
        self.line = line;
        self.column = column;
        self
    }

    pub fn with_excerpt(mut self, text: &str) -> Self {
        let cut = excerpt(text);
        if !cut.is_empty() {
            self.excerpt = Some(cut);
        }
        self
    }

    pub fn with_fix(mut self, fix: impl Into<String>) -> Self {
        self.fix = fix.into();
        self
    }
}

pub const EXCERPT_LINES: usize = 20;
pub const EXCERPT_LINE_CHARS: usize = 400;

/// The first lines of a program's output, each cut to a sane length.
pub fn excerpt(text: &str) -> String {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .take(EXCERPT_LINES)
        .map(|l| {
            if l.chars().count() > EXCERPT_LINE_CHARS {
                format!("{}...", l.chars().take(EXCERPT_LINE_CHARS).collect::<String>())
            } else {
                l.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Which step produced the list (decides the first sentence).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Verify,
    LiveTest,
}

/// The text for the next prompt (SPEC 6.2). Empty when there is nothing to fix.
pub fn render(errors: &[CheckError], source: Source) -> String {
    if errors.is_empty() {
        return String::new();
    }
    let count = errors.len();
    let noun = if count == 1 { "problem" } else { "problems" };
    let lead = match source {
        Source::Verify => format!("The checker found {count} {noun}."),
        Source::LiveTest => format!("The live test ran your file against the real program and found {count} {noun}."),
    };
    let mut out = format!(
        "{lead} Fix every one and reply with the complete corrected file in one ```toml block.\n"
    );
    for (i, e) in errors.iter().enumerate() {
        out.push('\n');
        let at = match (e.path.is_empty(), e.line, e.column) {
            (true, Some(l), Some(c)) => format!(" (line {l}, column {c})"),
            (true, Some(l), None) => format!(" (line {l})"),
            (true, None, _) => String::new(),
            (false, Some(l), Some(c)) => format!(" at `{}` (line {l}, column {c})", e.path),
            (false, Some(l), None) => format!(" at `{}` (line {l})", e.path),
            (false, None, _) => format!(" at `{}`", e.path),
        };
        out.push_str(&format!("{}. [{}] {}{}\n", i + 1, e.stage, e.code, at));
        out.push_str(&format!("   Problem: {}\n", e.message));
        if !e.fix.is_empty() {
            out.push_str(&format!("   Fix: {}\n", e.fix));
        }
        if let Some(excerpt) = &e.excerpt {
            out.push_str("   Real output (first lines):\n");
            for line in excerpt.lines() {
                out.push_str(&format!("   {line}\n"));
            }
        }
    }
    out
}
