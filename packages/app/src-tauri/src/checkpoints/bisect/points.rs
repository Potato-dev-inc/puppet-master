//! Save-point timeline: the points the search tests, and the agent turns they belong to (SPEC n33
//! section 3.3). Pure: save points in, timeline out. No I/O, no clock, no agent names.
//!
//! - `points`: the end of every agent turn in time order (all panes of the folder), then one "now"
//!   point for the folder as it is at this moment. The search tests these states, not every file save.
//! - `spans`: every agent turn as a before..after pair, so blame can name the turn that covers a time.
//!
//! Save points with a `wb-` key belong to What broke it itself (its own "now" and "before undo"
//! snapshots), so they are ignored here. Undo and Stop safety copies are not turn ends, so they are
//! ignored for points. A turn whose "before" was skipped (nothing had changed) starts at the previous
//! "after" of the same pane, like `engine::last_turn_patch`.

use std::collections::BTreeMap;

use super::model::{Point, Span, MAX_POINTS};
use crate::checkpoints::Checkpoint;

/// The states and turns of one folder, built from its save points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Timeline {
    /// Oldest first. The last point is "now" (it may keep its pane and turn when its tree equals the
    /// tree of the turn before it, so the label is still right).
    pub points: Vec<Point>,
    /// Ordered by start time, then pane, then turn.
    pub spans: Vec<Span>,
}

/// Build the timeline. `now_tree` and `now_ms` describe the folder right now. No save points at all
/// give an empty timeline.
pub fn build(checkpoints: &[Checkpoint], now_tree: &str, now_ms: u64) -> Timeline {
    if checkpoints.is_empty() {
        return Timeline { points: Vec::new(), spans: Vec::new() };
    }
    let mut points = Vec::new();
    for c in ordered(checkpoints.iter().filter(|c| c.kind == "after" && !is_own_key(&c.pane_id))) {
        points.push(Point {
            tree: c.tree.clone(),
            at_ms: c.at_ms,
            pane_key: Some(c.pane_id.clone()),
            turn: Some(c.turn),
            checkpoint_id: Some(c.id.clone()),
        });
    }
    points.push(Point { tree: now_tree.to_string(), at_ms: now_ms, pane_key: None, turn: None, checkpoint_id: None });
    // Equal trees in a row are one state. The earlier point wins, so "now" equal to the last turn end
    // keeps that turn's pane and turn for its label.
    points.dedup_by(|later, earlier| later.tree == earlier.tree);
    if points.len() > MAX_POINTS {
        points.drain(..points.len() - MAX_POINTS);
    }
    Timeline { points, spans: spans(checkpoints) }
}

/// Keys that belong to What broke it (`wb-<finding id>`).
fn is_own_key(pane_id: &str) -> bool {
    pane_id.starts_with("wb-")
}

/// The numeric part of a save point id (`refs/pm/<pane>/<n>`). A non-number sorts first.
fn number(id: &str) -> u64 {
    id.parse().unwrap_or(0)
}

/// Save points in time order: by time, then pane, then number.
fn ordered<'a>(items: impl Iterator<Item = &'a Checkpoint>) -> Vec<&'a Checkpoint> {
    let mut items: Vec<&Checkpoint> = items.collect();
    items.sort_by(|a, b| {
        (a.at_ms, a.pane_id.as_str(), number(&a.id)).cmp(&(b.at_ms, b.pane_id.as_str(), number(&b.id)))
    });
    items
}

/// One span per (pane, turn) over the before, after, answer and stop save points.
fn spans(checkpoints: &[Checkpoint]) -> Vec<Span> {
    let mut groups: BTreeMap<(&str, u32), Vec<&Checkpoint>> = BTreeMap::new();
    for c in checkpoints {
        let turn_kind = matches!(c.kind.as_str(), "before" | "after" | "answer" | "stop");
        if turn_kind && !is_own_key(&c.pane_id) {
            groups.entry((c.pane_id.as_str(), c.turn)).or_default().push(c);
        }
    }
    let mut spans: Vec<Span> = Vec::with_capacity(groups.len());
    for ((pane, turn), items) in groups {
        let Some(first) = items.iter().min_by_key(|c| (c.at_ms, number(&c.id))) else { continue };
        let before = items.iter().filter(|c| c.kind == "before").min_by_key(|c| (c.at_ms, number(&c.id)));
        let after = items.iter().filter(|c| c.kind == "after").max_by_key(|c| (c.at_ms, number(&c.id)));
        let (start_ms, before_tree, before_id) = match before {
            Some(b) => (b.at_ms, Some(b.tree.clone()), Some(b.id.clone())),
            None => match previous_after(checkpoints, pane, first.at_ms) {
                Some(p) => (p.at_ms, Some(p.tree.clone()), Some(p.id.clone())),
                None => (first.at_ms, None, None),
            },
        };
        let (end_ms, after_tree, after_id) = match after {
            Some(a) => (Some(a.at_ms), Some(a.tree.clone()), Some(a.id.clone())),
            None => (None, None, None),
        };
        spans.push(Span {
            pane_key: pane.to_string(),
            turn,
            start_ms,
            end_ms,
            before_tree,
            after_tree,
            before_id,
            after_id,
        });
    }
    spans.sort_by(|a, b| {
        a.start_ms.cmp(&b.start_ms).then_with(|| a.pane_key.cmp(&b.pane_key)).then_with(|| a.turn.cmp(&b.turn))
    });
    spans
}

