//! Headless output parsers (HDL-2..HDL-6, HDL-8): `plain_text`, `json_single` and `json_lines`
//! behind one `OutputParser`, shaped like the `TuiProfile` extractors a declarative profile will
//! forward to (T16).
//!
//! Every entry point first caps the text at `max_output_bytes` (keeping the head; a result beyond
//! the cap is simply not seen, so the run fails as unparseable instead of guessing) and strips ANSI
//! when `strip_ansi` is set. `extract_result` is `None` for any non-zero exit code.

use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::Value;

use super::schema::{DefError, HeadlessTable, OutputFormat};
use super::select::{parse_json, CompiledSelector, MAX_LINE_BYTES};

static ANSI: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\x1B(?:\][^\x07\x1B]*(?:\x07|\x1B\\)|[@-Z\\-_]|\[[0-?]*[ -/]*[@-~])").expect("ansi regex"));

/// At most this many `{`-line start offsets are tried when recovering a pretty-printed object.
const MAX_SINGLE_PROBES: usize = 256;

#[derive(Debug, Clone)]
pub struct OutputParser {
    format: OutputFormat,
    strip_ansi: bool,
    max_output_bytes: usize,
    result: Option<CompiledSelector>,
    session: Option<CompiledSelector>,
    model: Option<CompiledSelector>,
    done: Option<CompiledSelector>,
    error: Option<CompiledSelector>,
    rejected: Option<CompiledSelector>,
    warning_code: Option<String>,
}

/// What was read from one output: the parsed JSON values (empty for `plain_text`) and how many
/// non-empty lines could not be parsed.
struct Parsed {
    text: String,
    values: Vec<Value>,
    unparseable_lines: usize,
}

impl OutputParser {
    pub fn compile(headless: &HeadlessTable) -> Result<Self, DefError> {
        let output = headless.output.as_ref().ok_or_else(|| {
            DefError::new("AGENT_DEF_INCONSISTENT", "headless.output", "[headless] needs a [headless.output] table")
        })?;
        let one = |name: &str, sel: &Option<super::schema::Selector>| {
            sel.as_ref()
                .map(|s| CompiledSelector::compile(s, &format!("headless.output.{name}")))
                .transpose()
        };
        Ok(Self {
            format: output.format,
            strip_ansi: output.strip_ansi,
            max_output_bytes: headless.max_output_bytes as usize,
            result: one("result", &output.result)?,
            session: one("session", &output.session)?,
            model: one("model", &output.model)?,
            done: one("done", &output.done)?,
            error: one("error", &output.error)?,
            rejected: one("rejected", &output.rejected)?,
            warning_code: output.rejected.as_ref().and_then(|s| s.warning_code.clone()),
        })
    }

    /// Cap and clean the raw text. Public so callers can report `unparseable_lines`.
    fn prepare(&self, output: &str) -> String {
        let capped = truncate_to(output, self.max_output_bytes);
        if self.strip_ansi { ANSI.replace_all(capped, "").into_owned() } else { capped.to_string() }
    }

    fn parse(&self, text: String) -> Parsed {
        let mut parsed = Parsed { text, values: Vec::new(), unparseable_lines: 0 };
        match self.format {
            OutputFormat::PlainText => {}
            OutputFormat::JsonLines => {
                for line in parsed.text.lines().map(str::trim).filter(|l| !l.is_empty()) {
                    match (line.len() <= MAX_LINE_BYTES).then(|| parse_json(line)).flatten() {
                        Some(value) => parsed.values.push(value),
                        None => parsed.unparseable_lines += 1,
                    }
                }
            }
            OutputFormat::JsonSingle => {
                let (value, bad) = parse_single(&parsed.text);
                parsed.unparseable_lines = bad;
                parsed.values.extend(value);
            }
        }
        parsed
    }

