//! Fixture expectations (SPEC P3-1/P3-2): every capture `<name>.<ext>` in a folder is described by a
//! sibling `<name>.expect.toml`. One runner serves the generic test and the Settings "Check with
//! fixtures" button, so an author needs no Rust.
//!
//! Headless captures (`headless*`, or `kind = "headless"`) accept `result`, `session`, `model`
//! (strings; "" means none), `done` (bool), `exit` (process exit code fed to the parser, default 0),
//! `error_code` and `error` (the classified failure; "" means no failure).
//! Screen captures (`tui*`, or `kind = "screen"`) accept `busy` (true, false or "unknown"),
//! `menu` (bool), `ready` (bool: `[tui.ready]` says the empty input box is showing), `prompt_kind` ("" means
//! no prompt), `model`, `chrome_lines` and `not_chrome_lines`
//! (lines checked with `is_chrome_line`, lowercased).
//! The runner only reads regular files directly inside the given folder.

use std::path::{Path, PathBuf};

use serde::Serialize;
use toml::Value;

use super::loader::load_text;
use super::sources::Origin;
use super::validate::Source;
use super::DeclarativeProfile;
use crate::tui::TuiProfile;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FixtureResult {
    pub fixture: String,
    pub passed: bool,
    pub failures: Vec<String>,
}

const MAX_FIXTURE_BYTES: u64 = 2 * 1024 * 1024;

fn read_regular(path: &Path) -> Result<String, String> {
    let meta = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !meta.file_type().is_file() {
        return Err("not a regular file".into());
    }
    if meta.len() > MAX_FIXTURE_BYTES {
        return Err(format!("larger than {MAX_FIXTURE_BYTES} bytes"));
    }
    std::fs::read_to_string(path).map_err(|e| e.to_string())
}

/// A capture without its leading `# capture:` header comment lines.
fn body(text: &str) -> &str {
    let mut rest = text;
    while rest.starts_with("# capture:") {
        rest = rest.split_once('\n').map_or("", |(_, tail)| tail);
    }
    rest
}

fn eq_opt(failures: &mut Vec<String>, what: &str, want: &Value, got: Option<String>) {
    let Some(want) = want.as_str() else {
        failures.push(format!("{what}: expected a string"));
        return;
    };
    let got = got.unwrap_or_default();
    if got != want {
        failures.push(format!("{what}: expected {want:?}, got {got:?}"));
    }
}

fn check_headless(p: &DeclarativeProfile, text: &str, expect: &Value, failures: &mut Vec<String>) {
    let exit = expect.get("exit").and_then(Value::as_integer).unwrap_or(0) as i32;
    if let Some(w) = expect.get("result") {
        eq_opt(failures, "result", w, p.extract_result(text, exit));
    }
    if let Some(w) = expect.get("session") {
        eq_opt(failures, "session", w, p.extract_provider_session(text));
    }
    if let Some(w) = expect.get("model") {
        eq_opt(failures, "model", w, p.extract_model(text));
    }
    if let Some(w) = expect.get("done").and_then(Value::as_bool) {
        let got = p.headless_result_ready(text);
        if got != w {
            failures.push(format!("done: expected {w}, got {got}"));
        }
    }
    let failure = p.classify_headless_failure(text, exit);
    if let Some(w) = expect.get("error_code") {
        eq_opt(failures, "error_code", w, failure.as_ref().map(|f| f.0.to_string()));
    }
    if let Some(w) = expect.get("error") {
        eq_opt(failures, "error", w, failure.map(|f| f.1.to_string()));
    }
}

