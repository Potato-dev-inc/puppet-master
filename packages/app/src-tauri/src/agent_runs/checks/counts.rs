//! Test counts ("9 of 12 pass") read from a test runner's output (DESIGN-MISSIONS 2.2).
//!
//! Called on the FULL output of a `command_exits` check (up to 1 MB), before it is cut down to its
//! tail. Each parser recognises one runner's summary; the first that finds one wins. When none
//! does, the caller says "1 of 2 checks pass" and never invents a number.
//!
//! Runners: `node --test` (TAP `# pass` and the spec reporter's `ℹ pass`), Jest and Vitest
//! (`Tests: …`), pytest (`N passed, M failed`), `cargo test` (every `test result:` line summed,
//! `ignored` counted as skipped) and `go test -v` (`--- PASS/FAIL/SKIP`).

use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestCounts {
    pub passed: u32,
    pub failed: u32,
    pub skipped: u32,
    pub total: u32,
}

impl TestCounts {
    fn new(passed: u32, failed: u32, skipped: u32, total: Option<u32>) -> TestCounts {
        TestCounts { passed, failed, skipped, total: total.unwrap_or(passed + failed + skipped) }
    }

    pub fn add(self, other: TestCounts) -> TestCounts {
        TestCounts {
            passed: self.passed + other.passed,
            failed: self.failed + other.failed,
            skipped: self.skipped + other.skipped,
            total: self.total + other.total,
        }
    }
}

static ANSI: Lazy<Regex> = Lazy::new(|| Regex::new(r"\x1b\[[0-9;?]*[ -/]*[@-~]").expect("ansi"));

fn strip(output: &str) -> String {
    ANSI.replace_all(output, "").replace('\r', "")
}

fn num(re: &Regex, text: &str) -> Option<u32> {
    re.captures(text).and_then(|c| c.get(1)).and_then(|m| m.as_str().parse().ok())
}

/// The last match of `re` in `text` (runners print their summary at the end).
fn last_num(re: &Regex, text: &str) -> Option<u32> {
    re.captures_iter(text).last().and_then(|c| c.get(1)).and_then(|m| m.as_str().parse().ok())
}

fn node(text: &str) -> Option<TestCounts> {
    static TESTS: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?m)^\s*(?:#|ℹ)\s+tests\s+(\d+)\s*$").unwrap());
    static PASS: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?m)^\s*(?:#|ℹ)\s+pass\s+(\d+)\s*$").unwrap());
    static FAIL: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?m)^\s*(?:#|ℹ)\s+fail\s+(\d+)\s*$").unwrap());
    static SKIP: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?m)^\s*(?:#|ℹ)\s+skipped\s+(\d+)\s*$").unwrap());
    static TODO: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?m)^\s*(?:#|ℹ)\s+todo\s+(\d+)\s*$").unwrap());
    let passed = last_num(&PASS, text)?;
    let failed = last_num(&FAIL, text).unwrap_or(0);
    let skipped = last_num(&SKIP, text).unwrap_or(0) + last_num(&TODO, text).unwrap_or(0);
    Some(TestCounts::new(passed, failed, skipped, last_num(&TESTS, text)))
}

fn jest_or_vitest(text: &str) -> Option<TestCounts> {
    // Jest: "Tests:       1 failed, 2 skipped, 9 passed, 12 total"
    static JEST: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?m)^\s*Tests:\s+(.+)$").unwrap());
    // Vitest: "      Tests  1 failed | 11 passed (12)"
    static VITEST: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?m)^\s*Tests\s{2,}(.+)$").unwrap());
    static PASSED: Lazy<Regex> = Lazy::new(|| Regex::new(r"(\d+) passed").unwrap());
    static FAILED: Lazy<Regex> = Lazy::new(|| Regex::new(r"(\d+) failed").unwrap());
    static SKIPPED: Lazy<Regex> = Lazy::new(|| Regex::new(r"(\d+) (?:skipped|todo|pending)").unwrap());
    static TOTAL: Lazy<Regex> = Lazy::new(|| Regex::new(r"(\d+) total|\((\d+)\)").unwrap());
    let line = JEST.captures_iter(text).last().or_else(|| VITEST.captures_iter(text).last())?.get(1)?.as_str().to_string();
    let passed = num(&PASSED, &line);
    let failed = num(&FAILED, &line);
    if passed.is_none() && failed.is_none() {
        return None;
    }
    let skipped = SKIPPED.captures_iter(&line).filter_map(|c| c[1].parse::<u32>().ok()).sum();
    let total = TOTAL.captures(&line).and_then(|c| c.get(1).or_else(|| c.get(2))).and_then(|m| m.as_str().parse().ok());
    Some(TestCounts::new(passed.unwrap_or(0), failed.unwrap_or(0), skipped, total))
}

