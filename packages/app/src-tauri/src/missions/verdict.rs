//! After every turn: run the mission's checks in the copy, read the test counts, compare the tests
//! with the start (the test-change guard), and take the mission checkpoint. The result is the
//! round's `Verdict`, which the decision table reads.
//!
//! The engine runs the checks itself, once per turn, on the files as they are (also after a turn
//! that timed out or ended with a restart). Round operations carry no checks, so a red check never
//! turns a turn into a failed operation and the follow-up path is the ordinary one.

use super::model::{CheckOutcome, MissionCheck, TestChanges, Verdict};
use crate::agent_runs::checks::{self, counts::TestCounts, Check};
use crate::tamper::{self, CheckKind, RunCounts};
use std::path::{Path, PathBuf};

const TAIL_LINES: usize = 40;

/// On Windows `npm`, `npx` and friends are `.cmd` shims a plain spawn can't find: look the program
/// up on PATH with PATHEXT the way a shell would. Elsewhere (or when nothing matches) the name is
/// used as given.
pub fn resolve_program(program: &str) -> String {
    #[cfg(windows)]
    {
        let p = Path::new(program);
        if p.extension().is_some() || p.components().count() > 1 {
            return program.to_string();
        }
        let exts: Vec<String> = std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into())
            .split(';')
            .filter(|e| !e.is_empty())
            .map(|e| e.to_ascii_lowercase())
            .collect();
        if let Some(path) = std::env::var_os("PATH") {
            for dir in std::env::split_paths(&path) {
                for ext in &exts {
                    let candidate: PathBuf = dir.join(format!("{program}{ext}"));
                    if candidate.is_file() {
                        return candidate.to_string_lossy().into_owned();
                    }
                }
            }
        }
    }
    program.to_string()
}

fn last_lines(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}

/// Runs every check in `copy`, in order.
pub fn run_checks(copy: &Path, list: &[MissionCheck], timeout_ms: u64) -> Vec<CheckOutcome> {
    list.iter()
        .map(|mc| match &mc.check {
            Check::CommandExits { command, expect_exit, timeout_ms: own } => {
                let mut argv = command.clone();
                if let Some(first) = argv.first_mut() {
                    *first = resolve_program(first);
                }
                let run = checks::run_command_capture(copy, &argv, *expect_exit, own.or(Some(timeout_ms)));
                // The detail names the command as the person wrote it, not the resolved path.
                let detail = if run.not_started {
                    format!("`{}` could not start: is it installed?", mc.display())
                } else if run.passed {
                    format!("`{}` passed", mc.display())
                } else if run.timed_out {
                    format!("`{}` was stopped after {} min", mc.display(), timeout_ms / 60_000)
                } else {
                    format!("`{}` failed", mc.display())
                };
                CheckOutcome {
                    name: mc.name.clone(),
                    passed: run.passed,
                    detail,
                    counts: run.counts,
                    not_started: run.not_started,
                    tail: if run.passed { String::new() } else { last_lines(&run.output, TAIL_LINES) },
                }
            }
            other => {
                let results = checks::evaluate_with_commands(copy, std::slice::from_ref(other), None, &Default::default());
                let result = results.into_iter().next();
                CheckOutcome {
                    name: mc.name.clone(),
                    passed: result.as_ref().is_some_and(|r| r.passed),
                    detail: result.map(|r| r.detail).unwrap_or_default(),
                    counts: None,
                    not_started: false,
                    tail: String::new(),
                }
            }
        })
        .collect()
}

/// The guard's kind for a check, from its name.
pub fn check_kind(name: &str) -> CheckKind {
    match name.to_ascii_lowercase().as_str() {
        n if n.contains("type") => CheckKind::TypeCheck,
        n if n.contains("lint") => CheckKind::Lint,
        n if n.contains("build") => CheckKind::Build,
        _ => CheckKind::Tests,
    }
}

pub fn summed_counts(outcomes: &[CheckOutcome]) -> Option<TestCounts> {
    outcomes.iter().filter_map(|c| c.counts).reduce(TestCounts::add)
}

pub fn run_counts(counts: Option<TestCounts>) -> Option<RunCounts> {
    counts.map(|c| RunCounts { total: Some(c.total), passed: Some(c.passed), failed: Some(c.failed), skipped: Some(c.skipped) })
}

/// The failing output with numbers, paths and timestamps taken out: two turns with the same key
/// failed the same way.
pub fn failing_key(outcomes: &[CheckOutcome]) -> String {
    outcomes
        .iter()
        .filter(|c| !c.passed)
        .flat_map(|c| c.tail.lines().map(|l| crate::loop_guard::normalize(l, false)).collect::<Vec<_>>())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Compares the copy with the test snapshot taken at the start.
pub fn test_changes(
    copy: &Path,
    snap: Option<&tamper::Snapshot>,
    list: &[MissionCheck],
    counts: Option<TestCounts>,
    may_change_tests: bool,
    edited_by_person: &[String],
) -> TestChanges {
    let Some(snap) = snap else { return TestChanges::default() };
    let argvs: Vec<Vec<String>> = list.iter().filter_map(|c| c.argv().map(<[String]>::to_vec)).collect();
    let report = tamper::check(
        copy,
        snap,
        &tamper::CheckOptions {
            may_change_tests,
            edited_by_person: edited_by_person.to_vec(),
            counts: run_counts(counts),
            check_argvs: Some(argvs),
        },
    );
    TestChanges { blocks: report.blocks, findings: report.findings }
}

/// Puts the verdict together.
pub fn build(outcomes: Vec<CheckOutcome>, changes: TestChanges, tree: String, checkpoint: String) -> Verdict {
    let counts = summed_counts(&outcomes);
    let checks_total = outcomes.len() as u32;
    let checks_passed = outcomes.iter().filter(|c| c.passed).count() as u32;
    let failing_key = failing_key(&outcomes);
    Verdict { checks: outcomes, counts, checks_passed, checks_total, test_changes: changes, tree, checkpoint, failing_key }
}

/// The snapshot options for the start: the checks in play, their commands and the start counts.
pub fn snapshot_options(list: &[MissionCheck], counts: Option<TestCounts>) -> tamper::SnapshotOptions {
    let mut kinds: Vec<CheckKind> = list.iter().map(|c| check_kind(&c.name)).collect();
    kinds.sort();
    kinds.dedup();
    tamper::SnapshotOptions {
        checks: kinds,
        check_argvs: list.iter().filter_map(|c| c.argv().map(<[String]>::to_vec)).collect(),
        counts: run_counts(counts),
        ..Default::default()
    }
}
