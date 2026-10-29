//! Review Desk: issue-run converter (SPEC n33 section 3.4 B). Pure: runs in, items out. No I/O. The facts
//! are the push check's own facts. Keep for an `AskPush` run is the draft push (its reasons), never a
//! merge. A `Held` run has no Keep (S9): its reason is the message.

use std::collections::HashSet;

use crate::issue_intake::model::{IssueRun, PushFacts, RunState};
use crate::issue_intake::pushcheck;
use crate::redact::rules::RedactContext;
use crate::redact::strict_with;
use crate::review_desk::model::{strip_controls, Facts, Item, ItemKey, Keep, Level, Risk, Source, Throw, TITLE_MAX};

/// Used for the pane: an issue run keeps no pane in the states the desk lists, so Ask to fix is off.
const PANE_CLOSED: &str = "The agent's pane is closed.";

/// Issue items (SPEC 3.4 B): runs in `AskPush` or `Held`. Returns the items and the owned handles: the
/// handle of every run that is not in a final state (`PrOpen`, `ThrownAway`, `Declined`, `Ignored`,
/// `Expired`, `Skipped`).
pub fn from_issues(runs: &[IssueRun]) -> (Vec<Item>, HashSet<String>) {
    let owned = runs
        .iter()
        .filter(|run| !is_final(&run.state))
        .filter_map(|run| run.handle.clone())
        .collect();
    let items = runs.iter().filter_map(item_of).collect();
    (items, owned)
}

fn is_final(state: &RunState) -> bool {
    matches!(
        state,
        RunState::PrOpen { .. }
            | RunState::ThrownAway
            | RunState::Declined
            | RunState::Ignored
            | RunState::Expired
            | RunState::Skipped { .. }
    )
}

fn item_of(run: &IssueRun) -> Option<Item> {
    let key = ItemKey::parse(&format!("issue:{}", run.id))?;
    let (facts, turns, keep) = match &run.state {
        RunState::AskPush { facts, .. } => (
            facts_of(facts),
            facts.turns,
            Keep::IssuePush { run_id: run.id.clone(), reasons: pushcheck::reasons(facts, true).0 },
        ),
        RunState::Held { message, .. } => (
            Facts::default(),
            0,
            Keep::No { why: format!("Held: {}. Open Settings > Assign an issue.", strip_controls(message)) },
        ),
        _ => return None,
    };
    Some(Item {
        key,
        source: Source::Issue,
        // Placeholder: `gather` computes the real fingerprint.
        fingerprint: String::new(),
        title: title_of(&format!("#{} {}", run.issue.number, run.issue.title)),
        agent_label: run.agent_label.clone(),
        account: run.agent.account.clone(),
        where_label: format!("Issue #{}", run.issue.number),
        // The local project is the repo id here; `gather` replaces it with the repo's folder.
        project: run.repo_id.clone(),
        pane_id: None,
        finished_at_ms: run.at_ms,
        turns,
        group: None,
        facts,
        // Placeholders: `gather` computes the real risk from the facts.
        risk: Risk { tier: 7, level: Level::Low, reasons: Vec::new() },
        keep,
        throw: Throw::IssueThrow { run_id: run.id.clone() },
        fix: Err(PANE_CLOSED.to_string()),
    })
}

/// The push check's facts, copied. The changed file names are not in the push facts, so `files` is
/// `None` until `gather` fills them.
fn facts_of(push: &PushFacts) -> Facts {
    Facts {
        files: None,
        files_total: push.files,
        added: push.additions,
        deleted: push.deletions,
        weakened: Vec::new(),
        checks: None,
        secret_files: push.secret_files.iter().map(|path| strip_controls(path)).collect(),
        ci_files: push.ci_files.iter().map(|path| strip_controls(path)).collect(),
        secret_lines: push.secret_lines.iter().map(|(path, line)| (strip_controls(path), *line)).collect(),
        home_lines: push.home_path_lines.iter().map(|(path, line)| (strip_controls(path), *line)).collect(),
        lines_scanned: true,
    }
}

