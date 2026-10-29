//! Review Desk: Night Shift converter (SPEC n33 section 3.4 A). Pure: a run and the injected `Clean` in,
//! items out. No I/O, no git, no model. The live layer fills what needs the disk: the changed files
//! (`facts.files`), the secret and home-folder scans, the keep target branch, `risk` and `fingerprint`.

use std::collections::HashSet;

use crate::night_shift::model::{Class, CopyRef, NightRun, Outcome, Review, Task};
use crate::review_desk::model::{
    strip_controls, CheckFacts, Clean, Facts, Item, ItemKey, Keep, Level, Risk, Source, Throw, TITLE_MAX,
};

/// The Keep target until the live layer reads the project's branch. `keep_target` reads git, so it is not
/// called here.
const CURRENT_BRANCH: &str = "your current branch";
/// Used when the copy has no pane (the desk cannot open one).
const PANE_CLOSED: &str = "The agent's pane is closed.";

/// Every handle this run owns: each try's handle (any review state) and each copy's handle. The copies
/// converter leaves these out, so Night Shift results are listed once.
fn owned_handles(run: &NightRun) -> HashSet<String> {
    let mut owned = HashSet::new();
    for task in &run.tasks {
        for attempt in &task.attempts {
            if let Some(handle) = &attempt.handle {
                owned.insert(handle.clone());
            }
        }
        if let Some(copy) = task.outcome.as_ref().and_then(|outcome| outcome.copy.as_ref()) {
            owned.insert(copy.handle.clone());
        }
    }
    owned
}

/// Night Shift items (SPEC 3.4 A): tasks still `Pending` review, with a copy, in class `Ready`,
/// `NoChecks` or `NeedsLook`. Returns the items and the owned handles.
pub fn from_night(run: &NightRun, ctx: &Clean) -> (Vec<Item>, HashSet<String>) {
    let owned = owned_handles(run);
    let mut items = Vec::new();
    for task in &run.tasks {
        let Some(outcome) = &task.outcome else { continue };
        let Some(copy) = &outcome.copy else { continue };
        let listed = matches!(task.review, Review::Pending)
            && matches!(outcome.class, Class::Ready | Class::NoChecks | Class::NeedsLook);
        if !listed {
            continue;
        }
        let Some(key) = ItemKey::parse(&format!("night:{}", task.spec.id)) else { continue };
        items.push(item_of(run, task, outcome, copy, key, ctx));
    }
    (items, owned)
}

fn item_of(run: &NightRun, task: &Task, outcome: &Outcome, copy: &CopyRef, key: ItemKey, ctx: &Clean) -> Item {
    // The try that holds the result is the one whose handle is the copy's; else the last try.
    let holder = task
        .attempts
        .iter()
        .rev()
        .find(|attempt| attempt.handle.as_deref() == Some(copy.handle.as_str()))
        .or_else(|| task.attempts.last());
    let finished_at_ms = holder.and_then(|attempt| attempt.ended_ms).or(run.finished_ms).unwrap_or(run.created_ms);
    Item {
        key,
        source: Source::Night,
        fingerprint: String::new(),
        title: title_of(ctx, &task.spec.text),
        agent_label: holder.map(|attempt| attempt.label.clone()).unwrap_or_default(),
        account: holder.and_then(|attempt| attempt.plan.account.clone()),
        where_label: "Night Shift".to_string(),
        project: task.spec.project.clone(),
        pane_id: copy.pane_id.clone(),
        finished_at_ms,
        turns: outcome.turns,
        group: None,
        facts: facts_of(outcome, ctx),
        // Placeholders: `gather` computes the real risk and fingerprint from the facts.
        risk: Risk { tier: 7, level: Level::Low, reasons: Vec::new() },
        keep: Keep::NightKeep { task_id: task.spec.id.clone(), into: CURRENT_BRANCH.to_string() },
        throw: Throw::NightThrow { task_id: task.spec.id.clone() },
        fix: copy.pane_id.clone().ok_or_else(|| PANE_CLOSED.to_string()),
    }
}

/// The facts the outcome records. Checks count only when the task declared checks.
fn facts_of(outcome: &Outcome, ctx: &Clean) -> Facts {
    let (files_total, added, deleted) = outcome.files.unwrap_or((0, 0, 0));
    let checks = if outcome.checks_declared {
        outcome.tests.map(|tests| CheckFacts { passed: tests.passed, total: tests.total, failed: Vec::new() })
    } else {
        None
    };
    Facts {
        files: None,
        files_total,
        added,
        deleted,
        weakened: outcome
            .test_change_lines
            .iter()
            .map(|line| first_line(ctx, line))
            .filter(|line| !line.is_empty())
            .collect(),
        checks,
        secret_files: Vec::new(),
        ci_files: Vec::new(),
        secret_lines: Vec::new(),
        home_lines: Vec::new(),
        lines_scanned: false,
    }
}

