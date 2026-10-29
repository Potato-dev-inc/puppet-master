//! Blame: which agent turn an answer points at (SPEC n33 section 3.3). Pure: no I/O.
//!
//! The search says "tests passed at save point A and failed at save point B". This file turns that
//! time range into the agent turn(s) that were running inside it:
//! - one turn overlaps the range: `Turn` (the culprit, if the start of the turn is also clean);
//! - more than one turn overlaps (two panes at once, say): `Several`, and no Undo (S7);
//! - no turn overlaps: `Outside` ("something outside the agents' turns changed it").
//!
//! Generic code: labels come from the turn summaries only (S13). A missing summary reads "An agent".

use super::model::{FileStatsLite, Span, Suspect};
use super::words::UNKNOWN_AGENT;
use crate::checkpoints::engine::pane_key;
use crate::turn_summary::TurnSummary;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Blame {
    /// Exactly one turn overlaps the range.
    Turn(Span),
    /// Two or more turns overlap the range, in the order they were given.
    Several(Vec<Span>),
    /// No turn overlaps the range.
    Outside,
}

/// The turns that overlap the range `(good_at_ms, bad_at_ms)`. A turn overlaps when it ends after the
/// good point (an open turn never ends) and starts before the bad point. A turn that ends exactly at
/// the good point, or starts exactly at the bad point, is not a suspect.
pub fn blame(spans: &[Span], good_at_ms: u64, bad_at_ms: u64) -> Blame {
    let mut suspects: Vec<Span> = spans
        .iter()
        .filter(|s| s.end_ms.unwrap_or(u64::MAX) > good_at_ms && s.start_ms < bad_at_ms)
        .cloned()
        .collect();
    match suspects.len() {
        0 => Blame::Outside,
        1 => Blame::Turn(suspects.remove(0)),
        _ => Blame::Several(suspects),
    }
}

/// When the turn's own starting files already fail, the turn did not break them: the answer becomes
/// `Outside`. Only a single `Turn` is changed; `Several` and `Outside` pass through.
pub fn after_refine(b: Blame, before_tree_fails: bool) -> Blame {
    match b {
        Blame::Turn(_) if before_tree_fails => Blame::Outside,
        other => other,
    }
}

/// When a single turn started from files that differ from the known-good tree, the search must also
/// measure that starting tree. Returns that tree id. `None` when the answer is not a single turn, when
/// the turn started from the good tree, or when its starting tree is unknown.
pub fn needs_refine(b: &Blame, good_tree: &str) -> Option<String> {
    match b {
        Blame::Turn(span) => match &span.before_tree {
            Some(before) if before != good_tree => Some(before.clone()),
            _ => None,
        },
        Blame::Several(_) | Blame::Outside => None,
    }
}

