//! What one file says about its tests: how many tests and assertions, and where the skip, focus and
//! silencing markers are. All patterns come from the language's rule table.

use super::lexer::{lex, LineView};
use super::model::CheckKind;
use super::rules::{Ecosystem, NameFrom, Pat};

/// One skip, focus or silencing marker found on a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marker {
    /// 1-based.
    pub line: u32,
    /// What to call it in a sentence: `.skip`, `xit`, `@ts-ignore`.
    pub label: String,
    /// The test it applies to, when it can be told.
    pub name: Option<String>,
    /// The whole line, trimmed.
    pub text: String,
    /// For silencing markers: the check whose errors it hides.
    pub check: Option<CheckKind>,
}

impl Marker {
    /// What makes two markers "the same one" when a test moves between files or a formatter reflows
    /// a line: the test name when there is one, else the line with whitespace collapsed.
    pub fn key(&self) -> String {
        match &self.name {
            Some(name) => format!("{}|{}", self.label, name),
            None => collapse_ws(&self.text),
        }
    }
}

pub fn collapse_ws(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Debug, Clone, Default)]
pub struct Facts {
    pub tests: u32,
    pub assertions: u32,
    pub skips: Vec<Marker>,
    pub focus: Vec<Marker>,
    pub silencers: Vec<Marker>,
    pub tautologies: Vec<Marker>,
    pub lines: Vec<LineView>,
}

fn count(pats: &[Pat], text: &str) -> u32 {
    pats.iter().map(|p| p.regex.find_iter(text).count() as u32).sum()
}

fn label_of(pat: &Pat, matched: &str) -> String {
    if !pat.label.is_empty() {
        return pat.label.clone();
    }
    let compact: String = matched.chars().filter(|c| !c.is_whitespace()).collect();
    compact
        .trim_start_matches(|c: char| !(c.is_alphanumeric() || c == '@' || c == '#' || c == '_'))
        .trim_end_matches('(')
        .to_string()
}

/// A file that is a test file only because it holds test code (Rust unit tests) is counted from
/// its first test marker on, so the implementation above it does not count as tests.
pub fn region_only(eco: &Ecosystem, path: &str) -> bool {
    !eco.test_markers.is_empty() && !eco.test_globs.is_match(path)
}

/// `region_only`: count tests and assertions only from the first line that matches a test marker.
pub fn analyze(eco: &Ecosystem, text: &str, region_only: bool) -> Facts {
    let lines = lex(&eco.lexer, text);
    let mut facts = Facts::default();

    // Test definition lines, for naming a skip that sits on the line before its test.
    let mut defs: Vec<(u32, String)> = Vec::new();
    if let Some(def) = &eco.def {
        for (i, view) in lines.iter().enumerate() {
            if let Some(name) = def.captures(&view.code).and_then(|c| c.name("name")) {
                defs.push((i as u32 + 1, name.as_str().to_string()));
            }
        }
    }

    let region_start = if region_only {
        lines.iter().position(|v| eco.test_markers.iter().any(|m| m.is_match(&v.code))).unwrap_or(lines.len())
    } else {
        0
    };
    for (i, view) in lines.iter().enumerate() {
        let number = i as u32 + 1;
        if i >= region_start {
            facts.tests += count(&eco.test, &view.code);
            facts.assertions += count(&eco.assertion, &view.code);
        }
        let marker = |pat: &Pat, m: regex::Match<'_>, check: Option<CheckKind>| Marker {
            line: number,
            label: label_of(pat, m.as_str()),
            name: name_for(eco, pat, &lines, i, m.start(), &defs),
            text: view.raw.trim().to_string(),
            check,
        };
        if let Some((pat, m)) = first_match(&eco.skip, &view.code) {
            facts.skips.push(marker(pat, m, None));
        }
        if let Some((pat, m)) = first_match(&eco.focus, &view.code) {
            facts.focus.push(marker(pat, m, None));
        }
        if let Some((pat, m)) = first_match(&eco.silence, &view.nostr) {
            facts.silencers.push(marker(pat, m, pat.check));
        }
        if let Some((pat, m)) = first_match(&eco.tautology, &view.code) {
            facts.tautologies.push(marker(pat, m, None));
        }
    }
    facts.lines = lines;
    facts
}

