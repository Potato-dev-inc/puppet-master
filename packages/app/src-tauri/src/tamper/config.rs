//! Check config: the parts of package.json, runner configs, pyproject.toml, Cargo.toml, tsconfig and
//! lint configs that decide which tests run and how strict the checks are. Which sections matter and
//! which changes are red flags come from the rule tables.

use std::collections::BTreeMap;

use serde_json::Value;

use super::diff::diff_lines;
use super::lexer::split_lines;
use super::model::{CheckKind, Finding, Kind, Severity};
use super::rules::{ConfigFormat, ConfigRule, FlagKind};

pub struct ConfigOutcome {
    pub findings: Vec<Finding>,
    pub plus: u32,
    pub minus: u32,
    /// The compared sections differ.
    pub changed: bool,
}

/// What the compared sections of a config file contain.
enum Extracted {
    /// `path = value`, for json, toml and ini.
    Map(BTreeMap<String, String>),
    /// `(line number, text)`, for plain text (or a config that did not parse).
    Lines(Vec<(u32, String)>),
}

fn how_phrase(check: CheckKind) -> &'static str {
    match check {
        CheckKind::Tests => "how tests run",
        CheckKind::TypeCheck => "how the type check runs",
        CheckKind::Lint => "how lint runs",
        CheckKind::Build => "how the build runs",
    }
}

fn kind_of(flag: FlagKind) -> Kind {
    match flag {
        FlagKind::Checks => Kind::ChecksChanged,
        FlagKind::Focus => Kind::FocusAdded,
        FlagKind::Snapshots => Kind::SnapshotsRewritten,
    }
}

/// Compare one config rule's view of a file before and after.
pub fn compare(file: &str, rule: &ConfigRule, before: Option<&str>, after: Option<&str>, active: &[CheckKind]) -> ConfigOutcome {
    let (old, new) = extract_pair(rule, before, after);
    let mut out = ConfigOutcome { findings: Vec::new(), plus: 0, minus: 0, changed: false };
    match (&old, &new) {
        (Extracted::Map(old), Extracted::Map(new)) => compare_maps(file, rule, old, new, active, &mut out),
        (Extracted::Lines(old), Extracted::Lines(new)) => compare_lines(file, rule, old, new, &mut out),
        _ => unreachable!("extract_pair returns matching shapes"),
    }
    out.findings.dedup_by(|a, b| a.kind == b.kind && a.summary == b.summary);
    out
}

fn extract_pair(rule: &ConfigRule, before: Option<&str>, after: Option<&str>) -> (Extracted, Extracted) {
    let old = before.map(|t| extract(rule, t));
    let new = after.map(|t| extract(rule, t));
    match (old, new) {
        (Some(Extracted::Map(a)), Some(Extracted::Map(b))) => (Extracted::Map(a), Extracted::Map(b)),
        (Some(Extracted::Lines(a)), Some(Extracted::Lines(b))) => (Extracted::Lines(a), Extracted::Lines(b)),
        (None, Some(Extracted::Map(b))) => (Extracted::Map(BTreeMap::new()), Extracted::Map(b)),
        (Some(Extracted::Map(a)), None) => (Extracted::Map(a), Extracted::Map(BTreeMap::new())),
        (None, Some(Extracted::Lines(b))) => (Extracted::Lines(Vec::new()), Extracted::Lines(b)),
        (Some(Extracted::Lines(a)), None) => (Extracted::Lines(a), Extracted::Lines(Vec::new())),
        (None, None) => (Extracted::Map(BTreeMap::new()), Extracted::Map(BTreeMap::new())),
        // One side parsed and the other did not: compare both as plain lines.
        (_, _) => {
            let lines = |t: Option<&str>| t.map(|t| text_lines(rule, t)).unwrap_or_default();
            (Extracted::Lines(lines(before)), Extracted::Lines(lines(after)))
        }
    }
}

fn extract(rule: &ConfigRule, text: &str) -> Extracted {
    let mut map = BTreeMap::new();
    let parsed = match rule.format {
        ConfigFormat::Json => serde_json::from_str::<Value>(text).ok().map(|v| flatten_json("", &v, &mut map)),
        ConfigFormat::Jsonc => serde_json::from_str::<Value>(&strip_jsonc(text)).ok().map(|v| flatten_json("", &v, &mut map)),
        ConfigFormat::Toml => text.parse::<toml::Table>().ok().map(|t| flatten_toml("", &toml::Value::Table(t), &mut map)),
        ConfigFormat::Ini => {
            parse_ini(text, &mut map);
            Some(())
        }
        ConfigFormat::Text => None,
    };
    if parsed.is_none() {
        return Extracted::Lines(text_lines(rule, text));
    }
    if !rule.sections.is_empty() {
        map.retain(|path, _| rule.sections.iter().any(|s| in_section(path, s)));
    }
    Extracted::Map(map)
}

