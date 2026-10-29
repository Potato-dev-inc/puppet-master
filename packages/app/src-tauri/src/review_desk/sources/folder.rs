//! Review Desk: in-folder turns become desk items (SPEC n33 section 3.4 D). No I/O of its own: it
//! reads only what the caller passes. The one exception is `workspace_covers` (rule 3 of Throw
//! away), which canonicalizes two folder paths as the spec names it.
//!
//! One item per pane: its finished turns that changed files in the agent's own folder. A turn
//! counts when it is idle, changed at least one file, ended after the pane's "looked at" mark and
//! inside `WINDOW_MS`. Turns that ran in a separate copy are copy items and are skipped here.

use std::path::PathBuf;

use super::super::model::{
    strip_controls, CheckFacts, Facts, FileRow, Item, ItemKey, Keep, Level, Marks, Risk, Source, Throw, TITLE_MAX,
    WINDOW_MS,
};
use crate::project_path::workspace_covers;
use crate::pty::status::PaneStatus;
use crate::turn_summary::model::{ReceiptFacts, TurnSummary};

/// A pane as the desk needs it. The live layer fills it from the registry and passes a redacted
/// `title`; this module only removes control characters and cuts it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneFacts {
    pub id: String,
    /// The folder the pane runs in.
    pub cwd: PathBuf,
    pub title: String,
    pub status: PaneStatus,
    /// The pane that started this one (the helper link), if any.
    pub parent: Option<String>,
    /// The pane is still open in the app.
    pub alive: bool,
}

/// One item per pane that has in-folder work waiting for a look.
///
/// `turns` pairs each turn with the separate copy folder it ran in (`None` = the pane's own folder);
/// copy turns are skipped. `checkpoints(pane, first_started_at_ms)` gives the save point taken just
/// before that turn, if there is one. The item's `risk` and `fingerprint` are placeholders; `gather`
/// fills them, and `group` is set there too.
pub fn from_turns(
    turns: &[(TurnSummary, Option<PathBuf>)],
    panes: &[PaneFacts],
    marks: &Marks,
    checkpoints: &dyn Fn(&str, u64) -> Option<String>,
    now_ms: u64,
) -> Vec<Item> {
    let floor = now_ms.saturating_sub(WINDOW_MS);
    panes
        .iter()
        .filter_map(|pane| {
            let watermark = marks.get(&pane.id).map_or(0, |mark| mark.through_ms);
            let waiting: Vec<&TurnSummary> = turns
                .iter()
                .filter(|(_, copy)| copy.is_none())
                .map(|(turn, _)| turn)
                .filter(|turn| turn.pane_id == pane.id)
                .filter(|turn| turn.ended_at_ms > watermark && turn.ended_at_ms >= floor)
                .filter(|turn| turn.end.kind == "idle")
                .filter(|turn| turn.files.as_ref().is_some_and(|files| files.count > 0))
                .collect();
            item_of(pane, &waiting, turns, panes, checkpoints)
        })
        .collect()
}

/// The item for one pane's waiting turns (`None` when there are none or the key is not valid).
fn item_of(
    pane: &PaneFacts,
    waiting: &[&TurnSummary],
    turns: &[(TurnSummary, Option<PathBuf>)],
    panes: &[PaneFacts],
    checkpoints: &dyn Fn(&str, u64) -> Option<String>,
) -> Option<Item> {
    let first = *waiting.iter().min_by_key(|turn| (turn.started_at_ms, turn.turn))?;
    let last = *waiting.iter().max_by_key(|turn| (turn.ended_at_ms, turn.turn))?;
    let lo = waiting.iter().map(|turn| turn.turn).min()?;
    let hi = waiting.iter().map(|turn| turn.turn).max()?;
    let key = ItemKey::parse(&format!("folder:{}:{lo}-{hi}", pane.id))?;

    // Files: the union of the summaries' paths (each summary keeps at most a few), counts summed.
    // The live layer replaces these rows with the exact checkpoint changes when it has them.
    let mut rows: Vec<FileRow> = Vec::new();
    let (mut count, mut added, mut deleted) = (0u32, 0u32, 0u32);
    for turn in waiting {
        if let Some(stats) = &turn.files {
            count = count.saturating_add(stats.count);
            added = added.saturating_add(stats.added);
            deleted = deleted.saturating_add(stats.deleted);
            for path in &stats.paths {
                if !rows.iter().any(|row| &row.path == path) {
                    rows.push(FileRow { path: path.clone(), status: 'M', added: 0, deleted: 0, binary: false });
                }
            }
        }
    }
    let facts = Facts {
        files: Some(rows),
        files_total: count,
        added,
        deleted,
        checks: checks_of(last.receipt.as_ref()),
        ..Facts::default()
    };

    let alive = pane.alive;
    Some(Item {
        key,
        source: Source::Folder,
        fingerprint: String::new(),
        // The summary sentence was redacted when the turn was recorded.
        title: plain_line(&last.sentence, TITLE_MAX),
        agent_label: last.agent.clone(),
        account: last.account.clone(),
        where_label: "In your folder".to_string(),
        project: pane.cwd.to_string_lossy().into_owned(),
        pane_id: alive.then(|| pane.id.clone()),
        finished_at_ms: last.ended_at_ms,
        turns: waiting.len() as u32,
        group: None,
        facts,
        risk: Risk { tier: 7, level: Level::Low, reasons: Vec::new() },
        keep: Keep::Mark { pane_id: pane.id.clone(), through_ms: last.ended_at_ms },
        throw: throw_of(pane, first, count, turns, panes, checkpoints),
        fix: if alive { Ok(pane.id.clone()) } else { Err("The agent's pane is closed.".to_string()) },
    })
}

