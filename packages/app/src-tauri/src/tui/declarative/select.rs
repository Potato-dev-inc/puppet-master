//! The JSON selector engine (SPEC 7.1 "Selector"). A selector is compiled once (regexes built
//! with the VAL-2 size limit) and then run over parsed JSON values; garbage, missing paths and
//! over-deep documents yield `None`, never a panic.
//!
//! Evaluation order for one value: every `where` entry (a key starting with `~` asserts that the
//! path exists, or with `false` that it does not), then every `where_regex` entry. A matching value
//! contributes `path` (else the whole object serialised) as a string. `pick` chooses among the
//! contributions, then `regex` captures (group `value`, else group 1, else the whole match), then
//! `trim` runs.

use regex::{Regex, RegexBuilder};
use serde_json::Value;

use super::schema::{DefError, Pick, Scalar, Selector};
use super::validate::REGEX_SIZE_LIMIT;

/// Documents nested deeper than this are treated as unparseable.
pub const MAX_DEPTH: usize = 64;
/// Lines longer than this are skipped (counted as unparseable) by the line-oriented parsers.
pub const MAX_LINE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone)]
pub struct CompiledSelector {
    where_eq: Vec<(String, Scalar)>,
    exists: Vec<(String, bool)>,
    where_regex: Vec<(String, Regex)>,
    path: Option<String>,
    pick: Pick,
    join_with: String,
    max_by: Option<String>,
    trim: bool,
    regex: Option<Regex>,
}

fn build_regex(pattern: &str, path: &str) -> Result<Regex, DefError> {
    RegexBuilder::new(pattern).size_limit(REGEX_SIZE_LIMIT).build().map_err(|err| match err {
        regex::Error::CompiledTooBig(limit) => {
            DefError::new("AGENT_DEF_REGEX_TOO_LARGE", path, format!("compiled regex exceeds {limit} bytes"))
        }
        other => DefError::new("AGENT_DEF_BAD_REGEX", path, other.to_string()),
    })
}

impl CompiledSelector {
    /// `at` is the selector's path in the definition (for error reporting).
    pub fn compile(selector: &Selector, at: &str) -> Result<Self, DefError> {
        let mut where_eq = Vec::new();
        let mut exists = Vec::new();
        for (key, value) in &selector.where_eq {
            match key.strip_prefix('~') {
                Some(path) => exists.push((path.to_string(), !matches!(value, Scalar::Bool(false)))),
                None => where_eq.push((key.clone(), value.clone())),
            }
        }
        let where_regex = selector
            .where_regex
            .iter()
            .map(|(key, pattern)| Ok((key.clone(), build_regex(pattern, &format!("{at}.where_regex.{key}"))?)))
            .collect::<Result<Vec<_>, DefError>>()?;
        let regex = selector.regex.as_deref().map(|p| build_regex(p, &format!("{at}.regex"))).transpose()?;
        Ok(Self {
            where_eq,
            exists,
            where_regex,
            path: selector.path.clone(),
            pick: selector.pick,
            join_with: selector.join_with.clone(),
            max_by: selector.max_by.clone(),
            trim: selector.trim,
            regex,
        })
    }

    /// True when `value` satisfies every `where` / `where_regex` condition.
    pub fn matches(&self, value: &Value) -> bool {
        if !within_depth(value, MAX_DEPTH) {
            return false;
        }
        self.exists.iter().all(|(path, want)| walk(value, path).is_some() == *want)
            && self.where_eq.iter().all(|(path, want)| walk(value, path).is_some_and(|got| scalar_eq(got, want)))
            && self.where_regex.iter().all(|(path, re)| {
                walk(value, path).and_then(scalar_text).is_some_and(|text| re.is_match(&text))
            })
    }

    /// The string a matching value contributes: `path`, else the serialised object.
    fn contribution(&self, value: &Value) -> Option<String> {
        match &self.path {
            Some(path) => walk(value, path).and_then(scalar_text),
            None => serde_json::to_string(value).ok(),
        }
    }

