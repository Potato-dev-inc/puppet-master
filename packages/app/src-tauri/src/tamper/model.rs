//! Types of the tamper guard: what a snapshot holds and what a check reports.
//!
//! Words in `Finding::summary` follow UX-PHASE1 section 0.3: "tests", "checks", "Type check", "Lint".
//! The guard never says "cheated", "tamper", "baseline" or "argv" to a person.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// What a finding is about. The numbers are the rows of DESIGN-MISSIONS section 4.3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// 1. A skip marker was added (`.skip`, `xit`, `@pytest.mark.skip`, `#[ignore]`, `t.Skip`).
    SkipAdded,
    /// 2. A focus marker was added (`.only`, `fit`) or a name filter went into the test command.
    FocusAdded,
    /// 3. Tests were removed, or a test file was deleted or moved out of the test set.
    TestsRemoved,
    /// 4. Fewer assertions in the test set than at the start.
    AssertionsRemoved,
    /// 5. An assertion got weaker.
    AssertionLoosened,
    /// 6. A snapshot file was rewritten or deleted, or an update flag was added.
    SnapshotsRewritten,
    /// 7. How the tests run changed (test script, runner config, strictness flags).
    ChecksChanged,
    /// 8. The runner itself reported fewer tests than at the start.
    FewerTestsRan,
    /// 9. A marker that hides check errors was added (`@ts-ignore`, `eslint-disable`, `# type: ignore`).
    ChecksSilenced,
    /// 10. A protected file changed and nothing above fired.
    TestsChanged,
    /// 11. A changed protected file is in a language the guard has no patterns for.
    CantRead,
}

pub const ALL_KINDS: [Kind; 11] = [
    Kind::SkipAdded,
    Kind::FocusAdded,
    Kind::TestsRemoved,
    Kind::AssertionsRemoved,
    Kind::AssertionLoosened,
    Kind::SnapshotsRewritten,
    Kind::ChecksChanged,
    Kind::FewerTestsRan,
    Kind::ChecksSilenced,
    Kind::TestsChanged,
    Kind::CantRead,
];

impl Kind {
    /// The row number in the design table.
    pub fn number(self) -> u8 {
        ALL_KINDS.iter().position(|k| *k == self).map(|i| i as u8 + 1).unwrap_or(0)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::SkipAdded => "skip_added",
            Kind::FocusAdded => "focus_added",
            Kind::TestsRemoved => "tests_removed",
            Kind::AssertionsRemoved => "assertions_removed",
            Kind::AssertionLoosened => "assertion_loosened",
            Kind::SnapshotsRewritten => "snapshots_rewritten",
            Kind::ChecksChanged => "checks_changed",
            Kind::FewerTestsRan => "fewer_tests_ran",
            Kind::ChecksSilenced => "checks_silenced",
            Kind::TestsChanged => "tests_changed",
            Kind::CantRead => "cant_read",
        }
    }

    pub fn parse(name: &str) -> Option<Kind> {
        ALL_KINDS.iter().copied().find(|k| k.as_str() == name)
    }
}

/// `Blocks` stops "done" and raises the card; `Note` only shows in the timeline and the receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Blocks,
    Note,
}

/// The kinds of check a project runs (UX 0.3: Tests, Type check, Lint, Build). Lint and type-check
/// config and silencing markers only matter when the matching check is in play.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckKind {
    Tests,
    TypeCheck,
    Lint,
    Build,
}

impl CheckKind {
    /// The words a person reads.
    pub fn label(self) -> &'static str {
        match self {
            CheckKind::Tests => "Tests",
            CheckKind::TypeCheck => "Type check",
            CheckKind::Lint => "Lint",
            CheckKind::Build => "Build",
        }
    }

    pub fn parse(name: &str) -> Option<CheckKind> {
        match name {
            "tests" => Some(CheckKind::Tests),
            "typecheck" | "type_check" => Some(CheckKind::TypeCheck),
            "lint" => Some(CheckKind::Lint),
            "build" => Some(CheckKind::Build),
            _ => None,
        }
    }
}