/// The first non-blank line of a text, cleaned by the injected hook and with control characters removed.
fn first_line(ctx: &Clean, raw: &str) -> String {
    let cleaned = ctx.apply(raw);
    let line = cleaned.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or("");
    strip_controls(line).trim().to_string()
}

/// The task text as a title: first line, cleaned, at most `TITLE_MAX` characters.
fn title_of(ctx: &Clean, raw: &str) -> String {
    first_line(ctx, raw).chars().take(TITLE_MAX).collect::<String>().trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::night_shift::model::Attempt;
    use crate::night_shift::view::morning_fixture;

    /// A stand-in for the live redaction: wraps the text so tests can see the hook ran.
    fn marker() -> Clean {
        Clean { text: Box::new(|s| format!("[{s}]")) }
    }

    fn keys(items: &[Item]) -> Vec<&str> {
        items.iter().map(|item| item.key.as_str()).collect()
    }

    fn by_key<'a>(items: &'a [Item], key: &str) -> &'a Item {
        items.iter().find(|item| item.key.as_str() == key).expect("item listed")
    }

    #[test]
    fn lists_only_pending_copies_in_the_three_classes() {
        let mut run = morning_fixture();
        // The fixture has a Ready task (1), a NeedsLook task (2) and a DidntRun task with no copy (3).
        let mut no_checks = run.tasks[0].clone();
        no_checks.spec.id = "nt-00000004".into();
        no_checks.outcome.as_mut().unwrap().class = Class::NoChecks;
        run.tasks.push(no_checks);
        let mut no_changes = run.tasks[0].clone();
        no_changes.spec.id = "nt-00000005".into();
        no_changes.outcome.as_mut().unwrap().class = Class::NoChanges;
        run.tasks.push(no_changes);
        for (id, review) in [
            ("nt-00000006", Review::Kept { at_ms: 1, into: "main".into() }),
            ("nt-00000007", Review::Discarded { at_ms: 1 }),
            ("nt-00000008", Review::KeepFailed { at_ms: 1, sentence: "Merge failed.".into() }),
        ] {
            let mut task = run.tasks[0].clone();
            task.spec.id = id.into();
            task.review = review;
            run.tasks.push(task);
        }
        let (items, _) = from_night(&run, &marker());
        assert_eq!(keys(&items), vec!["night:nt-00000001", "night:nt-00000002", "night:nt-00000004"]);
    }

    #[test]
    fn owned_handles_include_earlier_tries_and_copies_in_any_review_state() {
        let mut run = morning_fixture();
        let mut earlier = run.tasks[0].attempts[0].clone();
        earlier.handle = Some("run-0".into());
        run.tasks[0].attempts.insert(0, earlier);
        // A try on the task that never got a copy.
        let mut skipped_try = run.tasks[0].attempts[0].clone();
        skipped_try.handle = Some("run-3".into());
        run.tasks[2].attempts.push(skipped_try);
        // Already discarded: still owns its handles.
        run.tasks[0].review = Review::Discarded { at_ms: 5 };

        let (items, owned) = from_night(&run, &marker());
        let expected: HashSet<String> = ["run-0", "run-1", "run-3"].into_iter().map(String::from).collect();
        assert_eq!(owned, expected);
        assert_eq!(keys(&items), vec!["night:nt-00000002"]);
    }

    #[test]
    fn keep_and_throw_name_the_task() {
        let (items, _) = from_night(&morning_fixture(), &marker());
        let ready = by_key(&items, "night:nt-00000001");
        assert_eq!(
            ready.keep,
            Keep::NightKeep { task_id: "nt-00000001".into(), into: "your current branch".into() }
        );
        assert_eq!(ready.throw, Throw::NightThrow { task_id: "nt-00000001".into() });
    }

    #[test]
    fn source_where_and_agent_come_from_the_run() {
        let (items, _) = from_night(&morning_fixture(), &marker());
        let ready = by_key(&items, "night:nt-00000001");
        assert_eq!(ready.source, Source::Night);
        assert_eq!(ready.where_label, "Night Shift");
        assert_eq!(ready.agent_label, "Codex");
        assert_eq!(ready.account, None);
        assert_eq!(ready.project, "C:\\Users\\you\\work\\app");
        assert_eq!(ready.turns, 3);
        assert!(ready.fingerprint.is_empty(), "gather fills the fingerprint");
    }

    #[test]
    fn title_is_the_first_line_through_the_hook() {
        let (items, _) = from_night(&morning_fixture(), &marker());
        // The hook sees the whole text first (the marker wraps it), then the first line is taken.
        assert_eq!(by_key(&items, "night:nt-00000002").title, "[Rename the config loader.");
        assert_eq!(by_key(&items, "night:nt-00000001").title, "[Fix the cart total rounding.]");
    }

    #[test]
    fn title_drops_control_and_bidi_characters_and_is_capped() {
        let mut run = morning_fixture();
        run.tasks[0].spec.text = "Fix\u{202E} the\tcart".into();
        let (items, _) = from_night(&run, &marker());
        assert_eq!(by_key(&items, "night:nt-00000001").title, "[Fix thecart]");

        run.tasks[0].spec.text = "x".repeat(300);
        let (items, _) = from_night(&run, &marker());
        assert_eq!(by_key(&items, "night:nt-00000001").title.chars().count(), TITLE_MAX);
    }

    #[test]
    fn facts_come_from_the_outcome_and_the_hook() {
        let (items, _) = from_night(&morning_fixture(), &marker());
        let ready = by_key(&items, "night:nt-00000001");
        assert_eq!(ready.facts.checks, Some(CheckFacts { passed: 42, total: 42, failed: Vec::new() }));
        assert_eq!((ready.facts.files_total, ready.facts.added, ready.facts.deleted), (5, 120, 34));
        assert!(ready.facts.weakened.is_empty());
        assert!(ready.facts.files.is_none(), "the live layer fills the changed files");
        assert!(!ready.facts.lines_scanned);

        let needs = by_key(&items, "night:nt-00000002");
        assert_eq!(needs.facts.weakened, vec!["[Changed tests/cart.test.ts]".to_string()]);
        assert_eq!(needs.facts.checks, Some(CheckFacts { passed: 9, total: 12, failed: Vec::new() }));
    }

    #[test]
    fn checks_count_only_when_declared() {
        let mut run = morning_fixture();
        run.tasks[0].outcome.as_mut().unwrap().checks_declared = false;
        let (items, _) = from_night(&run, &marker());
        assert_eq!(by_key(&items, "night:nt-00000001").facts.checks, None);
    }

    #[test]
    fn finished_time_is_the_holding_try_then_the_run() {
        let mut run = morning_fixture();
        run.tasks[0].attempts[0].ended_ms = Some(9_000_000);
        let (items, _) = from_night(&run, &marker());
        assert_eq!(by_key(&items, "night:nt-00000001").finished_at_ms, 9_000_000);

        run.tasks[0].attempts[0].ended_ms = None;
        let (items, _) = from_night(&run, &marker());
        assert_eq!(by_key(&items, "night:nt-00000001").finished_at_ms, 7_200_000);
    }

    #[test]
    fn fix_uses_the_copy_pane_or_says_it_is_closed() {
        let mut run = morning_fixture();
        let (items, _) = from_night(&run, &marker());
        let ready = by_key(&items, "night:nt-00000001");
        assert_eq!(ready.pane_id.as_deref(), Some("pane-42"));
        assert_eq!(ready.fix, Ok("pane-42".to_string()));

        run.tasks[0].outcome.as_mut().unwrap().copy.as_mut().unwrap().pane_id = None;
        let (items, _) = from_night(&run, &marker());
        assert_eq!(by_key(&items, "night:nt-00000001").fix, Err("The agent's pane is closed.".to_string()));
    }

    #[test]
    fn night_facts_leave_the_scans_to_the_live_layer() {
        let (items, _) = from_night(&morning_fixture(), &marker());
        for item in &items {
            assert!(item.facts.secret_files.is_empty());
            assert!(item.facts.secret_lines.is_empty());
            assert!(item.facts.home_lines.is_empty());
            assert!(item.facts.ci_files.is_empty());
        }
    }

    #[test]
    fn owned_handles_include_the_tries_of_a_task_without_an_outcome() {
        let mut run = morning_fixture();
        let attempt: Attempt = run.tasks[1].attempts[0].clone();
        let mut extra = attempt;
        extra.handle = Some("run-9".into());
        run.tasks[2].attempts.push(extra);
        run.tasks[2].outcome = None;
        let (_, owned) = from_night(&run, &marker());
        assert!(owned.contains("run-9"));
    }
}