fn in_section(path: &str, section: &str) -> bool {
    path.strip_prefix(section).is_some_and(|rest| rest.is_empty() || rest.starts_with(['.', '[', ':']))
}

fn flatten_json(prefix: &str, value: &Value, out: &mut BTreeMap<String, String>) {
    match value {
        Value::Object(map) if !map.is_empty() => {
            for (k, v) in map {
                let path = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
                flatten_json(&path, v, out);
            }
        }
        other => {
            out.insert(prefix.to_string(), other.to_string());
        }
    }
}

fn flatten_toml(prefix: &str, value: &toml::Value, out: &mut BTreeMap<String, String>) {
    match value {
        toml::Value::Table(table) if !table.is_empty() => {
            for (k, v) in table {
                let path = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
                flatten_toml(&path, v, out);
            }
        }
        toml::Value::Array(items) if !items.is_empty() && items.iter().all(|i| i.is_table()) => {
            for (i, item) in items.iter().enumerate() {
                flatten_toml(&format!("{prefix}[{i}]"), item, out);
            }
        }
        other => {
            out.insert(prefix.to_string(), other.to_string());
        }
    }
}

fn parse_ini(text: &str, out: &mut BTreeMap<String, String>) {
    let mut section = String::new();
    let mut last: Option<String> = None;
    for raw in split_lines(text) {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if raw.starts_with([' ', '\t']) {
            // A continuation of the previous value.
            if let Some(key) = &last {
                if let Some(value) = out.get_mut(key) {
                    value.push(' ');
                    value.push_str(line);
                }
            }
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = name.trim().to_string();
            last = None;
            continue;
        }
        if let Some(at) = line.find(['=', ':']) {
            let key = format!("{section}.{}", line[..at].trim());
            out.insert(key.clone(), line[at + 1..].trim().to_string());
            last = Some(key);
        }
    }
}

/// Lines of a text config, or only the lines of its block when the rule names one.
fn text_lines(rule: &ConfigRule, text: &str) -> Vec<(u32, String)> {
    let lines = split_lines(text);
    let all = lines.iter().enumerate().map(|(i, l)| (i as u32 + 1, l.to_string()));
    let Some(block) = &rule.block else { return all.collect() };
    let Some(start) = lines.iter().position(|l| block.is_match(l)) else { return Vec::new() };
    let mut depth: i32 = 0;
    let mut started = false;
    let mut kept = Vec::new();
    for (i, line) in lines.iter().enumerate().skip(start) {
        kept.push((i as u32 + 1, line.to_string()));
        for ch in line.chars() {
            match ch {
                '{' => {
                    depth += 1;
                    started = true;
                }
                '}' => depth -= 1,
                _ => {}
            }
        }
        if started && depth <= 0 {
            break;
        }
    }
    kept
}

/// Drop comments and trailing commas so tsconfig-style JSON parses.
pub fn strip_jsonc(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let (mut i, mut in_str) = (0, false);
    while i < chars.len() {
        let c = chars[i];
        if in_str {
            out.push(c);
            if c == '\\' && i + 1 < chars.len() {
                out.push(chars[i + 1]);
                i += 1;
            } else if c == '"' {
                in_str = false;
            }
        } else if c == '"' {
            in_str = true;
            out.push(c);
        } else if c == '/' && chars.get(i + 1) == Some(&'/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        } else if c == '/' && chars.get(i + 1) == Some(&'*') {
            i += 2;
            while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            i += 2;
            continue;
        } else if c == ',' {
            // A comma followed only by whitespace and a closing bracket is a trailing comma.
            let mut j = i + 1;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            if !matches!(chars.get(j), Some('}') | Some(']')) {
                out.push(c);
            }
        } else {
            out.push(c);
        }
        i += 1;
    }
    out
}

fn entry_text(path: &str, value: &str) -> String {
    format!("{path} = {value}")
}

/// `the "test" script in package.json` for a script, else `path in file`.
fn subject(rule: &ConfigRule, file: &str, path: &str) -> String {
    match rule.scripts_prefix.as_deref().and_then(|prefix| path.strip_prefix(prefix)) {
        Some(name) => format!("the \"{name}\" script in {file}"),
        None => format!("{path} in {file}"),
    }
}

fn blocking(kind: Kind, file: &str, summary: String) -> Finding {
    Finding::new(kind, Severity::Blocks, file, summary)
}

