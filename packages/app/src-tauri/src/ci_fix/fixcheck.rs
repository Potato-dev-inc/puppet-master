//! Red to Green: the push check extras for a fix (SPEC n33 sections 3.1 and 3.3, threat S6).
//!
//! The base check is `issue_intake::pushcheck`, reused as it is. This file adds what a fix can do
//! that a plain intake run cannot: skipped or deleted tests, changes to the files CI runs, web
//! addresses added to the code, and the remote or the history moving. Nothing here changes the copy.
//!
//! Matching is case-insensitive and treats `\` and `/` the same. Skip and web facts are read from
//! added lines only, so a removed or unchanged line never raises a card. Every fact here is a
//! question on the push card; only the held causes stop the push.

use super::model::FixFacts;
use crate::issue_intake::pushcheck;

/// The sentence the intake check ends a held push with. Reused word for word; the tests check that
/// the intake check still says it.
const HELD_LINE: &str = "Puppet Master won't push this. Check the copy yourself.";
/// How many names or places a sentence shows before "and N more".
const SHOWN: usize = 3;
/// Folders that hold tests.
const TEST_FOLDERS: &[&str] = &["test", "tests", "__tests__", "spec"];
/// Skip markers for JavaScript, Python, Rust, Go and C#. Lower case; matched on added lines.
const SKIP_MARKERS: &[&str] = &[
    ".skip(", "xit(", "xdescribe(", "it.only(", "test.only(", "@skip", "@pytest.mark.skip", "@unittest.skip",
    "#[ignore]", "t.skip(", "[ignore]", "[fact(skip",
];
/// Ways an added line can name a web address. Lower case.
const WEB_MARKERS: &[&str] = &["http://", "https://", "curl ", "wget ", "invoke-webrequest"];
/// Files that CI or a build runs, matched by name at any depth. Lower case.
const CI_RUN_NAMES: &[&str] =
    &["package.json", "makefile", "dockerfile", "build.rs", "setup.py", "pyproject.toml", "conftest.py", "cargo.toml"];
/// Files that CI or a build runs, matched by extension at any depth. Lower case.
const CI_RUN_SUFFIXES: &[&str] = &[".sh", ".ps1", ".gradle"];

/// Added lines with a skip marker, as (file, new line number). Capped at 20.
pub fn skipped_tests(unified_diff: &str) -> Vec<(String, u32)> {
    pushcheck::scan_added(unified_diff, &|text: &str| has_skip_marker(text), None).0
}

/// Deleted test files, from `(status, path)` pairs such as `harden::name_status` gives. A deleted
/// file that is not a test is not listed.
pub fn deleted_tests(name_status: &[(String, String)]) -> Vec<String> {
    name_status
        .iter()
        .filter(|(status, path)| status.starts_with('D') && is_test_path(path))
        .map(|(_, path)| slashes(path))
        .collect()
}

/// Files that CI or a build runs (see [`CI_RUN_NAMES`], [`CI_RUN_SUFFIXES`] and any `scripts`
/// folder). Windows separators are written as `/` in the result.
pub fn ci_run_files(files: &[String]) -> Vec<String> {
    files.iter().filter(|path| runs_in_ci(path)).map(|path| slashes(path)).collect()
}

/// Added lines with a web address, as (file, new line number). Capped at 20.
pub fn web_lines(unified_diff: &str) -> Vec<(String, u32)> {
    pushcheck::scan_added(unified_diff, &|text: &str| has_web_address(text), None).0
}