/// Throw away for a folder item: a restore only when all three rules hold; the first rule that
/// fails gives its plain reason.
fn throw_of(
    pane: &PaneFacts,
    first: &TurnSummary,
    files: u32,
    turns: &[(TurnSummary, Option<PathBuf>)],
    panes: &[PaneFacts],
    checkpoints: &dyn Fn(&str, u64) -> Option<String>,
) -> Throw {
    let no = |why: &str| Throw::No { why: why.to_string() };
    // Rule 1: the pane is open and not running.
    if !pane.alive {
        return no("The agent's pane is closed. Use History to undo.");
    }
    if pane.status == PaneStatus::Running {
        return no("The agent is still working.");
    }
    // Rule 2: a save point exists just before the first waiting turn.
    let Some(checkpoint_id) = checkpoints(&pane.id, first.started_at_ms) else {
        return no("No save point before this work. Use History to undo.");
    };
    // Rule 3: no other pane sharing this folder changed files after this work started.
    if changed_since(pane, first.started_at_ms, turns, panes) {
        return no("Another agent changed this folder after this. Use History to undo.");
    }
    Throw::Restore { pane_id: pane.id.clone(), checkpoint_id, files }
}

/// Rule 3 of Throw away: another pane whose folder covers this one (or is covered by it) has a
/// turn in its own folder that changed files and ended after `since`.
fn changed_since(
    pane: &PaneFacts,
    since: u64,
    turns: &[(TurnSummary, Option<PathBuf>)],
    panes: &[PaneFacts],
) -> bool {
    panes.iter().filter(|other| other.id != pane.id).any(|other| {
        let shared = workspace_covers(&pane.cwd, &other.cwd) || workspace_covers(&other.cwd, &pane.cwd);
        shared
            && turns.iter().any(|(turn, copy)| {
                copy.is_none()
                    && turn.pane_id == other.id
                    && turn.ended_at_ms > since
                    && turn.files.as_ref().is_some_and(|files| files.count > 0)
            })
    })
}

/// `"3/5 passed"` gives 3 of 5. `none configured`, or any other text, gives no checks.
fn checks_of(receipt: Option<&ReceiptFacts>) -> Option<CheckFacts> {
    let text = receipt?.checks.trim();
    let (passed, total) = text.strip_suffix(" passed")?.split_once('/')?;
    let passed: u32 = passed.trim().parse().ok()?;
    let total: u32 = total.trim().parse().ok()?;
    (passed <= total).then(|| CheckFacts { passed, total, failed: Vec::new() })
}