/// The title as one line: redacted with the pure rules (token shapes and long opaque runs; this converter has
/// no live secrets or home folder to mask), control and bidi characters removed, at most `TITLE_MAX`.
fn title_of(raw: &str) -> String {
    let context = RedactContext { env_values: Vec::new(), home: None };
    let redacted = strict_with(raw, &context).into_inner();
    let line = redacted.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or("");
    strip_controls(line).trim().chars().take(TITLE_MAX).collect::<String>().trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::issue_intake::model::{AgentPick, CleanNotes, IssueSnapshot};

    fn push_facts() -> PushFacts {
        PushFacts {
            files: 3,
            additions: 12,
            deletions: 3,
            ci_files: Vec::new(),
            secret_files: Vec::new(),
            secret_lines: Vec::new(),
            home_path_lines: Vec::new(),
            internal_files: Vec::new(),
            git_changed: false,
            remote_branch_exists: false,
            issue_changed: false,
            run_status: "completed".into(),
            turns: 4,
            minutes: 30,
        }
    }

    /// A run with a valid `ir-` id built from `n`.
    fn run(n: u64, state: RunState, handle: Option<&str>) -> IssueRun {
        IssueRun {
            id: format!("ir-{n:012x}"),
            repo_id: "rw-0123abcd".into(),
            issue: IssueSnapshot {
                owner_repo: "acme/app".into(),
                number: 12,
                url: "https://github.com/acme/app/issues/12".into(),
                title: "Cart total rounds down".into(),
                body: String::new(),
                author: "maintainer".into(),
                author_association: "MEMBER".into(),
                labeled_by: "maintainer".into(),
                labeled_by_bot: false,
                label_event_id: 1,
                labeled_at_ms: 1,
                notes: CleanNotes::default(),
                sha8: "00000000".into(),
            },
            agent: AgentPick { agent_type: "codex".into(), account: None },
            agent_label: "Codex".into(),
            at_ms: 1_000,
            started_at_ms: Some(2_000),
            forced_ask: Vec::new(),
            approved_by: Some("desktop".into()),
            handle: handle.map(str::to_string),
            branch: None,
            base: None,
            git_fingerprint: None,
            state,
        }
    }

    fn ask(facts: PushFacts) -> RunState {
        RunState::AskPush { question_id: "q-1".into(), facts }
    }

    #[test]
    fn ask_push_becomes_an_item_with_the_push_reasons() {
        let facts = PushFacts { secret_lines: vec![("src/config.ts".into(), 12)], ..push_facts() };
        let runs = [run(1, ask(facts.clone()), Some("issue-12-ab12"))];
        let (items, owned) = from_issues(&runs);
        assert_eq!(items.len(), 1);
        let item = &items[0];
        assert_eq!(item.key.as_str(), "issue:ir-000000000001");
        assert_eq!(item.source, Source::Issue);
        assert_eq!(item.where_label, "Issue #12");
        assert_eq!(item.title, "#12 Cart total rounds down");
        assert_eq!(item.agent_label, "Codex");
        assert_eq!(
            item.keep,
            Keep::IssuePush { run_id: "ir-000000000001".into(), reasons: pushcheck::reasons(&facts, true).0 }
        );
        assert!(
            matches!(&item.keep, Keep::IssuePush { reasons, .. } if reasons.iter().any(|r| r.contains("src/config.ts line 12")))
        );
        assert_eq!(item.throw, Throw::IssueThrow { run_id: "ir-000000000001".into() });
        assert_eq!(owned, HashSet::from(["issue-12-ab12".to_string()]));
    }

    #[test]
    fn push_facts_are_copied_into_the_item() {
        let facts = PushFacts {
            ci_files: vec![".github/workflows/ci.yml".into()],
            secret_files: vec![".env".into()],
            secret_lines: vec![("src/config.ts".into(), 12)],
            home_path_lines: vec![("notes.md".into(), 3)],
            ..push_facts()
        };
        let (items, _) = from_issues(&[run(2, ask(facts), Some("issue-12-cd34"))]);
        let item = &items[0];
        assert_eq!(item.facts.files_total, 3);
        assert_eq!((item.facts.added, item.facts.deleted), (12, 3));
        assert_eq!(item.facts.ci_files, vec![".github/workflows/ci.yml".to_string()]);
        assert_eq!(item.facts.secret_files, vec![".env".to_string()]);
        assert_eq!(item.facts.secret_lines, vec![("src/config.ts".to_string(), 12)]);
        assert_eq!(item.facts.home_lines, vec![("notes.md".to_string(), 3)]);
        assert!(item.facts.lines_scanned);
        assert!(item.facts.files.is_none(), "names are filled by the live layer");
        assert!(item.facts.checks.is_none());
        assert!(item.facts.weakened.is_empty());
        assert_eq!(item.turns, 4);
        assert_eq!(item.project, "rw-0123abcd");
        assert_eq!(item.pane_id, None);
        assert_eq!(item.fix, Err("The agent's pane is closed.".to_string()));
    }

    #[test]
    fn held_run_has_no_keep_and_says_why() {
        let held = RunState::Held {
            code: "GIT_HOOKS_CHANGED".into(),
            message: "The git hooks changed while the agent worked".into(),
        };
        let (items, owned) = from_issues(&[run(3, held, Some("issue-13-ef56"))]);
        assert_eq!(items.len(), 1);
        let item = &items[0];
        assert_eq!(
            item.keep,
            Keep::No {
                why: "Held: The git hooks changed while the agent worked. Open Settings > Assign an issue.".into()
            }
        );
        assert_eq!(item.throw, Throw::IssueThrow { run_id: "ir-000000000003".into() });
        assert!(!item.facts.lines_scanned);
        assert_eq!(owned, HashSet::from(["issue-13-ef56".to_string()]));
    }

    #[test]
    fn final_states_own_nothing_and_list_nothing() {
        let finals = vec![
            RunState::PrOpen { number: 1, url: "https://github.com/acme/app/pull/1".into() },
            RunState::ThrownAway,
            RunState::Declined,
            RunState::Ignored,
            RunState::Expired,
            RunState::Skipped { why: "The labeler is not allowed.".into() },
        ];
        for (n, state) in finals.into_iter().enumerate() {
            let (items, owned) = from_issues(&[run(10 + n as u64, state, Some("issue-final"))]);
            assert!(items.is_empty());
            assert!(owned.is_empty(), "a final run owns no handle");
        }
    }

    #[test]
    fn live_runs_own_their_handle_and_are_not_listed_unless_asking_or_held() {
        let states = vec![
            RunState::Running { handle: "issue-run-1".into(), operation_id: "op-1".into(), pane_id: None },
            RunState::Checking,
            RunState::NoChanges { message: "Nothing changed.".into() },
            RunState::Failed { code: "X".into(), message: "Failed.".into() },
        ];
        for (n, state) in states.into_iter().enumerate() {
            let handle = format!("issue-live-{n}");
            let (items, owned) = from_issues(&[run(20 + n as u64, state, Some(handle.as_str()))]);
            assert!(items.is_empty(), "run {n} is not listed");
            assert!(owned.contains(&handle), "run {n} owns its handle");
        }
    }

    #[test]
    fn a_run_without_a_handle_owns_nothing() {
        let (items, owned) = from_issues(&[run(30, ask(push_facts()), None)]);
        assert_eq!(items.len(), 1);
        assert!(owned.is_empty());
    }

    #[test]
    fn title_drops_control_and_bidi_characters_and_keeps_the_first_line() {
        assert_eq!(title_of("Fix\u{202E}ed\tcart\nsecond line"), "Fixedcart");
        // Words with spaces are not one opaque run, so the cap is what limits the length here.
        let long = "Cart total rounds down again ".repeat(10);
        assert!(title_of(&long).chars().count() <= TITLE_MAX);
    }
}