    /// Run over parsed values in order; `None` when nothing matches or the result is empty.
    pub fn select<'a, I>(&self, values: I) -> Option<String>
    where
        I: IntoIterator<Item = &'a Value>,
    {
        let matched: Vec<&Value> = values.into_iter().filter(|v| self.matches(v)).collect();
        let picked = match self.pick {
            Pick::First => matched.iter().find_map(|v| self.contribution(v)),
            Pick::Last => matched.iter().rev().find_map(|v| self.contribution(v)),
            Pick::Join => {
                let parts: Vec<String> = matched.iter().filter_map(|v| self.contribution(v)).collect();
                (!parts.is_empty()).then(|| parts.join(&self.join_with))
            }
            Pick::MaxBy => {
                let key = self.max_by.as_deref()?;
                let mut best: Option<(f64, &Value)> = None;
                for v in &matched {
                    let Some(n) = walk(v, key).and_then(Value::as_f64) else { continue };
                    if best.is_none_or(|(b, _)| n > b) {
                        best = Some((n, v));
                    }
                }
                best.and_then(|(_, v)| self.contribution(v))
            }
        }?;
        self.finish(picked)
    }

    /// Select over text lines (the `plain_text` format): a line is a candidate when it matches
    /// `regex` (all non-empty lines when there is none) and contributes the capture or the line.
    pub fn select_lines<'a, I>(&self, lines: I) -> Option<String>
    where
        I: IntoIterator<Item = &'a str>,
    {
        let candidates: Vec<String> = lines
            .into_iter()
            .filter(|line| line.len() <= MAX_LINE_BYTES && !line.trim().is_empty())
            .filter_map(|line| match &self.regex {
                Some(re) => capture(re, line),
                None => Some(line.to_string()),
            })
            .collect();
        let picked = match self.pick {
            Pick::First => candidates.first().cloned(),
            Pick::Last | Pick::MaxBy => candidates.last().cloned(),
            Pick::Join => (!candidates.is_empty()).then(|| candidates.join(&self.join_with)),
        }?;
        let picked = if self.trim { picked.trim().to_string() } else { picked };
        (!picked.is_empty()).then_some(picked)
    }

    /// True when any line matches `regex` (the line-oriented `matches`).
    pub fn matches_line(&self, line: &str) -> bool {
        line.len() <= MAX_LINE_BYTES && self.regex.as_ref().is_some_and(|re| re.is_match(line))
    }

    fn finish(&self, picked: String) -> Option<String> {
        let captured = match &self.regex {
            Some(re) => capture(re, &picked)?,
            None => picked,
        };
        let out = if self.trim { captured.trim().to_string() } else { captured };
        (!out.is_empty()).then_some(out)
    }
}

fn capture(re: &Regex, text: &str) -> Option<String> {
    let caps = re.captures(text)?;
    let group = caps.name("value").or_else(|| caps.get(1)).or_else(|| caps.get(0))?;
    Some(group.as_str().to_string())
}

/// Dotted path lookup (`a.b.0.c`); numeric segments index arrays. An empty path is the value itself.
pub fn walk<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    if path.is_empty() {
        return Some(value);
    }
    let mut current = value;
    for segment in path.split('.') {
        current = match current {
            Value::Object(map) => map.get(segment)?,
            Value::Array(items) => items.get(segment.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(current)
}

fn scalar_text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Null => None,
        other => serde_json::to_string(other).ok(),
    }
}

fn scalar_eq(got: &Value, want: &Scalar) -> bool {
    match (got, want) {
        (Value::String(a), Scalar::Str(b)) => a == b,
        (Value::Bool(a), Scalar::Bool(b)) => a == b,
        (Value::Number(a), Scalar::Int(b)) => {
            a.as_i64() == Some(*b) || a.as_f64().is_some_and(|f| f.fract() == 0.0 && f == *b as f64)
        }
        _ => false,
    }
}

/// True when `value` nests at most `max` levels deep (iterative: no recursion on hostile input).
pub fn within_depth(value: &Value, max: usize) -> bool {
    let mut stack = vec![(value, 1usize)];
    while let Some((v, depth)) = stack.pop() {
        // depth counts container nesting; scalar leaves add no level
        if !matches!(v, Value::Array(_) | Value::Object(_)) {
            continue;
        }
        if depth > max {
            return false;
        }
        match v {
            Value::Array(items) => stack.extend(items.iter().map(|i| (i, depth + 1))),
            Value::Object(map) => stack.extend(map.values().map(|i| (i, depth + 1))),
            _ => {}
        }
    }
    true
}

/// Parse one JSON text. `None` for garbage or nesting over `MAX_DEPTH` (callers cap the size).
pub fn parse_json(text: &str) -> Option<Value> {
    let value: Value = serde_json::from_str(text).ok()?;
    within_depth(&value, MAX_DEPTH).then_some(value)
}
