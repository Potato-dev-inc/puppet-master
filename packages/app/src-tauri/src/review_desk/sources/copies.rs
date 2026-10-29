//! Copies and missions (SPEC n33 section 3.4 C): finished separate copies that nobody reviewed yet.
//!
//! Pure. The live layer reads the copies, their last run, their changes and their scan; this file only
//! turns those facts into items and does no I/O. `gather` fills `risk` (the order needs the path roles)
//! and `fingerprint` afterwards, so both are placeholders here, as are `agent_label` (from the registry)
//! and `group`. Titles, file names and scan lines pass through the injected `Clean` (S4, S5).

use std::collections::HashSet;

use crate::issue_intake::pushcheck::{ci_paths, secret_paths};
use crate::missions::model::HANDLE_PREFIX;
use crate::review_desk::model::{strip_controls, CheckFacts, Clean, Facts, FileRow, Item, ItemKey, Keep, Level, Risk, Source, Throw, TITLE_MAX};
use crate::worktrees::service::ListItem;
use crate::worktrees::{Changes, WorktreeEntry};

/// At most this many failed check names are kept (SPEC 3.2).
const FAILED_MAX: usize = 5;
/// Title of a copy that has no task text at all.
const FALLBACK_TITLE: &str = "Finished agent work";
/// `where_label` of a mission copy (the list in `model::Item`).
const MISSION_WHERE: &str = "Mission";
/// Placeholder until `gather` sets the real order (SPEC 3.3 says tier 7 is "no flags").
const PLACEHOLDER_TIER: u8 = 7;

/// One copy as the live layer read it. `changes` is `None` when the file list could not be read.
#[derive(Debug, Clone)]
pub struct CopyInput {
    /// The copy from `worktrees::service::list` (state, handle, base, repo, pane, summary).
    pub item: ListItem,
    pub changes: Option<Changes>,
    /// The last run that wrote into the copy, if the registry still knows it.
    pub op: Option<OpFacts>,
    /// Set for a mission copy (handle `mission-<id>`).
    pub mission: Option<MissionFacts>,
    /// The redacted scan of the patch, if the live layer ran one.
    pub scan: Option<ScanFacts>,
}

/// The facts of the last run in a copy.
#[derive(Debug, Clone)]
pub struct OpFacts {
    /// The run's status. Kept for the detail; the order does not use it.
    pub status: String,
    /// The task text the person gave (cleaned here before it becomes the title).
    pub task: String,
    /// When the run finished; a copy with no run uses its creation time.
    pub finished_at_ms: u64,
    /// `(check name, passed)` in the order they ran. Empty = no checks.
    pub check_results: Vec<(String, bool)>,
    pub account: Option<String>,
}

/// The last verdict of a mission copy.
#[derive(Debug, Clone)]
pub struct MissionFacts {
    pub objective: String,
    pub checks_passed: u32,
    pub checks_total: u32,
    /// Tamper blocking sentences from the last verdict.
    pub weakened: Vec<String>,
}

/// The line scan of the copy's patch (already masked by the live layer's rules).
#[derive(Debug, Clone)]
pub struct ScanFacts {
    pub secret_lines: Vec<(String, u32)>,
    pub home_lines: Vec<(String, u32)>,
    /// `false` when the patch was too big or not read; then only names were checked.
    pub scanned: bool,
}

/// Turns the copies in state `unreviewed` into items. A copy whose handle is in `owned` belongs to Night
/// Shift or an issue run, so it is not listed here. Items come back unordered (`gather` orders them).
pub fn from_copies(list: &[CopyInput], owned: &HashSet<String>, clean: &Clean) -> Vec<Item> {
    list.iter().filter_map(|input| item_of(input, owned, clean)).collect()
}