fn compare_maps(
    file: &str,
    rule: &ConfigRule,
    old: &BTreeMap<String, String>,
    new: &BTreeMap<String, String>,
    active: &[CheckKind],
    out: &mut ConfigOutcome,
) {
    let how = how_phrase(rule.check);
    let script_check = |path: &str| -> Option<CheckKind> {
        let key = path.strip_prefix(rule.scripts_prefix.as_deref()?)?;
        rule.scripts.iter().find(|s| active.contains(&s.check) && s.key.is_match(key)).map(|s| s.check)
    };
    for (path, new_value) in new {
        let old_value = old.get(path);
        if old_value == Some(new_value) {
            continue;
        }
        out.changed = true;
        out.plus += 1;
        if old_value.is_some() {
            out.minus += 1;
        }
        let text = entry_text(path, new_value);
        let mut finding = None;
        if let Some(flag) = rule.flags.iter().find(|f| !f.on_removed && f.regex.is_match(&text)) {
            let summary = format!("Changed {how}: {} {}", subject(rule, file, path), flag.say);
            finding = Some(blocking(kind_of(flag.kind), file, summary));
        } else if let (Some(old_value), Some(check)) = (old_value, script_check(path)) {
            let summary = format!(
                "Changed {}: {} was {} and is now {}",
                how_phrase(check),
                subject(rule, file, path),
                old_value.trim_matches('"'),
                new_value.trim_matches('"')
            );
            finding = Some(blocking(Kind::ChecksChanged, file, summary));
        }
        if let Some(mut f) = finding {
            f.before = old_value.map(|v| entry_text(path, v));
            f.after = Some(text);
            out.findings.push(f);
        }
    }
    for (path, old_value) in old {
        if new.contains_key(path) {
            continue;
        }
        out.changed = true;
        out.minus += 1;
        let text = entry_text(path, old_value);
        let finding = if let Some(flag) = rule.flags.iter().find(|f| f.on_removed && f.regex.is_match(&text)) {
            Some(blocking(kind_of(flag.kind), file, format!("Changed {how}: {} {}", subject(rule, file, path), flag.say)))
        } else {
            script_check(path)
                .map(|check| blocking(Kind::ChecksChanged, file, format!("Changed {}: removed {}", how_phrase(check), subject(rule, file, path))))
        };
        if let Some(mut f) = finding {
            f.before = Some(text);
            out.findings.push(f);
        }
    }
    let old_text: Vec<String> = old.iter().map(|(k, v)| entry_text(k, v)).collect();
    let new_text: Vec<String> = new.iter().map(|(k, v)| entry_text(k, v)).collect();
    thresholds(file, rule, &old_text, &new_text, &mut out.findings);
}

fn compare_lines(file: &str, rule: &ConfigRule, old: &[(u32, String)], new: &[(u32, String)], out: &mut ConfigOutcome) {
    let how = how_phrase(rule.check);
    let old_text: Vec<&str> = old.iter().map(|(_, t)| t.as_str()).collect();
    let new_text: Vec<&str> = new.iter().map(|(_, t)| t.as_str()).collect();
    let diff = diff_lines(&old_text, &new_text);
    out.plus = diff.plus();
    out.minus = diff.minus();
    out.changed = !diff.is_empty();
    for number in &diff.added {
        let (real, text) = &new[*number as usize - 1];
        if let Some(flag) = rule.flags.iter().find(|f| !f.on_removed && f.regex.is_match(text)) {
            let mut f = blocking(kind_of(flag.kind), file, format!("Changed {how}: {file} {}", flag.say));
            f.line = Some(*real);
            f.after = Some(text.trim().to_string());
            out.findings.push(f);
        }
    }
    for number in &diff.removed {
        let (real, text) = &old[*number as usize - 1];
        for flag in rule.flags.iter().filter(|f| f.on_removed && f.regex.is_match(text)) {
            // A reformatted line that still matches is not a removal.
            if new_text.iter().any(|n| flag.regex.is_match(n)) {
                continue;
            }
            let mut f = blocking(kind_of(flag.kind), file, format!("Changed {how}: {file} {}", flag.say));
            f.line = Some(*real);
            f.before = Some(text.trim().to_string());
            out.findings.push(f);
        }
    }
    let old_all: Vec<String> = old.iter().map(|(_, t)| t.clone()).collect();
    let new_all: Vec<String> = new.iter().map(|(_, t)| t.clone()).collect();
    thresholds(file, rule, &old_all, &new_all, &mut out.findings);
}