fn check_screen(p: &DeclarativeProfile, text: &str, expect: &Value, failures: &mut Vec<String>) {
    if let Some(w) = expect.get("busy") {
        let want = match (w.as_bool(), w.as_str()) {
            (Some(b), _) => Some(b),
            (None, Some("unknown")) => None,
            _ => {
                failures.push("busy: expected true, false or \"unknown\"".into());
                return;
            }
        };
        let got = p.busy_marker(text);
        if got != want {
            failures.push(format!("busy: expected {want:?}, got {got:?}"));
        }
    }
    if let Some(w) = expect.get("menu").and_then(Value::as_bool) {
        let got = p.looks_like_menu(text);
        if got != w {
            failures.push(format!("menu: expected {w}, got {got}"));
        }
    }
    if let Some(w) = expect.get("ready").and_then(Value::as_bool) {
        let got = p.standby_screen(text) == crate::tui::types::StandbyScreen::Ready;
        if got != w {
            failures.push(format!("ready: expected {w}, got {got}"));
        }
    }
    if let Some(w) = expect.get("prompt_kind") {
        let got = p
            .detect_prompt(text, false)
            .and_then(|v| v.get("kind").and_then(|k| k.as_str()).map(str::to_string));
        eq_opt(failures, "prompt_kind", w, got);
    }
    if let Some(w) = expect.get("model") {
        eq_opt(failures, "model", w, p.detect_footer_model(text));
    }
    for (key, want) in [("summary_lines", true), ("not_summary_lines", false)] {
        for line in expect.get(key).and_then(Value::as_array).into_iter().flatten() {
            let Some(line) = line.as_str() else {
                failures.push(format!("{key}: entries must be strings"));
                continue;
            };
            if p.is_summary_line(&line.trim().to_lowercase()) != want {
                failures.push(format!("{key}: {line:?} should{} be a post-turn summary row", if want { "" } else { " not" }));
            }
        }
    }
    for (key, want) in [("thinking_lines", true), ("not_thinking_lines", false)] {
        for line in expect.get(key).and_then(Value::as_array).into_iter().flatten() {
            let Some(line) = line.as_str() else {
                failures.push(format!("{key}: entries must be strings"));
                continue;
            };
            if p.is_thinking_line(&line.trim().to_lowercase()) != want {
                failures.push(format!("{key}: {line:?} should{} be a thinking row", if want { "" } else { " not" }));
            }
        }
    }
    for (key, want) in [("chrome_lines", true), ("not_chrome_lines", false)] {
        for line in expect.get(key).and_then(Value::as_array).into_iter().flatten() {
            let Some(line) = line.as_str() else {
                failures.push(format!("{key}: entries must be strings"));
                continue;
            };
            if p.is_chrome_line(&line.trim().to_lowercase()) != want {
                failures.push(format!("{key}: {line:?} should{} be a chrome line", if want { "" } else { " not" }));
            }
        }
    }
}

fn kind_of(name: &str, expect: &Value) -> Option<&'static str> {
    match expect.get("kind").and_then(Value::as_str) {
        Some("headless") => Some("headless"),
        Some("screen") => Some("screen"),
        Some(_) => None,
        None if name.starts_with("headless") => Some("headless"),
        None if name.starts_with("tui") => Some("screen"),
        None => None,
    }
}

fn stem_of(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(s, _)| s)
}

/// Runs every capture in `dir` against `profile`. A capture with no expect file, an expect file with
/// no capture, and an expect file that does not parse are each reported as a failed entry.
pub fn check_dir(profile: &DeclarativeProfile, dir: &Path) -> Vec<FixtureResult> {
    let mut names: Vec<String> = match std::fs::read_dir(dir) {
        Ok(rd) => rd
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .filter_map(|e| e.file_name().into_string().ok())
            .collect(),
        Err(e) => {
            return vec![FixtureResult {
                fixture: dir.display().to_string(),
                passed: false,
                failures: vec![format!("cannot read folder: {e}")],
            }]
        }
    };
    names.sort();
    let is_expect = |n: &str| n.ends_with(".expect.toml");
    let mut out = Vec::new();
    for name in names.iter().filter(|n| !is_expect(n)) {
        let stem = stem_of(name);
        if !names.iter().any(|n| n == &format!("{stem}.expect.toml")) {
            out.push(FixtureResult {
                fixture: name.clone(),
                passed: false,
                failures: vec![format!("no expectation file: add {stem}.expect.toml next to it")],
            });
        }
    }
    for expect_name in names.iter().filter(|n| is_expect(n)) {
        let stem = expect_name.trim_end_matches(".expect.toml");
        let mut failures = Vec::new();
        let capture = names.iter().find(|n| !is_expect(n) && stem_of(n) == stem);
        let expect: Option<Value> =
            match read_regular(&dir.join(expect_name)).and_then(|t| t.parse::<Value>().map_err(|e| e.to_string())) {
                Ok(v) => Some(v),
                Err(e) => {
                    failures.push(format!("{expect_name}: {e}"));
                    None
                }
            };
        match (capture, &expect) {
            (None, _) => failures.push(format!("{expect_name}: no capture named {stem}.* in the folder")),
            (Some(capture), Some(expect)) => match read_regular(&dir.join(capture)) {
                Err(e) => failures.push(format!("{capture}: {e}")),
                Ok(text) => match kind_of(capture, expect) {
                    Some("headless") => check_headless(profile, &text, expect, &mut failures),
                    Some(_) => check_screen(profile, body(&text), expect, &mut failures),
                    None => failures.push(format!(
                        "{capture}: cannot tell headless from screen; set kind = \"headless\" or \"screen\""
                    )),
                },
            },
            _ => {}
        }
        out.push(FixtureResult {
            fixture: capture.cloned().unwrap_or_else(|| expect_name.clone()),
            passed: failures.is_empty(),
            failures,
        });
    }
    out
}