/// Counts from the test runner (DESIGN-MISSIONS section 2.2). Any field may be unknown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunCounts {
    pub total: Option<u32>,
    pub passed: Option<u32>,
    pub failed: Option<u32>,
    pub skipped: Option<u32>,
}

/// One thing the guard noticed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub kind: Kind,
    pub severity: Severity,
    /// Project-relative path with forward slashes. Empty for findings about the test runner's counts.
    pub file: String,
    /// 1-based. The line in the file as it is now; for removals, the line in the file as it was.
    pub line: Option<u32>,
    /// The line (or value) as it was at the start.
    pub before: Option<String>,
    /// The line (or value) as it is now.
    pub after: Option<String>,
    /// One plain sentence a person can read on a card.
    pub summary: String,
    /// How many things this finding stands for (3 removed tests, 2 `@ts-ignore` lines).
    pub count: u32,
    /// The file was edited by the person between rounds, so this is a note about them, not the agent.
    pub by_person: bool,
    /// The marker sits in a test file that did not exist at the start.
    pub in_new_file: bool,
}

impl Finding {
    pub(crate) fn new(kind: Kind, severity: Severity, file: &str, summary: String) -> Finding {
        Finding {
            kind,
            severity,
            file: file.to_string(),
            line: None,
            before: None,
            after: None,
            summary,
            count: 1,
            by_person: false,
            in_new_file: false,
        }
    }
}

/// The result of comparing the project with a snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub snapshot_id: String,
    pub checked_at_ms: u64,
    /// At least one finding blocks "done".
    pub blocks: bool,
    pub findings: Vec<Finding>,
    /// Plain headline: "Tests were changed", "No test changes".
    pub headline: String,
    /// Protected files that differ from the snapshot.
    pub files_changed: u32,
    /// Protected files looked at now.
    pub files_checked: u32,
    /// Some files were too big or not text, so only their size and hash were compared.
    pub partial: bool,
}

impl Report {
    pub fn blocking(&self) -> impl Iterator<Item = &Finding> {
        self.findings.iter().filter(|f| f.severity == Severity::Blocks)
    }

    /// Paths behind blocking findings (what "Undo the test changes" restores).
    pub fn blocking_files(&self) -> Vec<String> {
        let mut files: Vec<String> = self.blocking().filter(|f| !f.file.is_empty()).map(|f| f.file.clone()).collect();
        files.sort();
        files.dedup();
        files
    }
}

/// What a protected file is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// A test file.
    Test,
    /// A snapshot file (`.snap`).
    Snapshot,
    /// Check config (package.json scripts, runner config, strictness flags).
    Config,
}

/// What the snapshot remembers about one protected file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileSnap {
    pub role: Role,
    pub sha256: String,
    pub bytes: u64,
    pub tests: u32,
    pub assertions: u32,
    pub skips: u32,
    pub focus: u32,
    /// The ecosystem that owns the file's patterns; `None` when no ecosystem could read it.
    pub ecosystem: Option<String>,
    /// The bytes as text. `None` when the file is too big, not UTF-8, or the snapshot hit its cap; the
    /// guard then compares hashes and counts only.
    pub content: Option<String>,
}

/// A point-in-time picture of the test files and check config. Self-contained: it carries the file
/// text it needs, so a check never depends on git.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub version: u32,
    pub id: String,
    pub created_at_ms: u64,
    /// Checks in play. Lint and type-check config and markers are only watched for those.
    pub checks: Vec<CheckKind>,
    /// The commands the checks run, as the person would type them.
    pub check_argvs: Vec<Vec<String>>,
    /// Counts from the run at the start, when there was one.
    pub counts: Option<RunCounts>,
    /// Extra globs from `[tamper] protect`.
    pub protect: Vec<String>,
    /// Globs from `[tamper] ignore`.
    pub ignore: Vec<String>,
    pub files: BTreeMap<String, FileSnap>,
    /// Lines that hide check errors (`@ts-ignore`, `eslint-disable`...) in any source file, by path.
    pub silencers: BTreeMap<String, Vec<String>>,
    /// Some content was left out because of the size cap.
    pub partial: bool,
}

pub const SNAPSHOT_VERSION: u32 = 1;