/// Coverage requirements: a number that went down, or a requirement that went away.
fn thresholds(file: &str, rule: &ConfigRule, old: &[String], new: &[String], findings: &mut Vec<Finding>) {
    if rule.thresholds.is_empty() {
        return;
    }
    let collect = |lines: &[String]| -> BTreeMap<String, f64> {
        let mut map = BTreeMap::new();
        for line in lines {
            for re in &rule.thresholds {
                for caps in re.captures_iter(line) {
                    let key = caps.name("key").map(|k| k.as_str().to_string()).unwrap_or_else(|| "coverage".to_string());
                    if let Some(n) = caps.name("n").and_then(|n| n.as_str().parse::<f64>().ok()) {
                        map.insert(key, n);
                    }
                }
            }
        }
        map
    };
    let (before, after) = (collect(old), collect(new));
    for (key, old_n) in &before {
        let summary = match after.get(key) {
            Some(new_n) if new_n < old_n => {
                Some(format!("Lowered a coverage requirement in {file}: {key} {} → {}", fmt_n(*old_n), fmt_n(*new_n)))
            }
            None => Some(format!("Removed a coverage requirement from {file}: {key} was {}", fmt_n(*old_n))),
            _ => None,
        };
        if let Some(summary) = summary {
            let mut f = blocking(Kind::ChecksChanged, file, summary);
            f.before = Some(format!("{key} {}", fmt_n(*old_n)));
            f.after = after.get(key).map(|n| format!("{key} {}", fmt_n(*n)));
            findings.push(f);
        }
    }
}

fn fmt_n(n: f64) -> String {
    if n.fract() == 0.0 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tamper::rules::Rules;

    fn pkg_rule() -> &'static ConfigRule {
        &Rules::builtin().ecosystem("js").unwrap().config[0]
    }

    #[test]
    fn jsonc_comments_and_trailing_commas_parse() {
        let v: Value = serde_json::from_str(&strip_jsonc("{\n // c\n \"a\": [1, 2,], /* x */ \"b\": \"//no\",\n}")).unwrap();
        assert_eq!(v["b"], "//no");
    }

    #[test]
    fn a_changed_test_script_blocks_and_names_both_values() {
        let before = r#"{"scripts":{"test":"jest","build":"tsc"}}"#;
        let after = r#"{"scripts":{"test":"jest --silent","build":"tsc"}}"#;
        let out = compare("package.json", pkg_rule(), Some(before), Some(after), &[CheckKind::Tests]);
        assert!(out.changed);
        assert_eq!(out.findings.len(), 1);
        assert!(out.findings[0].summary.contains("Changed how tests run"), "{}", out.findings[0].summary);
        assert!(out.findings[0].summary.contains("\"test\" script in package.json"));
    }

    #[test]
    fn a_name_filter_in_the_script_is_a_focus_finding() {
        let out = compare(
            "package.json",
            pkg_rule(),
            Some(r#"{"scripts":{"test":"vitest run"}}"#),
            Some(r#"{"scripts":{"test":"vitest run -t cart"}}"#),
            &[CheckKind::Tests],
        );
        assert_eq!(out.findings[0].kind, Kind::FocusAdded);
    }

    #[test]
    fn other_scripts_are_not_check_scripts_and_lint_needs_the_lint_check() {
        let before = r#"{"scripts":{"start":"node a","lint":"eslint ."}}"#;
        let after = r#"{"scripts":{"start":"node b","lint":"eslint . --quiet"}}"#;
        let out = compare("package.json", pkg_rule(), Some(before), Some(after), &[CheckKind::Tests]);
        assert!(out.changed && out.findings.is_empty());
        let out = compare("package.json", pkg_rule(), Some(before), Some(after), &[CheckKind::Tests, CheckKind::Lint]);
        assert_eq!(out.findings.len(), 1);
    }

    #[test]
    fn a_lowered_threshold_blocks() {
        let rule = Rules::builtin().ecosystem("js").unwrap().config.iter().find(|c| c.block.is_none() && c.globs.is_match("jest.config.js")).unwrap();
        let out = compare("jest.config.js", rule, Some("coverageThreshold: { global: { lines: 90 } }"), Some("coverageThreshold: { global: { lines: 60 } }"), &[CheckKind::Tests]);
        assert!(out.findings.iter().any(|f| f.summary.contains("Lowered a coverage requirement")), "{:?}", out.findings);
    }

    #[test]
    fn a_vite_config_only_counts_its_test_block() {
        let rule = Rules::builtin().ecosystem("js").unwrap().config.iter().find(|c| c.block.is_some()).unwrap();
        let before = "export default {\n  build: { rollupOptions: { exclude: [] } },\n  test: {\n    globals: true,\n  },\n};\n";
        let after_build = before.replace("exclude: []", "exclude: ['x']");
        let after_test = before.replace("globals: true,", "globals: true,\n    exclude: ['src/cart.test.ts'],");
        assert!(compare("vite.config.ts", rule, Some(before), Some(&after_build), &[CheckKind::Tests]).findings.is_empty());
        assert_eq!(compare("vite.config.ts", rule, Some(before), Some(&after_test), &[CheckKind::Tests]).findings.len(), 1);
    }
}