/// Only the markers that hide check errors: what the walk needs from source files that are not tests.
pub fn silencers(eco: &Ecosystem, text: &str) -> Vec<Marker> {
    if eco.silence.is_empty() {
        return Vec::new();
    }
    let mut found = Vec::new();
    for (i, view) in lex(&eco.lexer, text).iter().enumerate() {
        if let Some((pat, m)) = first_match(&eco.silence, &view.nostr) {
            found.push(Marker {
                line: i as u32 + 1,
                label: label_of(pat, m.as_str()),
                name: None,
                text: view.raw.trim().to_string(),
                check: pat.check,
            });
        }
    }
    found
}

fn first_match<'p, 't>(pats: &'p [Pat], text: &'t str) -> Option<(&'p Pat, regex::Match<'t>)> {
    pats.iter().find_map(|p| p.regex.find(text).map(|m| (p, m)))
}

fn name_for(eco: &Ecosystem, pat: &Pat, lines: &[LineView], idx: usize, start: usize, defs: &[(u32, String)]) -> Option<String> {
    let number = idx as u32 + 1;
    match pat.name_from {
        NameFrom::None => None,
        NameFrom::Quoted => {
            let quoted = eco.quoted.as_ref()?;
            let here = &lines[idx].raw[start.min(lines[idx].raw.len())..];
            let found = quoted.captures(here).or_else(|| lines.get(idx + 1).and_then(|n| quoted.captures(&n.raw)));
            found.and_then(|c| c.name("name")).map(|n| n.as_str().trim().to_string()).filter(|n| !n.is_empty())
        }
        NameFrom::NextDef => defs.iter().find(|(line, _)| *line >= number && *line <= number + 10).map(|(_, n)| n.clone()),
        NameFrom::PrevDef => defs.iter().rev().find(|(line, _)| *line <= number).map(|(_, n)| n.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tamper::rules::Rules;

    fn eco(id: &str) -> &'static Ecosystem {
        Rules::builtin().ecosystem(id).unwrap()
    }

    #[test]
    fn js_counts_tests_assertions_and_names_the_skipped_test() {
        let facts = analyze(
            eco("js"),
            "describe('cart', () => {\n  it('adds', () => { expect(add(1, 2)).toBe(3); });\n  it.skip(\"handles 0.005\", () => { expect(a).toBe(1); expect(b).toBe(2); });\n  // it.only('x')\n  const s = 'it.only(';\n  regex.test(x);\n});",
            false,
        );
        assert_eq!(facts.tests, 2);
        assert_eq!(facts.assertions, 3);
        assert_eq!(facts.skips.len(), 1);
        assert_eq!(facts.skips[0].label, ".skip");
        assert_eq!(facts.skips[0].name.as_deref(), Some("handles 0.005"));
        assert_eq!(facts.skips[0].line, 3);
        assert!(facts.focus.is_empty(), "a comment and a string are not focus markers");
    }

    #[test]
    fn python_decorator_skip_is_named_from_the_next_def() {
        let facts = analyze(eco("python"), "import pytest\n\n@pytest.mark.skip(reason='later')\ndef test_rounds():\n    assert round(2.5) == 2\n", false);
        assert_eq!(facts.tests, 1);
        assert_eq!(facts.assertions, 1);
        assert_eq!(facts.skips[0].label, "@pytest.mark.skip");
        assert_eq!(facts.skips[0].name.as_deref(), Some("test_rounds"));
    }

    #[test]
    fn rust_ignore_and_asserts_and_silencers() {
        let text = "fn total() { assert!(true); }
#[cfg(test)]
mod t {
    #[test]
    #[ignore]
    fn slow() { assert_eq!(1, 1); }
}
#[allow(dead_code)]
fn f() {}
";
        let facts = analyze(eco("rust"), text, false);
        assert_eq!((facts.tests, facts.assertions), (1, 2));
        // A file that is a test file only because it holds unit tests counts from its first test marker on.
        let facts = analyze(eco("rust"), text, true);
        assert_eq!((facts.tests, facts.assertions), (1, 1));
        assert_eq!(facts.skips[0].name.as_deref(), Some("slow"));
        assert_eq!(facts.silencers[0].check, Some(CheckKind::Lint));
    }

    #[test]
    fn go_skip_is_named_from_the_enclosing_test() {
        let facts = analyze(eco("go"), "func TestA(t *testing.T) {\n\tt.Skip(\"later\")\n\tt.Errorf(\"x\")\n}\n", false);
        assert_eq!(facts.tests, 1);
        assert_eq!(facts.skips[0].name.as_deref(), Some("TestA"));
        assert_eq!(facts.assertions, 1);
    }
}