/// The newest "after" of `pane` taken before `at_ms` (the state a turn starting then began from).
fn previous_after<'a>(checkpoints: &'a [Checkpoint], pane: &str, at_ms: u64) -> Option<&'a Checkpoint> {
    checkpoints
        .iter()
        .filter(|c| c.kind == "after" && c.pane_id == pane && c.at_ms < at_ms && !is_own_key(&c.pane_id))
        .max_by_key(|c| (c.at_ms, number(&c.id)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cp(pane: &str, id: &str, turn: u32, kind: &str, at_ms: u64, tree: &str) -> Checkpoint {
        Checkpoint {
            id: id.into(),
            commit: format!("commit-{id}"),
            pane_id: pane.into(),
            turn,
            kind: kind.into(),
            label: String::new(),
            note: String::new(),
            at_ms,
            files: 0,
            added: 0,
            deleted: 0,
            tree: tree.into(),
        }
    }

    fn trees(t: &Timeline) -> Vec<&str> {
        t.points.iter().map(|p| p.tree.as_str()).collect()
    }

    #[test]
    fn empty_input_gives_empty_timeline() {
        let t = build(&[], "now", 10);
        assert!(t.points.is_empty());
        assert!(t.spans.is_empty());
    }

    #[test]
    fn two_panes_interleave_in_time_order_with_now_last() {
        let items = [
            cp("a", "1", 1, "after", 100, "A1"),
            cp("b", "1", 1, "after", 150, "B1"),
            cp("a", "2", 2, "after", 200, "A2"),
        ];
        let t = build(&items, "NOW", 300);
        assert_eq!(trees(&t), ["A1", "B1", "A2", "NOW"]);
        assert_eq!(t.points[1].pane_key.as_deref(), Some("b"));
        assert_eq!(t.points[1].turn, Some(1));
        assert_eq!(t.points[1].checkpoint_id.as_deref(), Some("1"));
        assert_eq!(t.points[2].pane_key.as_deref(), Some("a"));
        assert_eq!(t.points[2].turn, Some(2));
        assert_eq!(t.points[3].pane_key, None);
        assert_eq!(t.points[3].turn, None);
        assert_eq!(t.points[3].at_ms, 300);
    }

    #[test]
    fn equal_trees_in_a_row_merge_keeping_the_earlier() {
        let items = [
            cp("a", "1", 1, "after", 100, "X"),
            cp("b", "1", 1, "after", 150, "X"),
            cp("a", "2", 2, "after", 200, "Y"),
        ];
        let t = build(&items, "NOW", 300);
        assert_eq!(trees(&t), ["X", "Y", "NOW"]);
        assert_eq!(t.points[0].pane_key.as_deref(), Some("a"));
        assert_eq!(t.points[0].at_ms, 100);
    }

    #[test]
    fn now_equal_to_last_point_keeps_its_label() {
        let items = [cp("a", "1", 1, "after", 100, "X"), cp("a", "2", 2, "after", 200, "Y")];
        let t = build(&items, "Y", 300);
        assert_eq!(trees(&t), ["X", "Y"]);
        let last = t.points.last().unwrap();
        assert_eq!(last.pane_key.as_deref(), Some("a"));
        assert_eq!(last.turn, Some(2));
        assert_eq!(last.checkpoint_id.as_deref(), Some("2"));
        assert_eq!(last.at_ms, 200);
    }

    #[test]
    fn now_equal_to_an_earlier_point_is_still_a_point() {
        let items = [cp("a", "1", 1, "after", 100, "X"), cp("a", "2", 2, "after", 200, "Y")];
        let t = build(&items, "X", 300);
        assert_eq!(trees(&t), ["X", "Y", "X"]);
        assert_eq!(t.points[2].pane_key, None);
    }

    #[test]
    fn undo_stop_and_answer_are_not_points_but_answer_and_stop_join_spans() {
        let items = [
            cp("a", "1", 1, "before", 50, "T0"),
            cp("a", "2", 1, "answer", 60, "T1"),
            cp("a", "3", 1, "after", 100, "T2"),
            cp("a", "4", 2, "undo", 150, "T3"),
            cp("a", "5", 3, "stop", 170, "T4"),
        ];
        let t = build(&items, "NOW", 200);
        assert_eq!(trees(&t), ["T2", "NOW"]);
        assert_eq!(t.spans.len(), 2);
        let turn1 = &t.spans[0];
        assert_eq!((turn1.turn, turn1.start_ms, turn1.end_ms), (1, 50, Some(100)));
        assert_eq!(turn1.before_tree.as_deref(), Some("T0"));
        assert_eq!(turn1.after_tree.as_deref(), Some("T2"));
        assert_eq!(turn1.before_id.as_deref(), Some("1"));
        assert_eq!(turn1.after_id.as_deref(), Some("3"));
        // Turn 2 has only an undo save point, so it is not a turn at all.
        // Turn 3 has only a stop: no before, so it starts at the previous "after" of the pane.
        let turn3 = &t.spans[1];
        assert_eq!((turn3.turn, turn3.start_ms, turn3.end_ms), (3, 100, None));
        assert_eq!(turn3.before_tree.as_deref(), Some("T2"));
        assert_eq!(turn3.after_tree, None);
    }

    #[test]
    fn missing_before_falls_back_to_previous_after_of_the_pane() {
        let items = [
            cp("a", "1", 1, "after", 100, "A"),
            cp("a", "2", 2, "answer", 120, "B"),
            cp("a", "3", 2, "after", 140, "C"),
        ];
        let t = build(&items, "NOW", 200);
        let turn2 = t.spans.iter().find(|s| s.turn == 2).unwrap();
        assert_eq!((turn2.start_ms, turn2.end_ms), (100, Some(140)));
        assert_eq!(turn2.before_tree.as_deref(), Some("A"));
        assert_eq!(turn2.before_id.as_deref(), Some("1"));
        assert_eq!(turn2.after_tree.as_deref(), Some("C"));
    }

    #[test]
    fn fallback_uses_only_the_same_panes_after() {
        let items = [cp("b", "1", 1, "after", 100, "OTHER"), cp("a", "1", 1, "answer", 120, "B")];
        let t = build(&items, "NOW", 200);
        let span = t.spans.iter().find(|s| s.pane_key == "a").unwrap();
        assert_eq!(span.start_ms, 120);
        assert_eq!(span.before_tree, None);
        assert_eq!(span.before_id, None);
    }

    #[test]
    fn open_turn_has_no_end() {
        let items = [cp("a", "1", 1, "before", 50, "T0")];
        let t = build(&items, "NOW", 200);
        assert_eq!(trees(&t), ["NOW"]);
        let span = &t.spans[0];
        assert_eq!((span.start_ms, span.end_ms), (50, None));
        assert_eq!(span.before_tree.as_deref(), Some("T0"));
        assert_eq!(span.after_tree, None);
        assert_eq!(span.after_id, None);
    }

    #[test]
    fn wb_keys_are_ignored_for_points_and_spans() {
        let items = [
            cp("wb-0123456789ab", "1", 1, "after", 100, "W1"),
            cp("wb-0123456789ab", "2", 1, "before", 90, "W0"),
            cp("a", "1", 1, "after", 200, "A1"),
        ];
        let t = build(&items, "NOW", 300);
        assert_eq!(trees(&t), ["A1", "NOW"]);
        assert_eq!(t.spans.len(), 1);
        assert_eq!(t.spans[0].pane_key, "a");
    }

    #[test]
    fn ties_sort_by_pane_then_number() {
        let items = [
            cp("b", "1", 1, "after", 100, "B"),
            cp("a", "10", 2, "after", 100, "A10"),
            cp("a", "9", 1, "after", 100, "A9"),
        ];
        let t = build(&items, "NOW", 300);
        assert_eq!(trees(&t), ["A9", "A10", "B", "NOW"]);
    }

    #[test]
    fn cap_keeps_the_newest_points_and_now() {
        let items: Vec<Checkpoint> =
            (0..120u32).map(|i| cp("a", &i.to_string(), i, "after", 10 + u64::from(i), &format!("t{i}"))).collect();
        let t = build(&items, "NOW", 1000);
        assert_eq!(t.points.len(), MAX_POINTS);
        assert_eq!(t.points[0].tree, "t21");
        assert_eq!(t.points.last().unwrap().tree, "NOW");
        assert_eq!(t.spans.len(), 120);
    }
}