/// Outcome of "Check with fixtures". A missing or empty folder is the normal state of a new
/// definition, so it is `no_fixtures` (neutral), never a failed result.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CheckReport {
    /// `"ran"` or `"no_fixtures"`.
    pub status: &'static str,
    /// The resolved fixtures folder.
    pub dir: String,
    /// True when the caller typed the folder instead of using the default next to the file.
    pub explicit: bool,
    /// `kind = "generic"`: captures are optional for experimental agents.
    pub generic: bool,
    /// For `no_fixtures`: `"missing"`, `"empty"` or `"not_a_folder"`.
    pub reason: Option<&'static str>,
    /// The definition's id (the default folder name is the file stem).
    pub name: String,
    pub results: Vec<FixtureResult>,
}

fn load_profile(file: &Path, code_ids: &[&str]) -> Result<DeclarativeProfile, String> {
    let text = read_regular(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let loaded = load_text(file, &text, text.len(), Origin::new(Source::User), None, code_ids);
    if !loaded.errors.is_empty() {
        let list: Vec<String> = loaded.errors.iter().map(|e| format!("{} {}", e.code, e.path)).collect();
        return Err(format!("the definition does not validate: {}", list.join("; ")));
    }
    let def = loaded.definition.ok_or("the definition did not load")?;
    DeclarativeProfile::new(def).map_err(|e| e.to_string())
}

/// The folder `check_file` reads: the given one, else a folder named after the file next to it.
pub fn resolve_fixtures_dir(file: &Path, fixtures_dir: Option<&Path>) -> PathBuf {
    match fixtures_dir {
        Some(d) => d.to_path_buf(),
        None => {
            let stem = file.file_stem().map(PathBuf::from).unwrap_or_default();
            file.parent().map(|p| p.join(&stem)).unwrap_or(stem)
        }
    }
}

/// Loads the definition at `file` (validated as a user definition) and checks it against
/// `fixtures_dir`, default: a folder named after the file (`kilo.toml` -> `kilo/`) next to it.
pub fn check_file(file: &Path, fixtures_dir: Option<&Path>, code_ids: &[&str]) -> Result<Vec<FixtureResult>, String> {
    let profile = load_profile(file, code_ids)?;
    Ok(check_dir(&profile, &resolve_fixtures_dir(file, fixtures_dir)))
}

/// Like `check_file`, but a folder that is missing, not a folder, or holds no files is reported as
/// `no_fixtures` instead of a failure. Real expectation failures stay failed results.
pub fn check_report(file: &Path, fixtures_dir: Option<&Path>, code_ids: &[&str]) -> Result<CheckReport, String> {
    let profile = load_profile(file, code_ids)?;
    let dir = resolve_fixtures_dir(file, fixtures_dir);
    let generic = matches!(profile.definition().agent.kind, super::schema::Kind::Generic);
    let name = file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let reason = match std::fs::metadata(&dir) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Some("missing"),
        Err(_) => None,
        Ok(m) if !m.is_dir() => Some("not_a_folder"),
        Ok(_) => {
            let has_files = std::fs::read_dir(&dir)
                .map(|rd| rd.flatten().any(|e| e.file_type().is_ok_and(|t| t.is_file())))
                .unwrap_or(true);
            if has_files { None } else { Some("empty") }
        }
    };
    let mut report = CheckReport {
        status: if reason.is_some() { "no_fixtures" } else { "ran" },
        dir: dir.display().to_string(),
        explicit: fixtures_dir.is_some(),
        generic,
        reason,
        name,
        results: Vec::new(),
    };
    if reason.is_none() {
        report.results = check_dir(&profile, &dir);
    }
    Ok(report)
}
