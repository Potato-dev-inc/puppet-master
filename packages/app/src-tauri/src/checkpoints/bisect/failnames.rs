//! Names of the failing tests, read from a test runner's plain output (What Broke It, spec 3.3).
//!
//! Pure: text in, names out. It knows the plain line shapes of Jest, Vitest, pytest, `go test`,
//! `cargo test` and TAP (`node --test`). Output that matches none of them gives an empty list, and
//! the caller then says only that tests failed; a number or a name is never invented. At most
//! `MAX_NAMES` names, each at most `MAX_NAME_CHARS` characters. The names come from test output, so
//! the caller redacts them before anything is saved or shown (spec 4, S9).

use once_cell::sync::Lazy;
use regex::Regex;

pub const MAX_NAMES: usize = 5;
pub const MAX_NAME_CHARS: usize = 120;

static RE_ANSI: Lazy<Regex> = Lazy::new(|| Regex::new(r"\x1b\[[0-9;?]*[ -/]*[@-~]").expect("ansi"));
/// Jest and Vitest file header, `FAIL  path`. A tab after FAIL is Go's package line (`FAIL	pkg	0.1s`),
/// which is not a test name, so the name may not contain a tab.
static RE_FAIL_FILE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^FAIL\s+(\S[^\t]*)$").expect("fail file"));
/// Jest and Vitest verbose lines, marked by a cross (`×`, `✕`, `✗`). The plain letter x is not used:
/// it matches code lines such as `x = 1`.
static RE_CROSS: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[×✕✗]\s+(\S.*)$").expect("cross"));
/// A run time at the end of a Jest or Vitest line, `3ms` or `(4 ms)`. It is not part of the name.
static RE_TIMING: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\s+(?:\(\d+(?:\.\d+)?\s?ms\)|\d+(?:\.\d+)?ms)$").expect("timing"));
static RE_PYTEST: Lazy<Regex> = Lazy::new(|| Regex::new(r"^FAILED\s+(\S+)").expect("pytest"));
static RE_CARGO: Lazy<Regex> = Lazy::new(|| Regex::new(r"^test\s+(\S+)\s+\.\.\.\s+FAILED$").expect("cargo"));
static RE_GO: Lazy<Regex> = Lazy::new(|| Regex::new(r"^--- FAIL:\s+(\S+)").expect("go"));
/// TAP (`node --test`): `not ok 2 - name`. A line with no name is named `test 2`.
static RE_TAP: Lazy<Regex> = Lazy::new(|| Regex::new(r"^not ok\s+(\d+)\b\s*-?\s*(.*)$").expect("tap"));