fn item_of(input: &CopyInput, owned: &HashSet<String>, clean: &Clean) -> Option<Item> {
    let list_item = &input.item;
    if list_item.state != "unreviewed" || !list_item.entry.exists {
        return None;
    }
    let worktree = &list_item.entry.worktree;
    let handle = worktree.handle.as_str();
    if owned.contains(handle) {
        return None;
    }
    let key = ItemKey::parse(&format!("copy:{handle}"))?;
    let is_mission = handle.starts_with(HANDLE_PREFIX);

    let facts = facts_of(input, is_mission, clean);
    let title = title_of(input, is_mission, clean);
    let base = tidy(&worktree.base, clean);
    let where_label = if is_mission { MISSION_WHERE.to_string() } else { format!("Separate copy of {base}") };
    let finished_at_ms = input.op.as_ref().map_or(worktree.created_at_ms, |op| op.finished_at_ms);
    let pane_id = list_item.pane_id.clone();
    let fix = match &pane_id {
        Some(pane) => Ok(pane.clone()),
        None => Err("The agent's pane is closed.".to_string()),
    };

    Some(Item {
        source: if is_mission { Source::Mission } else { Source::Copy },
        fingerprint: String::new(),
        title,
        agent_label: String::new(),
        account: input.op.as_ref().and_then(|op| op.account.clone()),
        where_label,
        project: worktree.repo.clone(),
        pane_id,
        finished_at_ms,
        turns: 1,
        group: None,
        facts,
        risk: Risk { tier: PLACEHOLDER_TIER, level: Level::Low, reasons: Vec::new() },
        keep: Keep::Merge { project: worktree.repo.clone(), handle: handle.to_string(), into: worktree.base.clone() },
        throw: Throw::Discard { project: worktree.repo.clone(), handle: handle.to_string() },
        fix,
        key,
    })
}

/// Every fact the order is made from. Names and sentences are cleaned here, so nothing raw leaves this file.
fn facts_of(input: &CopyInput, is_mission: bool, clean: &Clean) -> Facts {
    let raw_paths: Vec<String> = input
        .changes
        .as_ref()
        .map(|changes| changes.files.iter().map(|file| file.path.clone()).collect())
        .unwrap_or_default();

    // Totals: the live file list when there is one, else the copy's own live summary.
    let summary = input.changes.as_ref().map(Changes::summary).or(input.item.summary);
    let (files_total, added, deleted) = match summary {
        Some(summary) => (count(summary.files_changed), summary.additions, summary.deletions),
        None => (0, 0, 0),
    };

    let files = input.changes.as_ref().map(|changes| {
        changes
            .files
            .iter()
            .map(|file| FileRow {
                path: tidy(&file.path, clean),
                status: status_of(&file.status),
                added: file.additions,
                deleted: file.deletions,
                binary: file.binary,
            })
            .collect::<Vec<FileRow>>()
    });

    let (secret_lines, home_lines, lines_scanned) = match &input.scan {
        Some(scan) => (cleaned_lines(&scan.secret_lines, clean), cleaned_lines(&scan.home_lines, clean), scan.scanned),
        None => (Vec::new(), Vec::new(), false),
    };

    let (weakened, checks) = match (is_mission, &input.mission) {
        (true, Some(mission)) => (
            mission.weakened.iter().map(|sentence| tidy(sentence, clean)).collect(),
            mission_checks(mission),
        ),
        (true, None) => (Vec::new(), None),
        (false, _) => (Vec::new(), input.op.as_ref().and_then(|op| checks_of(&op.check_results, clean))),
    };

    Facts {
        files,
        files_total,
        added,
        deleted,
        weakened,
        checks,
        // Found on the raw paths (a cleaned name could hide the pattern), then shown cleaned.
        secret_files: secret_paths(&raw_paths).iter().map(|path| tidy(path, clean)).collect(),
        ci_files: ci_paths(&raw_paths).iter().map(|path| tidy(path, clean)).collect(),
        secret_lines,
        home_lines,
        lines_scanned,
    }
}

/// The title: the task text (mission: the objective) cleaned, first non-empty line, controls removed, cut.
fn title_of(input: &CopyInput, is_mission: bool, clean: &Clean) -> String {
    let mission_objective = match (is_mission, &input.mission) {
        (true, Some(mission)) => Some(mission.objective.as_str()),
        _ => None,
    };
    let raw = mission_objective
        .or(input.op.as_ref().map(|op| op.task.as_str()))
        .or(input.item.task.as_deref())
        .unwrap_or("");
    let cleaned = clean.apply(raw);
    let first = cleaned.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or("");
    let plain: String = strip_controls(first).chars().take(TITLE_MAX).collect();
    let plain = plain.trim_end();
    if plain.is_empty() { FALLBACK_TITLE.to_string() } else { plain.to_string() }
}

/// Checks of a mission: the last verdict's counts. Its failed names are not recorded, so none are listed.
fn mission_checks(mission: &MissionFacts) -> Option<CheckFacts> {
    (mission.checks_total > 0).then(|| CheckFacts {
        passed: mission.checks_passed,
        total: mission.checks_total,
        failed: Vec::new(),
    })
}