fn pytest(text: &str) -> Option<TestCounts> {
    // "==== 2 failed, 10 passed, 1 skipped in 0.12s ====" or "5 passed in 0.10s"
    static LINE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(?m)^=*\s*((?:\d+ (?:passed|failed|skipped|xfailed|xpassed|errors?|deselected|warnings?)(?:, )?)+) in [\d.]+s").unwrap());
    static PART: Lazy<Regex> = Lazy::new(|| Regex::new(r"(\d+) (passed|failed|skipped|xfailed|xpassed|errors?)").unwrap());
    let line = LINE.captures_iter(text).last()?.get(1)?.as_str().to_string();
    let (mut passed, mut failed, mut skipped) = (0, 0, 0);
    for part in PART.captures_iter(&line) {
        let n: u32 = part[1].parse().unwrap_or(0);
        match &part[2] {
            "passed" | "xpassed" => passed += n,
            "failed" | "error" | "errors" => failed += n,
            _ => skipped += n,
        }
    }
    Some(TestCounts::new(passed, failed, skipped, None))
}

fn cargo(text: &str) -> Option<TestCounts> {
    static LINE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"test result: \w+\. (\d+) passed; (\d+) failed; (\d+) ignored").unwrap());
    let mut sum: Option<TestCounts> = None;
    for c in LINE.captures_iter(text) {
        let n = |i: usize| c[i].parse::<u32>().unwrap_or(0);
        let one = TestCounts::new(n(1), n(2), n(3), None);
        sum = Some(sum.map_or(one, |s| s.add(one)));
    }
    sum
}

fn go(text: &str) -> Option<TestCounts> {
    static ROW: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?m)^\s*--- (PASS|FAIL|SKIP): ").unwrap());
    let (mut passed, mut failed, mut skipped) = (0, 0, 0);
    for c in ROW.captures_iter(text) {
        match &c[1] {
            "PASS" => passed += 1,
            "FAIL" => failed += 1,
            _ => skipped += 1,
        }
    }
    (passed + failed + skipped > 0).then(|| TestCounts::new(passed, failed, skipped, None))
}

/// Counts from a runner's output, or `None` when no known summary is in it.
pub fn parse(output: &str) -> Option<TestCounts> {
    let text = strip(output);
    node(&text).or_else(|| jest_or_vitest(&text)).or_else(|| pytest(&text)).or_else(|| cargo(&text)).or_else(|| go(&text))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(passed: u32, failed: u32, skipped: u32, total: u32) -> Option<TestCounts> {
        Some(TestCounts { passed, failed, skipped, total })
    }

    #[test]
    fn node_tap_summary() {
        let out = "TAP version 13\nok 1 - a\nnot ok 2 - b\n1..2\n# tests 2\n# suites 0\n# pass 1\n# fail 1\n# cancelled 0\n# skipped 0\n# todo 0\n# duration_ms 50\n";
        assert_eq!(parse(out), c(1, 1, 0, 2));
    }

    #[test]
    fn node_spec_reporter_summary() {
        let out = "✔ a (1ms)\n✖ b (2ms)\nℹ tests 4\nℹ suites 0\nℹ pass 2\nℹ fail 1\nℹ cancelled 0\nℹ skipped 1\nℹ todo 0\n";
        assert_eq!(parse(out), c(2, 1, 1, 4));
    }

    #[test]
    fn jest_summary() {
        let out = "\x1b[1mTests:\x1b[22m       1 failed, 2 skipped, 9 passed, 12 total\nSnapshots:   0 total\n";
        assert_eq!(parse(out), c(9, 1, 2, 12));
    }

    #[test]
    fn vitest_summary() {
        let out = " Test Files  1 failed | 2 passed (3)\n      Tests  1 failed | 11 passed (12)\n   Start at  10:00:00\n";
        assert_eq!(parse(out), c(11, 1, 0, 12));
    }

    #[test]
    fn pytest_summary() {
        assert_eq!(parse("===== 2 failed, 10 passed, 1 skipped in 0.12s =====\n"), c(10, 2, 1, 13));
        assert_eq!(parse("5 passed in 0.10s\n"), c(5, 0, 0, 5));
    }

    #[test]
    fn cargo_sums_every_result_line() {
        let out = "test result: ok. 3 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out\n\
                   test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out\n";
        assert_eq!(parse(out), c(5, 1, 1, 7));
    }

    #[test]
    fn go_verbose_rows() {
        let out = "=== RUN TestA\n--- PASS: TestA (0.00s)\n--- FAIL: TestB (0.00s)\n--- SKIP: TestC (0.00s)\nFAIL\n";
        assert_eq!(parse(out), c(1, 1, 1, 3));
    }

    #[test]
    fn unknown_output_gives_no_number() {
        assert_eq!(parse("Build succeeded.\n"), None);
        assert_eq!(parse(""), None);
    }
}