/// The failing test names in `output`, in the order they appear, each listed once.
pub fn failing_tests(output: &str) -> Vec<String> {
    let plain = RE_ANSI.replace_all(output, "");
    let mut names: Vec<String> = Vec::new();
    for line in plain.lines() {
        if names.len() >= MAX_NAMES {
            break;
        }
        let Some(name) = name_on_line(line.trim()) else {
            continue;
        };
        let name = cut(&name);
        if !name.is_empty() && !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

/// The name a single trimmed line gives, if it is a failure line of a known runner.
fn name_on_line(line: &str) -> Option<String> {
    if let Some(c) = RE_FAIL_FILE.captures(line) {
        return Some(c[1].to_string());
    }
    if let Some(c) = RE_CROSS.captures(line) {
        return Some(RE_TIMING.replace(&c[1], "").trim().to_string());
    }
    if let Some(c) = RE_PYTEST.captures(line) {
        return Some(c[1].to_string());
    }
    if let Some(c) = RE_CARGO.captures(line) {
        return Some(c[1].to_string());
    }
    if let Some(c) = RE_GO.captures(line) {
        return Some(c[1].to_string());
    }
    if let Some(c) = RE_TAP.captures(line) {
        let rest = c[2].trim();
        return Some(if rest.is_empty() {
            format!("test {}", &c[1])
        } else {
            rest.to_string()
        });
    }
    None
}

/// At most `MAX_NAME_CHARS` characters; a longer name ends in `…`.
fn cut(name: &str) -> String {
    if name.chars().count() <= MAX_NAME_CHARS {
        return name.to_string();
    }
    let mut short: String = name.chars().take(MAX_NAME_CHARS - 1).collect();
    short.push('…');
    short
}

#[cfg(test)]
mod tests {
    use super::*;

    const JEST: &str = include_str!("fixtures/fail_jest.txt");
    const VITEST: &str = include_str!("fixtures/fail_vitest.txt");
    const PYTEST: &str = include_str!("fixtures/fail_pytest.txt");
    const GO: &str = include_str!("fixtures/fail_go.txt");
    const CARGO: &str = include_str!("fixtures/fail_cargo.txt");
    const TAP: &str = include_str!("fixtures/fail_tap.txt");
    const JUNK: &str = include_str!("fixtures/fail_junk.txt");

    #[test]
    fn jest_names_come_from_fail_headers_and_crosses() {
        assert_eq!(
            failing_tests(JEST),
            vec!["src/strings.test.js", "src/dates.test.js", "parses ISO"]
        );
    }

    #[test]
    fn vitest_names_drop_the_timing_at_the_end() {
        assert_eq!(
            failing_tests(VITEST),
            vec![
                "trims spaces",
                "parses ISO",
                "src/strings.test.ts > strings > trims spaces",
                "src/dates.test.ts > parses ISO",
            ]
        );
    }

    #[test]
    fn pytest_names_are_node_ids_with_windows_and_posix_paths() {
        assert_eq!(
            failing_tests(PYTEST),
            vec![
                "tests/test_strings.py::test_trim",
                "tests\\test_dates.py::test_parse_iso"
            ]
        );
    }

    #[test]
    fn go_names_skip_the_package_line_with_tabs() {
        assert_eq!(failing_tests(GO), vec!["TestAdd", "TestAdd/neg"]);
    }

    #[test]
    fn cargo_names_are_the_failed_test_paths() {
        assert_eq!(failing_tests(CARGO), vec!["math::sub_fails", "strings::trim"]);
    }

    #[test]
    fn tap_names_use_the_description_or_the_number() {
        assert_eq!(failing_tests(TAP), vec!["adds negatives", "test 3"]);
    }

    #[test]
    fn junk_output_gives_no_names() {
        assert!(failing_tests(JUNK).is_empty());
    }

    #[test]
    fn code_lines_that_start_with_x_are_not_names() {
        assert!(failing_tests("    x = compute(3)\nx-ray scan done\n").is_empty());
    }

    #[test]
    fn at_most_five_names_are_kept() {
        let text: String = (1..=7).map(|n| format!("not ok {n} - case {n}\n")).collect();
        assert_eq!(
            failing_tests(&text),
            vec!["case 1", "case 2", "case 3", "case 4", "case 5"]
        );
    }

    #[test]
    fn a_long_name_is_cut_to_120_characters() {
        let long = "a".repeat(200);
        let names = failing_tests(&format!("not ok 1 - {long}\n"));
        assert_eq!(names.len(), 1);
        assert_eq!(names[0].chars().count(), MAX_NAME_CHARS);
        assert!(names[0].ends_with('…'));
    }

    #[test]
    fn crlf_input_gives_the_same_names() {
        let crlf = CARGO.replace('\n', "\r\n");
        assert_eq!(failing_tests(&crlf), vec!["math::sub_fails", "strings::trim"]);
    }

    #[test]
    fn ansi_colour_codes_are_stripped_first() {
        let coloured =
            "\x1b[1m\x1b[31mFAIL\x1b[39m\x1b[22m  src/a.test.js\n\x1b[31m×\x1b[39m adds 1 + 2 \x1b[90m(3 ms)\x1b[39m\n";
        assert_eq!(failing_tests(coloured), vec!["src/a.test.js", "adds 1 + 2"]);
    }

    #[test]
    fn the_same_name_is_listed_once() {
        assert_eq!(
            failing_tests("FAIL  src/a.test.js\n FAIL  src/a.test.js\n"),
            vec!["src/a.test.js"]
        );
    }

    #[test]
    fn a_timing_word_inside_a_name_is_kept() {
        assert_eq!(failing_tests("× retries after 5s\n"), vec!["retries after 5s"]);
    }
}