    /// Number of non-empty lines of `output` that did not parse as JSON (always 0 for
    /// `plain_text`). For `json_single` this counts only lines skipped while recovering the object.
    pub fn unparseable_lines(&self, output: &str) -> usize {
        self.parse(self.prepare(output)).unparseable_lines
    }

    fn select(&self, selector: &Option<CompiledSelector>, output: &str) -> Option<String> {
        let selector = selector.as_ref()?;
        let parsed = self.parse(self.prepare(output));
        match self.format {
            OutputFormat::PlainText => selector.select_lines(parsed.text.lines()),
            _ => selector.select(parsed.values.iter()),
        }
    }

    /// HDL-3: `None` for a non-zero exit code and for output the selector does not match.
    /// `plain_text` without a result selector returns the whole trimmed text.
    pub fn extract_result(&self, output: &str, exit_code: i32) -> Option<String> {
        if exit_code != 0 {
            return None;
        }
        match (&self.result, self.format) {
            (None, OutputFormat::PlainText) => {
                let text = self.prepare(output).trim().to_string();
                (!text.is_empty()).then_some(text)
            }
            (None, _) => None,
            (selector, _) => self.select(selector, output),
        }
    }

    /// HDL-4: the first match of `[headless.output.session]`.
    pub fn extract_provider_session(&self, output: &str) -> Option<String> {
        self.select(&self.session, output)
    }

    pub fn extract_model(&self, output: &str) -> Option<String> {
        self.select(&self.model, output)
    }

    /// HDL-6: the message of `[headless.output.error]`, or the serialised matched object.
    pub fn error_message(&self, output: &str) -> Option<String> {
        self.select(&self.error, output)
    }

    /// HDL-5: a line/object matching `done` or `error` has been seen.
    pub fn result_ready(&self, output: &str) -> bool {
        let parsed = self.parse(self.prepare(output));
        [&self.done, &self.error].into_iter().flatten().any(|selector| match self.format {
            OutputFormat::PlainText => parsed.text.lines().any(|l| selector.matches_line(l)),
            _ => parsed.values.iter().any(|v| selector.matches(v)),
        })
    }

    /// The `warning_code` of `[headless.output.rejected]` when a rejected call was reported.
    pub fn rejected_call_warning(&self, output: &str) -> Option<&str> {
        let selector = self.rejected.as_ref()?;
        let parsed = self.parse(self.prepare(output));
        let hit = match self.format {
            OutputFormat::PlainText => parsed.text.lines().any(|l| selector.matches_line(l)),
            _ => parsed.values.iter().any(|v| selector.matches(v)),
        };
        if hit { self.warning_code.as_deref() } else { None }
    }
}

fn truncate_to(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// `json_single`: the whole text, else the last line that is a JSON object, else a pretty-printed
/// object starting on one of the last `{` lines. Returns the value and the count of non-empty
/// lines that were not JSON.
fn parse_single(text: &str) -> (Option<Value>, usize) {
    if let Some(value) = parse_json(text.trim()) {
        return (Some(value), 0);
    }
    let lines: Vec<&str> = text.lines().collect();
    let non_empty = lines.iter().filter(|l| !l.trim().is_empty()).count();
    for line in lines.iter().rev().map(|l| l.trim()).filter(|l| l.starts_with('{') && l.len() <= MAX_LINE_BYTES) {
        if let Some(value @ Value::Object(_)) = parse_json(line) {
            return (Some(value), non_empty.saturating_sub(1));
        }
    }
    let mut offset = 0;
    let mut starts = Vec::new();
    for line in text.split_inclusive('\n') {
        if line.trim_start().starts_with('{') {
            starts.push(offset);
        }
        offset += line.len();
    }
    for start in starts.into_iter().rev().take(MAX_SINGLE_PROBES) {
        if let Some(value @ Value::Object(_)) = parse_json(text[start..].trim()) {
            let skipped = text[..start].lines().filter(|l| !l.trim().is_empty()).count();
            return (Some(value), skipped);
        }
    }
    (None, non_empty)
}