/// The first line of `raw` without control or bidi characters, trimmed, cut to `max` characters
/// (not bytes). Shared with `group.rs` for the group label.
pub fn plain_line(raw: &str, max: usize) -> String {
    let first = raw.lines().next().unwrap_or("");
    strip_controls(first).trim().chars().take(max).collect::<String>().trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review_desk::model::How;
    use crate::turn_summary::model::{EndState, FileStats};

    const DAY: u64 = 24 * 3_600_000;
    const NOW: u64 = 100 * DAY;
    const T0: u64 = NOW - 3_600_000;

    /// A finished idle turn in `pane` that changed `count` files (`paths` are the names it keeps).
    fn turn(pane: &str, n: u32, started: u64, ended: u64, paths: &[&str], count: u32) -> TurnSummary {
        TurnSummary {
            pane_id: pane.to_string(),
            agent: "Fixture agent".to_string(),
            agent_id: None,
            account: None,
            turn: n,
            started_at_ms: started,
            ended_at_ms: ended,
            duration_ms: ended.saturating_sub(started),
            files: Some(FileStats {
                count,
                added: 2 * count,
                deleted: 1,
                paths: paths.iter().map(|path| path.to_string()).collect(),
            }),
            approvals: Vec::new(),
            end: EndState { kind: "idle".to_string(), prompt: None },
            receipt: None,
            sentence: format!("Turn {n} changed {count} files"),
        }
    }

    fn pane(id: &str, cwd: &str, alive: bool) -> PaneFacts {
        PaneFacts {
            id: id.to_string(),
            cwd: PathBuf::from(cwd),
            title: format!("Title {id}"),
            status: PaneStatus::Idle,
            parent: None,
            alive,
        }
    }

    /// A save point exists before every turn: its id names the pane and the first start time.
    fn saved(pane: &str, first: u64) -> Option<String> {
        Some(format!("cp-{pane}-{first}"))
    }

    fn only(items: &[Item], prefix: &str) -> Item {
        items.iter().find(|item| item.key.as_str().starts_with(prefix)).cloned().expect("an item for the pane")
    }

    const APP: &str = r"C:\Users\you\app";
    const SUB: &str = r"C:\Users\you\app\sub";
    const OTHER: &str = r"C:\Users\you\other";

    #[test]
    fn one_item_per_pane_sums_its_waiting_turns() {
        let turns = vec![
            (turn("pA", 1, T0, T0 + 10, &["src/a.ts"], 1), None),
            (turn("pA", 2, T0 + 20, T0 + 30, &["src/a.ts", "src/b.ts"], 2), None),
        ];
        let panes = vec![pane("pA", APP, true)];
        let items = from_turns(&turns, &panes, &Marks::empty(), &saved, NOW);
        assert_eq!(items.len(), 1);
        let item = &items[0];
        assert_eq!(item.key.as_str(), "folder:pA:1-2");
        assert_eq!(item.source, Source::Folder);
        assert_eq!(item.turns, 2);
        assert_eq!(item.facts.files_total, 3);
        assert_eq!((item.facts.added, item.facts.deleted), (6, 2));
        let paths: Vec<&str> = item.facts.files.as_ref().unwrap().iter().map(|row| row.path.as_str()).collect();
        assert_eq!(paths, ["src/a.ts", "src/b.ts"], "paths are a union, first seen first");
        assert_eq!(item.title, "Turn 2 changed 2 files", "the title is the last turn's sentence");
        assert_eq!(item.where_label, "In your folder");
        assert_eq!(item.agent_label, "Fixture agent");
        assert_eq!(item.finished_at_ms, T0 + 30);
        assert_eq!(item.pane_id.as_deref(), Some("pA"));
        assert_eq!(item.fix, Ok("pA".to_string()));
        assert_eq!(item.keep, Keep::Mark { pane_id: "pA".to_string(), through_ms: T0 + 30 });
        assert!(item.group.is_none(), "groups are set by gather");
    }

    #[test]
    fn marks_watermark_hides_older_turns_per_pane() {
        let turns = vec![
            (turn("pA", 1, T0, T0 + 10, &["a.ts"], 1), None),
            (turn("pA", 2, T0 + 20, T0 + 30, &["b.ts"], 1), None),
            (turn("pA", 3, T0 + 40, T0 + 50, &["c.ts"], 1), None),
            (turn("pB", 1, T0, T0 + 5, &["x.ts"], 1), None),
        ];
        let panes = vec![pane("pA", APP, true), pane("pB", OTHER, true)];
        let mut marks = Marks::empty();
        marks.set("pA", T0 + 30, How::Kept, NOW);

        let items = from_turns(&turns, &panes, &marks, &saved, NOW);
        let keys: Vec<&str> = items.iter().map(|item| item.key.as_str()).collect();
        assert_eq!(keys, ["folder:pA:3-3", "folder:pB:1-1"], "pane A keeps only the turn after its mark");
        assert_eq!(items[0].turns, 1);
        assert_eq!(items[0].facts.files_total, 1);

        marks.set("pB", T0 + 5, How::Kept, NOW);
        let keys: Vec<String> =
            from_turns(&turns, &panes, &marks, &saved, NOW).iter().map(|item| item.key.as_str().to_string()).collect();
        assert_eq!(keys, ["folder:pA:3-3"], "a pane with every turn marked has no item");
    }

    #[test]
    fn zero_file_and_waiting_turns_are_skipped() {
        let mut waiting = turn("pA", 2, T0 + 20, T0 + 30, &["w.ts"], 1);
        waiting.end.kind = "waiting_input".to_string();
        let mut errored = turn("pA", 3, T0 + 40, T0 + 50, &["e.ts"], 1);
        errored.end.kind = "error".to_string();
        let mut unknown = turn("pA", 4, T0 + 60, T0 + 70, &["u.ts"], 1);
        unknown.files = None;
        let mut turns = vec![
            (turn("pA", 1, T0, T0 + 10, &[], 0), None),
            (waiting, None),
            (errored, None),
            (unknown, None),
        ];
        let panes = vec![pane("pA", APP, true)];
        assert!(from_turns(&turns, &panes, &Marks::empty(), &saved, NOW).is_empty());

        turns.push((turn("pA", 5, T0 + 80, T0 + 90, &["ok.ts"], 1), None));
        let items = from_turns(&turns, &panes, &Marks::empty(), &saved, NOW);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].key.as_str(), "folder:pA:5-5", "only the idle turn with files is listed");
        assert_eq!(items[0].turns, 1);
    }

    #[test]
    fn turns_older_than_the_window_are_skipped_and_the_edge_is_kept() {
        let floor = NOW - WINDOW_MS;
        let turns = vec![
            (turn("pA", 1, floor - 20, floor - 1, &["old.ts"], 1), None),
            (turn("pA", 2, floor - 10, floor, &["edge.ts"], 1), None),
        ];
        let items = from_turns(&turns, &[pane("pA", APP, true)], &Marks::empty(), &saved, NOW);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].key.as_str(), "folder:pA:2-2", "ended exactly at the floor counts, one ms before does not");
    }

    #[test]
    fn a_pane_inside_a_separate_copy_is_skipped() {
        let copy = Some(PathBuf::from(r"C:\Users\you\app-copy\worktree"));
        let turns = vec![(turn("pA", 1, T0, T0 + 10, &["a.ts"], 1), copy)];
        assert!(from_turns(&turns, &[pane("pA", APP, true)], &Marks::empty(), &saved, NOW).is_empty());
    }

    #[test]
    fn a_pane_id_that_is_not_a_valid_token_gets_no_item() {
        let turns = vec![(turn("bad:id", 1, T0, T0 + 10, &["a.ts"], 1), None)];
        assert!(from_turns(&turns, &[pane("bad:id", APP, true)], &Marks::empty(), &saved, NOW).is_empty());
    }

    #[test]
    fn a_closed_pane_has_no_pane_id_and_says_so() {
        let turns = vec![(turn("pA", 1, T0, T0 + 10, &["a.ts"], 1), None)];
        let items = from_turns(&turns, &[pane("pA", APP, false)], &Marks::empty(), &saved, NOW);
        let item = &items[0];
        assert_eq!(item.pane_id, None);
        assert_eq!(item.fix, Err("The agent's pane is closed.".to_string()));
        assert_eq!(item.throw, Throw::No { why: "The agent's pane is closed. Use History to undo.".to_string() });
    }

    #[test]
    fn throw_restores_when_every_rule_holds() {
        let turns = vec![
            (turn("pA", 1, T0, T0 + 10, &["a.ts"], 1), None),
            // Another folder: it does not touch this one, even though it changed after.
            (turn("pB", 1, T0 + 20, T0 + 30, &["b.ts"], 1), None),
        ];
        let panes = vec![pane("pA", APP, true), pane("pB", OTHER, true)];
        let items = from_turns(&turns, &panes, &Marks::empty(), &saved, NOW);
        let item = only(&items, "folder:pA:");
        assert_eq!(
            item.throw,
            Throw::Restore { pane_id: "pA".to_string(), checkpoint_id: format!("cp-pA-{T0}"), files: 1 }
        );
    }

    #[test]
    fn throw_says_the_agent_is_still_working() {
        let turns = vec![(turn("pA", 1, T0, T0 + 10, &["a.ts"], 1), None)];
        let mut running = pane("pA", APP, true);
        running.status = PaneStatus::Running;
        let items = from_turns(&turns, &[running], &Marks::empty(), &saved, NOW);
        assert_eq!(items[0].throw, Throw::No { why: "The agent is still working.".to_string() });
    }

    #[test]
    fn throw_says_when_there_is_no_save_point() {
        let turns = vec![(turn("pA", 1, T0, T0 + 10, &["a.ts"], 1), None)];
        let no_save = |_: &str, _: u64| -> Option<String> { None };
        let items = from_turns(&turns, &[pane("pA", APP, true)], &Marks::empty(), &no_save, NOW);
        assert_eq!(
            items[0].throw,
            Throw::No { why: "No save point before this work. Use History to undo.".to_string() }
        );
    }

    #[test]
    fn throw_says_when_another_agent_changed_the_folder_after() {
        let panes = vec![pane("pA", APP, true), pane("pB", SUB, true)];
        // Pane B worked inside the same folder after pane A's turn started: refuse.
        let after = vec![
            (turn("pA", 1, T0, T0 + 10, &["a.ts"], 1), None),
            (turn("pB", 1, T0 + 5, T0 + 20, &["b.ts"], 1), None),
        ];
        let items = from_turns(&after, &panes, &Marks::empty(), &saved, NOW);
        assert_eq!(
            only(&items, "folder:pA:").throw,
            Throw::No { why: "Another agent changed this folder after this. Use History to undo.".to_string() }
        );

        // The same change, but it ended before pane A's work started: allowed.
        let before = vec![
            (turn("pA", 1, T0, T0 + 10, &["a.ts"], 1), None),
            (turn("pB", 1, T0 - 100, T0 - 50, &["b.ts"], 1), None),
        ];
        let items = from_turns(&before, &panes, &Marks::empty(), &saved, NOW);
        assert!(matches!(only(&items, "folder:pA:").throw, Throw::Restore { .. }));

        // A change in pane B with no files does not count.
        let no_files = vec![
            (turn("pA", 1, T0, T0 + 10, &["a.ts"], 1), None),
            (turn("pB", 1, T0 + 5, T0 + 20, &[], 0), None),
        ];
        let items = from_turns(&no_files, &panes, &Marks::empty(), &saved, NOW);
        assert!(matches!(only(&items, "folder:pA:").throw, Throw::Restore { .. }));
    }

    #[test]
    fn a_closed_pane_is_refused_before_the_other_rules() {
        let turns = vec![(turn("pA", 1, T0, T0 + 10, &["a.ts"], 1), None)];
        let no_save = |_: &str, _: u64| -> Option<String> { None };
        let items = from_turns(&turns, &[pane("pA", APP, false)], &Marks::empty(), &no_save, NOW);
        assert_eq!(
            items[0].throw,
            Throw::No { why: "The agent's pane is closed. Use History to undo.".to_string() }
        );
    }

    #[test]
    fn the_save_point_is_asked_for_the_first_waiting_turn() {
        // Turn 1 was looked at; the save point is only there for turn 2's start.
        let turns = vec![
            (turn("pA", 1, T0, T0 + 10, &["a.ts"], 1), None),
            (turn("pA", 2, T0 + 20, T0 + 30, &["b.ts"], 1), None),
        ];
        let mut marks = Marks::empty();
        marks.set("pA", T0 + 10, How::Kept, NOW);
        let only_second = |pane: &str, first: u64| (first == T0 + 20).then(|| format!("cp-{pane}"));
        let items = from_turns(&turns, &[pane("pA", APP, true)], &marks, &only_second, NOW);
        assert_eq!(
            items[0].throw,
            Throw::Restore { pane_id: "pA".to_string(), checkpoint_id: "cp-pA".to_string(), files: 1 }
        );
    }

    #[test]
    fn checks_come_from_the_last_receipt_only_when_counted() {
        let receipt = |checks: &str| ReceiptFacts { checks: checks.to_string(), outcome: "passed_checks".to_string() };
        assert_eq!(
            checks_of(Some(&receipt("3/5 passed"))),
            Some(CheckFacts { passed: 3, total: 5, failed: Vec::new() })
        );
        assert_eq!(checks_of(Some(&receipt("none configured"))), None);
        assert_eq!(checks_of(Some(&receipt("7/5 passed"))), None, "more passed than total is not a count");
        assert_eq!(checks_of(Some(&receipt("x/5 passed"))), None);
        assert_eq!(checks_of(None), None);

        let mut with_checks = turn("pA", 1, T0, T0 + 10, &["a.ts"], 1);
        with_checks.receipt = Some(receipt("4/4 passed"));
        let items = from_turns(&[(with_checks, None)], &[pane("pA", APP, true)], &Marks::empty(), &saved, NOW);
        assert_eq!(items[0].facts.checks, Some(CheckFacts { passed: 4, total: 4, failed: Vec::new() }));
    }

    #[test]
    fn title_is_the_first_line_without_controls_and_cut() {
        assert_eq!(plain_line("first\u{202E}\nsecond", 120), "first");
        assert_eq!(plain_line("  spaced  ", 120), "spaced");
        assert_eq!(plain_line("a\u{7}b", 10), "ab");
        assert_eq!(plain_line("", 10), "");
        let long = "\u{e9}".repeat(200);
        assert_eq!(plain_line(&long, TITLE_MAX).chars().count(), TITLE_MAX, "cut by characters, not bytes");
    }
}