/// Checks of a plain copy: passed/total from the results, the first failed names (at most `FAILED_MAX`).
fn checks_of(results: &[(String, bool)], clean: &Clean) -> Option<CheckFacts> {
    if results.is_empty() {
        return None;
    }
    let passed = results.iter().filter(|(_, ok)| *ok).count();
    let failed = results
        .iter()
        .filter(|(_, ok)| !*ok)
        .take(FAILED_MAX)
        .map(|(name, _)| tidy(name, clean))
        .collect();
    Some(CheckFacts { passed: count(passed), total: count(results.len()), failed })
}

fn cleaned_lines(lines: &[(String, u32)], clean: &Clean) -> Vec<(String, u32)> {
    lines.iter().map(|(path, line)| (tidy(path, clean), *line)).collect()
}

/// `added` -> `A`, `deleted` -> `D`, anything else (`modified`) -> `M`.
pub(crate) fn status_of(raw: &str) -> char {
    match raw {
        "added" => 'A',
        "deleted" => 'D',
        _ => 'M',
    }
}

/// Cleans a string for display and strips control and bidi characters.
pub(crate) fn tidy(raw: &str, clean: &Clean) -> String {
    strip_controls(&clean.apply(raw))
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worktrees::{ChangeSummary, FileChange, WorktreeRef};

    /// Fixture repo and creation time. No personal folder anywhere.
    const REPO: &str = "/work/app";
    const CREATED_MS: u64 = 1_000;

    /// Cleaning that changes nothing.
    fn plain() -> Clean {
        Clean { text: Box::new(|s: &str| s.to_string()) }
    }

    /// Marker cleaning: wraps every cleaned string in brackets so tests can see it ran.
    fn marker() -> Clean {
        Clean { text: Box::new(|s: &str| format!("[{s}]")) }
    }

    fn list_item(handle: &str, state: &str) -> ListItem {
        ListItem {
            entry: WorktreeEntry {
                worktree: WorktreeRef {
                    path: format!("/work/.puppet-master/worktrees/{handle}"),
                    cwd: format!("/work/.puppet-master/worktrees/{handle}"),
                    branch: format!("pm/{handle}"),
                    base: "main".to_string(),
                    base_commit: "abc1234".to_string(),
                    repo: REPO.to_string(),
                    handle: handle.to_string(),
                    created_at_ms: CREATED_MS,
                    closed: None,
                    changes: None,
                    setup: None,
                    pr: None,
                },
                exists: true,
            },
            state: state.to_string(),
            agent_type: Some("fixture-agent".to_string()),
            pane_id: Some("pane-1".to_string()),
            task: Some("Item task text".to_string()),
            summary: None,
        }
    }

    fn file(path: &str, status: &str, additions: u32, deletions: u32) -> FileChange {
        FileChange { path: path.to_string(), status: status.to_string(), additions, deletions, binary: false }
    }

    fn changes_of(files: Vec<FileChange>) -> Changes {
        Changes {
            files_changed: files.len(),
            additions: files.iter().map(|f| f.additions).sum(),
            deletions: files.iter().map(|f| f.deletions).sum(),
            files,
            truncated: false,
        }
    }

    fn op(task: &str, checks: Vec<(&str, bool)>) -> OpFacts {
        OpFacts {
            status: "completed".to_string(),
            task: task.to_string(),
            finished_at_ms: 5_000,
            check_results: checks.into_iter().map(|(name, ok)| (name.to_string(), ok)).collect(),
            account: Some("work".to_string()),
        }
    }

    /// A plain finished copy with one changed file, two passing checks and a clean scan.
    fn copy_input(handle: &str) -> CopyInput {
        CopyInput {
            item: list_item(handle, "unreviewed"),
            changes: Some(changes_of(vec![file("src/cart.ts", "modified", 3, 1)])),
            op: Some(op("Fix the cart total", vec![("unit", true), ("lint", true)])),
            mission: None,
            scan: Some(ScanFacts { secret_lines: Vec::new(), home_lines: Vec::new(), scanned: true }),
        }
    }

    fn keys(items: &[Item]) -> Vec<String> {
        items.iter().map(|item| item.key.as_str().to_string()).collect()
    }

    #[test]
    fn skips_active_missing_and_vanished_copies() {
        let mut gone = copy_input("h-gone");
        gone.item.entry.exists = false;
        let list = vec![
            CopyInput { item: list_item("h-active", "active"), ..copy_input("h-active") },
            CopyInput { item: list_item("h-missing", "missing"), ..copy_input("h-missing") },
            gone,
            copy_input("h-ok"),
        ];
        let items = from_copies(&list, &HashSet::new(), &plain());
        assert_eq!(keys(&items), vec!["copy:h-ok"]);
    }

    #[test]
    fn skips_owned_handles_and_unsafe_handles() {
        let owned: HashSet<String> = ["h-night".to_string()].into_iter().collect();
        let list = vec![copy_input("h-night"), copy_input("../escape"), copy_input("h-plain")];
        let items = from_copies(&list, &owned, &plain());
        assert_eq!(keys(&items), vec!["copy:h-plain"]);
    }

    #[test]
    fn mission_handle_is_a_mission_with_weakened_and_checks() {
        let mut input = copy_input("mission-7");
        input.op = None;
        input.mission = Some(MissionFacts {
            objective: "Ship the cart page".to_string(),
            checks_passed: 3,
            checks_total: 5,
            weakened: vec!["The tests were weakened: 2 tests were skipped in cart.test.ts".to_string()],
        });
        let items = from_copies(&[input], &HashSet::new(), &plain());
        let item = &items[0];
        assert_eq!(item.key.as_str(), "copy:mission-7");
        assert_eq!(item.source, Source::Mission);
        assert_eq!(item.where_label, "Mission");
        assert_eq!(item.title, "Ship the cart page");
        assert_eq!(item.facts.weakened, vec!["The tests were weakened: 2 tests were skipped in cart.test.ts"]);
        assert_eq!(item.facts.checks, Some(CheckFacts { passed: 3, total: 5, failed: Vec::new() }));
    }

    #[test]
    fn mission_without_checks_has_no_check_facts() {
        let mut input = copy_input("mission-8");
        input.mission = Some(MissionFacts {
            objective: "Objective".to_string(),
            checks_passed: 0,
            checks_total: 0,
            weakened: Vec::new(),
        });
        let items = from_copies(&[input], &HashSet::new(), &plain());
        assert_eq!(items[0].facts.checks, None);
    }

    #[test]
    fn plain_copy_checks_come_from_check_results_with_the_failed_name() {
        let mut input = copy_input("h-checks");
        input.op = Some(op("Fix", vec![("unit", true), ("lint", false)]));
        let items = from_copies(&[input], &HashSet::new(), &plain());
        assert_eq!(
            items[0].facts.checks,
            Some(CheckFacts { passed: 1, total: 2, failed: vec!["lint".to_string()] })
        );
    }

    #[test]
    fn plain_copy_without_check_results_has_no_checks_and_no_weakened() {
        let mut input = copy_input("h-none");
        input.op = Some(op("Fix", Vec::new()));
        let items = from_copies(&[input], &HashSet::new(), &plain());
        assert_eq!(items[0].facts.checks, None);
        assert!(items[0].facts.weakened.is_empty());
    }

    #[test]
    fn keep_and_throw_name_the_copy_and_its_base() {
        let items = from_copies(&[copy_input("h-keep")], &HashSet::new(), &plain());
        let item = &items[0];
        assert_eq!(item.source, Source::Copy);
        assert_eq!(item.where_label, "Separate copy of main");
        assert_eq!(item.project, REPO);
        assert_eq!(item.account.as_deref(), Some("work"));
        assert_eq!(item.pane_id.as_deref(), Some("pane-1"));
        assert_eq!(item.fix, Ok("pane-1".to_string()));
        assert_eq!(
            item.keep,
            Keep::Merge { project: REPO.to_string(), handle: "h-keep".to_string(), into: "main".to_string() }
        );
        assert_eq!(item.throw, Throw::Discard { project: REPO.to_string(), handle: "h-keep".to_string() });
        // Placeholders: gather fills these after the sources.
        assert!(item.fingerprint.is_empty());
        assert!(item.risk.reasons.is_empty());
        assert_eq!(item.risk.tier, PLACEHOLDER_TIER);
        assert!(item.group.is_none());
    }

    #[test]
    fn title_is_the_op_task_cleaned_by_the_injected_clean() {
        let items = from_copies(&[copy_input("h-title")], &HashSet::new(), &marker());
        assert_eq!(items[0].title, "[Fix the cart total]");
    }

    #[test]
    fn title_keeps_the_first_line_and_drops_control_and_bidi_characters() {
        let mut input = copy_input("h-lines");
        input.op = Some(op("Fix the\u{202E} cart\nsecond line", Vec::new()));
        let items = from_copies(&[input], &HashSet::new(), &plain());
        assert_eq!(items[0].title, "Fix the cart");
    }

    #[test]
    fn secret_in_the_task_is_masked_before_it_becomes_the_title() {
        let mask = Clean { text: Box::new(|s: &str| s.replace("sk-TEST", "[masked]")) };
        let mut input = copy_input("h-secret");
        input.op = Some(op("Use key sk-TEST for the upload", Vec::new()));
        let items = from_copies(&[input], &HashSet::new(), &mask);
        assert_eq!(items[0].title, "Use key [masked] for the upload");
    }

    #[test]
    fn title_falls_back_to_the_item_task_then_a_placeholder() {
        let mut from_item = copy_input("h-item");
        from_item.op = None;
        let mut blank = copy_input("h-blank");
        blank.op = None;
        blank.item.task = None;
        let items = from_copies(&[from_item, blank], &HashSet::new(), &plain());
        assert_eq!(items[0].title, "Item task text");
        assert_eq!(items[1].title, FALLBACK_TITLE);
    }

    #[test]
    fn title_is_cut_at_title_max() {
        let mut input = copy_input("h-long");
        input.op = Some(op(&"a".repeat(TITLE_MAX + 50), Vec::new()));
        let items = from_copies(&[input], &HashSet::new(), &plain());
        assert_eq!(items[0].title.chars().count(), TITLE_MAX);
    }

    #[test]
    fn finished_time_is_the_op_finish_else_the_copy_creation() {
        let mut no_op = copy_input("h-created");
        no_op.op = None;
        let items = from_copies(&[copy_input("h-done"), no_op], &HashSet::new(), &plain());
        assert_eq!(items[0].finished_at_ms, 5_000);
        assert_eq!(items[1].finished_at_ms, CREATED_MS);
    }

    #[test]
    fn file_rows_and_totals_come_from_the_changes() {
        let mut input = copy_input("h-files");
        input.changes = Some(changes_of(vec![
            file("src/new.ts", "added", 10, 0),
            file("src/cart.ts", "modified", 3, 1),
            file("src/old.ts", "deleted", 0, 7),
        ]));
        let items = from_copies(&[input], &HashSet::new(), &plain());
        let facts = &items[0].facts;
        assert_eq!(facts.files_total, 3);
        assert_eq!(facts.added, 13);
        assert_eq!(facts.deleted, 8);
        let rows = facts.files.as_ref().expect("file rows");
        let statuses: Vec<(String, char)> = rows.iter().map(|row| (row.path.clone(), row.status)).collect();
        assert_eq!(
            statuses,
            vec![
                ("src/new.ts".to_string(), 'A'),
                ("src/cart.ts".to_string(), 'M'),
                ("src/old.ts".to_string(), 'D'),
            ]
        );
        assert!(facts.lines_scanned);
    }

    #[test]
    fn secret_and_ci_files_are_named_from_the_changed_paths() {
        let mut input = copy_input("h-sensitive");
        input.changes = Some(changes_of(vec![
            file(".env", "added", 1, 0),
            file(".github/workflows/ci.yml", "modified", 2, 2),
            file("src/cart.ts", "modified", 1, 1),
        ]));
        let items = from_copies(&[input], &HashSet::new(), &plain());
        assert_eq!(items[0].facts.secret_files, vec![".env".to_string()]);
        assert_eq!(items[0].facts.ci_files, vec![".github/workflows/ci.yml".to_string()]);
    }

    #[test]
    fn missing_changes_keep_the_unknown_files_and_use_the_live_summary() {
        let mut input = copy_input("h-summary");
        input.changes = None;
        input.item.summary = Some(ChangeSummary { files_changed: 25, additions: 500, deletions: 3 });
        let items = from_copies(&[input], &HashSet::new(), &plain());
        let facts = &items[0].facts;
        assert!(facts.files.is_none(), "no file list means Unknown later");
        assert_eq!((facts.files_total, facts.added, facts.deleted), (25, 500, 3));
    }

    #[test]
    fn scan_lines_are_cleaned_and_a_missing_scan_is_not_scanned() {
        let mut scanned = copy_input("h-scan");
        scanned.scan = Some(ScanFacts {
            secret_lines: vec![("src/config.ts".to_string(), 12)],
            home_lines: vec![("notes.md".to_string(), 3)],
            scanned: true,
        });
        let mut unscanned = copy_input("h-noscan");
        unscanned.scan = None;
        let items = from_copies(&[scanned, unscanned], &HashSet::new(), &marker());
        assert_eq!(items[0].facts.secret_lines, vec![("[src/config.ts]".to_string(), 12)]);
        assert_eq!(items[0].facts.home_lines, vec![("[notes.md]".to_string(), 3)]);
        assert!(items[0].facts.lines_scanned);
        assert!(items[1].facts.secret_lines.is_empty());
        assert!(!items[1].facts.lines_scanned);
    }
}