/// Joins a turn with its summary (agent label, account, scrubbed sentence, file counts). The summary
/// matches on turn number and on the pane key (`engine::pane_key` of its pane id). When no summary is
/// kept, the label is "An agent" and the other text fields are empty. The sentence is copied as it is:
/// scrubbing already happened when the summary was saved.
pub fn join_summaries(span: &Span, summaries: &[TurnSummary]) -> Suspect {
    let at_ms = span.end_ms.unwrap_or(span.start_ms);
    let found = summaries
        .iter()
        .find(|s| s.turn == span.turn && pane_key(&s.pane_id) == span.pane_key);
    match found {
        Some(s) => Suspect {
            pane_key: span.pane_key.clone(),
            turn: span.turn,
            agent_label: s.agent.clone(),
            agent_id: s.agent_id.clone(),
            account: s.account.clone(),
            at_ms,
            sentence: Some(s.sentence.clone()),
            files: s.files.as_ref().map(|f| FileStatsLite { count: f.count, added: f.added, deleted: f.deleted }),
        },
        None => Suspect {
            pane_key: span.pane_key.clone(),
            turn: span.turn,
            agent_label: UNKNOWN_AGENT.into(),
            agent_id: None,
            account: None,
            at_ms,
            sentence: None,
            files: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::turn_summary::model::EndState;
    use crate::turn_summary::FileStats;

    fn span(pane: &str, turn: u32, start: u64, end: Option<u64>) -> Span {
        Span {
            pane_key: pane.into(),
            turn,
            start_ms: start,
            end_ms: end,
            before_tree: Some(format!("before-{pane}-{turn}")),
            after_tree: end.map(|_| format!("after-{pane}-{turn}")),
            before_id: None,
            after_id: None,
        }
    }

    fn summary(pane_id: &str, turn: u32, agent: &str, sentence: &str, files: Option<FileStats>) -> TurnSummary {
        TurnSummary {
            pane_id: pane_id.into(),
            agent: agent.into(),
            agent_id: Some("agent-one".into()),
            account: Some("account-one".into()),
            turn,
            started_at_ms: 0,
            ended_at_ms: 0,
            duration_ms: 0,
            files,
            approvals: Vec::new(),
            end: EndState { kind: "idle".into(), prompt: None },
            receipt: None,
            sentence: sentence.into(),
        }
    }

    #[test]
    fn one_turn_in_the_range_is_the_blame() {
        let spans = vec![span("p1", 1, 0, Some(40)), span("p1", 2, 100, Some(200)), span("p1", 3, 400, Some(500))];
        assert_eq!(blame(&spans, 50, 300), Blame::Turn(span("p1", 2, 100, Some(200))));
    }

    #[test]
    fn two_panes_overlapping_are_several() {
        let spans = vec![span("p1", 3, 100, Some(300)), span("p2", 1, 150, Some(250))];
        assert_eq!(blame(&spans, 50, 280), Blame::Several(spans.clone()));
    }

    #[test]
    fn a_gap_with_no_turn_is_outside() {
        let spans = vec![span("p1", 1, 0, Some(100)), span("p1", 2, 300, Some(400))];
        assert_eq!(blame(&spans, 150, 250), Blame::Outside);
        assert_eq!(blame(&[], 0, 10), Blame::Outside);
    }

    #[test]
    fn an_open_turn_is_a_suspect_when_it_started_in_range() {
        let open = span("p1", 4, 100, None);
        assert_eq!(blame(std::slice::from_ref(&open), 50, 200), Blame::Turn(open));
        // An open turn that starts after the bad point is not a suspect.
        assert_eq!(blame(&[span("p1", 5, 300, None)], 50, 200), Blame::Outside);
    }

    #[test]
    fn a_turn_that_began_before_the_good_point_is_a_suspect() {
        let spans = vec![span("p1", 2, 40, Some(180))];
        assert_eq!(blame(&spans, 100, 220), Blame::Turn(span("p1", 2, 40, Some(180))));
        // A turn that ended exactly at the good point is not.
        assert_eq!(blame(&[span("p1", 1, 0, Some(100))], 100, 150), Blame::Outside);
        // A turn that starts exactly at the bad point is not.
        assert_eq!(blame(&[span("p1", 3, 200, Some(300))], 50, 200), Blame::Outside);
    }

    #[test]
    fn after_refine_turns_a_failing_start_into_outside() {
        let turn = Blame::Turn(span("p1", 2, 100, Some(200)));
        assert_eq!(after_refine(turn.clone(), true), Blame::Outside);
        assert_eq!(after_refine(turn.clone(), false), turn);
        let several = Blame::Several(vec![span("p1", 2, 100, Some(200)), span("p2", 1, 90, Some(210))]);
        assert_eq!(after_refine(several.clone(), true), several);
        assert_eq!(after_refine(Blame::Outside, true), Blame::Outside);
    }

    #[test]
    fn needs_refine_returns_the_start_tree_when_it_differs_from_good() {
        let turn = Blame::Turn(span("p1", 2, 100, Some(200)));
        assert_eq!(needs_refine(&turn, "good-tree"), Some("before-p1-2".to_string()));
        assert_eq!(needs_refine(&turn, "before-p1-2"), None);
        let no_start = Blame::Turn(Span { before_tree: None, ..span("p1", 2, 100, Some(200)) });
        assert_eq!(needs_refine(&no_start, "good-tree"), None);
        assert_eq!(needs_refine(&Blame::Several(Vec::new()), "good-tree"), None);
        assert_eq!(needs_refine(&Blame::Outside, "good-tree"), None);
    }

    #[test]
    fn join_matches_the_pane_key_and_turn_and_copies_the_summary() {
        let stats = FileStats { count: 2, added: 7, deleted: 3, paths: vec!["a.txt".into()] };
        let kept = summary("pane:1", 7, "Test Agent", "Edited the login page [redacted]", Some(stats));
        let s = Span { pane_key: pane_key("pane:1"), ..span("x", 7, 100, Some(200)) };
        let suspect = join_summaries(&s, &[kept]);
        assert_eq!(suspect.pane_key, "pane_1");
        assert_eq!(suspect.turn, 7);
        assert_eq!(suspect.agent_label, "Test Agent");
        assert_eq!(suspect.agent_id.as_deref(), Some("agent-one"));
        assert_eq!(suspect.account.as_deref(), Some("account-one"));
        assert_eq!(suspect.at_ms, 200);
        assert_eq!(suspect.sentence.as_deref(), Some("Edited the login page [redacted]"));
        assert_eq!(suspect.files, Some(FileStatsLite { count: 2, added: 7, deleted: 3 }));
    }

    #[test]
    fn join_ignores_a_summary_of_another_pane_or_turn() {
        let other_pane = summary("pane-2", 7, "Other", "x", None);
        let other_turn = summary("pane-1", 8, "Other", "x", None);
        let s = Span { pane_key: pane_key("pane-1"), ..span("x", 7, 100, Some(200)) };
        let suspect = join_summaries(&s, &[other_pane, other_turn]);
        assert_eq!(suspect.agent_label, "An agent");
        assert_eq!(suspect.sentence, None);
    }

    #[test]
    fn a_missing_summary_reads_an_agent() {
        let s = span("p9", 3, 100, None);
        let suspect = join_summaries(&s, &[]);
        assert_eq!(suspect.agent_label, "An agent");
        assert_eq!(suspect.agent_id, None);
        assert_eq!(suspect.account, None);
        assert_eq!(suspect.sentence, None);
        assert_eq!(suspect.files, None);
        // An open turn is placed at its start.
        assert_eq!(suspect.at_ms, 100);
    }

    #[test]
    fn a_summary_without_file_counts_keeps_files_empty() {
        let kept = summary("p1", 1, "Test Agent", "Asked a question", None);
        let suspect = join_summaries(&span("p1", 1, 0, Some(10)), &[kept]);
        assert_eq!(suspect.agent_label, "Test Agent");
        assert_eq!(suspect.files, None);
        assert_eq!(suspect.at_ms, 10);
    }
}