/// The sentences for the push card, and whether the push is held. The intake sentences come
/// first, then the fix extras in card order, then the held sentence last when the push is held.
pub fn reasons(f: &FixFacts) -> (Vec<String>, bool) {
    // A fix is pushed to its own branch, so the intake check's remote-branch sentence is never
    // shown here: it is always false for a fix.
    let mut intake = f.push.clone();
    intake.remote_branch_exists = false;
    let (mut out, _) = pushcheck::reasons(&intake, false);
    if out.last().is_some_and(|line| line.as_str() == HELD_LINE) {
        out.pop();
    }
    if !f.skipped_tests.is_empty() {
        out.push(format!("Skips tests: {}.", places(&f.skipped_tests)));
    }
    if !f.deleted_tests.is_empty() {
        out.push(format!("Deletes test files: {}.", names(&f.deleted_tests)));
    }
    if !f.ci_run_files.is_empty() {
        out.push(format!("Changes files CI runs: {}.", names(&f.ci_run_files)));
    }
    if !f.web_lines.is_empty() {
        out.push(format!("Adds web addresses: {}.", places(&f.web_lines)));
    }
    if f.over_cap {
        out.push(format!(
            "The patch is over {} KiB, so the check did not read all of it.",
            pushcheck::DIFF_MAX_BYTES / 1024
        ));
    }
    if f.remote_moved {
        out.push("Someone pushed to the branch on GitHub since the failure.".to_string());
    }
    if f.history_rewritten {
        out.push("The branch history was rewritten.".to_string());
    }
    if f.wrong_branch {
        out.push("The copy is on another branch.".to_string());
    }
    let held = !f.push.ci_files.is_empty()
        || f.push.git_changed
        || !f.push.internal_files.is_empty()
        || f.remote_moved
        || f.history_rewritten
        || f.wrong_branch;
    if held {
        out.push(HELD_LINE.to_string());
    }
    (out, held)
}

/// Whether an added line holds a skip marker. A marker that starts with a letter must not sit
/// inside a longer word, so `process.exit(` is not `xit(`.
fn has_skip_marker(text: &str) -> bool {
    let line = text.to_ascii_lowercase();
    SKIP_MARKERS.iter().any(|marker| has_word_marker(&line, marker))
}

fn has_word_marker(line: &str, marker: &str) -> bool {
    let starts_with_word = marker.starts_with(|c: char| c.is_ascii_alphanumeric());
    line.match_indices(marker).any(|(at, _)| {
        !starts_with_word
            || line[..at].chars().next_back().map_or(true, |c| !(c.is_ascii_alphanumeric() || c == '_'))
    })
}

fn has_web_address(text: &str) -> bool {
    let line = text.to_ascii_lowercase();
    WEB_MARKERS.iter().any(|marker| line.contains(marker))
}

/// The folder and file parts of a path: `\` read as `/`, lower case, no empty or `.` parts.
fn parts(path: &str) -> Vec<String> {
    path.replace('\\', "/")
        .to_ascii_lowercase()
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .map(str::to_string)
        .collect()
}

/// A path with `/` separators and its case kept, for the card and the lists.
fn slashes(path: &str) -> String {
    path.replace('\\', "/")
}

/// A test file: any test folder on the way, or a name like `x_test.*`, `x.test.*`, `x.spec.*` or
/// `test_x.py`.
fn is_test_path(path: &str) -> bool {
    let parts = parts(path);
    let Some((name, folders)) = parts.split_last() else {
        return false;
    };
    folders.iter().any(|folder| TEST_FOLDERS.contains(&folder.as_str()))
        || name.contains("_test.")
        || name.contains(".test.")
        || name.contains(".spec.")
        || (name.starts_with("test_") && name.ends_with(".py"))
}

/// A file CI or a build runs: a listed name, a listed extension, or anything under a `scripts`
/// folder.
fn runs_in_ci(path: &str) -> bool {
    let parts = parts(path);
    let Some((name, folders)) = parts.split_last() else {
        return false;
    };
    folders.iter().any(|folder| folder.as_str() == "scripts")
        || CI_RUN_NAMES.contains(&name.as_str())
        || CI_RUN_SUFFIXES.iter().any(|suffix| name.ends_with(*suffix))
}

/// Up to three names, then "and N more".
fn names(items: &[String]) -> String {
    tail(items.iter().take(SHOWN).cloned().collect(), items.len())
}

/// Up to three `path line N` places, then "and N more".
fn places(items: &[(String, u32)]) -> String {
    let shown: Vec<String> = items.iter().take(SHOWN).map(|(path, line)| format!("{path} line {line}")).collect();
    tail(shown, items.len())
}

fn tail(shown: Vec<String>, total: usize) -> String {
    let more = total.saturating_sub(SHOWN);
    if more == 0 {
        shown.join(", ")
    } else {
        format!("{} and {} more", shown.join(", "), more)
    }
}

#[cfg(test)]
#[path = "fixcheck_tests.rs"]
mod tests;
