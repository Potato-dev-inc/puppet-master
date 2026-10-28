//! Collision Alert: the life of each alert, in memory (SPEC-n33 sections 3.6 and 5 CA-9).
//!
//! Pure: no process, no clock, no file. The caller passes the findings of one step and the panes
//! that closed; `apply` returns what changed. One alert per pair of panes (`AlertId::of`).
//!
//! - A finding for a pair creates or updates its alert. A pair without a finding gains a miss; at
//!   `RESOLVE_AFTER_MISSES` misses, or when a pane closes, a visible alert is resolved.
//! - `ignore` hides an alert and remembers its clash and shared paths. A hidden alert comes back
//!   only when a finding holds a clash or shared path that was not remembered for that pair.
//! - At most `MAX_ALERTS` visible alerts. Extra findings are dropped, lowest level first.
//! - A hidden alert keeps its record (and what it remembers) until a pane closes or `clear_all`.
//!
//! `Change` is the surface contract: `Raised` (first time visible), `Updated` (in place),
//! `Resolved` (gone). A hidden alert gets no change; the caller clears its attention item itself.

use std::collections::{BTreeMap, BTreeSet};

use super::advice::advise;
use super::model::{Alert, AlertId, FileOverlap, How, Level, PaneFact, Relation, SideFacts, MAX_ALERTS, RESOLVE_AFTER_MISSES};

/// What the comparison found for one pair of panes in one step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub relation: Relation,
    pub level: Level,
    pub panes: [PaneFact; 2],
    pub sides: [SideFacts; 2],
    pub files: Vec<FileOverlap>,
}

/// One alert that changed. The strings are alert ids.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    /// First time visible (or visible again after an ignore was undone by a new clash).
    Raised(String),
    /// Visible, and its content changed.
    Updated(String),
    /// Gone from the visible list.
    Resolved(String),
}

/// Every alert of the session. Visible ones and hidden (ignored) ones.
#[derive(Debug, Default)]
pub struct Alerts {
    /// Keyed by alert id.
    records: BTreeMap<String, Alert>,
    /// Hidden alert id -> the clash and shared paths remembered when it was ignored.
    remembered: BTreeMap<String, BTreeSet<String>>,
}

impl Alerts {
    /// Applies one step. `findings` are this step's results (at most one per pair is used), `gone`
    /// are the pane ids that closed. Returns the changes, in the order they happened.
    pub fn apply(&mut self, findings: &[Finding], gone: &BTreeSet<String>, now: u64) -> Vec<Change> {
        let mut changes = Vec::new();

        // A pane that closed ends every alert it is in. A hidden one goes without a change.
        let closed: Vec<String> = self
            .records
            .values()
            .filter(|a| a.panes.iter().any(|p| gone.contains(&p.pane_id)))
            .map(|a| a.id.clone())
            .collect();
        for id in closed {
            self.resolve(&id, &mut changes);
        }

        // One candidate per pair, in pane id order, loudest first (stable inside one level).
        let mut seen = BTreeSet::new();
        let mut cands: Vec<(String, Finding)> = Vec::new();
        for f in findings {
            if f.panes.iter().any(|p| gone.contains(&p.pane_id)) {
                continue;
            }
            let Some(n) = normalise(f) else { continue };
            let id = AlertId::of(&[n.panes[0].pane_id.as_str(), n.panes[1].pane_id.as_str()]);
            if seen.insert(id.clone()) {
                cands.push((id, n));
            }
        }
        cands.sort_by(|x, y| y.1.level.cmp(&x.1.level));

        // The cap. Visible alerts that no finding names and that survive this step (no miss yet)
        // use their place first; the rest of the room goes to the candidates, loudest first.
        let named: BTreeSet<String> = cands.iter().map(|(id, _)| id.clone()).collect();
        let survivors = self
            .records
            .values()
            .filter(|a| !a.hidden && a.misses == 0 && !named.contains(&a.id))
            .count();
        let mut budget = MAX_ALERTS.saturating_sub(survivors);

        let mut touched = BTreeSet::new();
        for (id, f) in cands {
            let existing = self.records.get(&id).cloned();
            match existing {
                Some(a) if a.hidden => {
                    let remembered = self.remembered.get(&id).cloned().unwrap_or_default();
                    if unhides(&remembered, &f.files) {
                        if budget == 0 {
                            continue;
                        }
                        budget -= 1;
                        self.remembered.remove(&id);
                        self.records.insert(id.clone(), build(&id, &f, now, now, false));
                        changes.push(Change::Raised(id.clone()));
                    } else {
                        // Still ignored: the facts follow the comparison, nothing is shown.
                        self.records.insert(id.clone(), build(&id, &f, a.first_ms, now, true));
                    }
                    touched.insert(id);
                }
                Some(a) => {
                    if budget == 0 {
                        self.resolve(&id, &mut changes);
                        continue;
                    }
                    budget -= 1;
                    let next = build(&id, &f, a.first_ms, now, false);
                    if !same_look(&a, &next) {
                        changes.push(Change::Updated(id.clone()));
                    }
                    self.records.insert(id.clone(), next);
                    touched.insert(id);
                }
                None => {
                    if budget == 0 {
                        continue;
                    }
                    budget -= 1;
                    self.records.insert(id.clone(), build(&id, &f, now, now, false));
                    changes.push(Change::Raised(id.clone()));
                    touched.insert(id);
                }
            }
        }

        // Pairs that no finding named gain a miss. A visible one at the limit is resolved.
        let missed: Vec<String> = self.records.keys().filter(|id| !touched.contains(*id)).cloned().collect();
        for id in missed {
            let Some(a) = self.records.get_mut(&id) else { continue };
            a.misses = a.misses.saturating_add(1);
            let due = !a.hidden && a.misses >= RESOLVE_AFTER_MISSES;
            if due {
                self.resolve(&id, &mut changes);
            }
        }
        changes
    }

    /// Hides an alert until a new clash or shared file shows up for its pair. Returns false for an
    /// id that is not known. Ignoring a hidden alert again changes nothing.
    pub fn ignore(&mut self, id: &str) -> bool {
        let Some(a) = self.records.get_mut(id) else { return false };
        if !a.hidden {
            a.hidden = true;
            self.remembered.insert(id.to_string(), loud_paths(&a.files));
        }
        true
    }

    /// Resolves every visible alert and forgets the hidden ones. Used when the setting goes off.
    pub fn clear_all(&mut self) -> Vec<Change> {
        let changes = self
            .records
            .values()
            .filter(|a| !a.hidden)
            .map(|a| Change::Resolved(a.id.clone()))
            .collect();
        self.records.clear();
        self.remembered.clear();
        changes
    }

    /// The visible alerts, newest first (by when each was raised; ties by id).
    pub fn visible(&self) -> Vec<&Alert> {
        let mut out: Vec<&Alert> = self.records.values().filter(|a| !a.hidden).collect();
        out.sort_by(|x, y| y.first_ms.cmp(&x.first_ms).then_with(|| x.id.cmp(&y.id)));
        out
    }

    /// Takes an alert out of the records. A visible one is reported as resolved.
    fn resolve(&mut self, id: &str, changes: &mut Vec<Change>) {
        let Some(alert) = self.records.remove(id) else { return };
        self.remembered.remove(id);
        if !alert.hidden {
            changes.push(Change::Resolved(id.to_string()));
        }
    }
}

/// Puts a pair in pane id order (the order of `Alert::panes` and `Alert::sides`). A pair of one
/// pane is `None`.
fn normalise(f: &Finding) -> Option<Finding> {
    let (a, b) = (&f.panes[0].pane_id, &f.panes[1].pane_id);
    if a == b {
        return None;
    }
    if a < b {
        return Some(f.clone());
    }
    Some(Finding {
        relation: f.relation,
        level: f.level,
        panes: [f.panes[1].clone(), f.panes[0].clone()],
        sides: [f.sides[1].clone(), f.sides[0].clone()],
        files: f
            .files
            .iter()
            .map(|o| FileOverlap { path: o.path.clone(), how: flip(o.how) })
            .collect(),
    })
}

/// `DeleteVsEdit` names a side by index; the pair was swapped, so the index flips with it.
fn flip(how: How) -> How {
    match how {
        How::DeleteVsEdit { deleted_by } => How::DeleteVsEdit { deleted_by: if deleted_by == 0 { 1 } else { 0 } },
        other => other,
    }
}

/// A clash, or a shared file (the same-folder kind).
fn is_clash(how: &How) -> bool {
    matches!(how, How::Lines(_) | How::LinesUnknown | How::BothNew | How::DeleteVsEdit { .. } | How::Binary)
}

fn is_loud(how: &How) -> bool {
    is_clash(how) || matches!(how, How::Shared)
}

/// The clash and shared paths of a finding.
fn loud_paths(files: &[FileOverlap]) -> BTreeSet<String> {
    files.iter().filter(|f| is_loud(&f.how)).map(|f| f.path.clone()).collect()
}

/// True when a finding holds a clash or shared path that was not remembered for its pair.
fn unhides(remembered: &BTreeSet<String>, files: &[FileOverlap]) -> bool {
    files.iter().any(|f| is_loud(&f.how) && !remembered.contains(&f.path))
}

/// The alert a finding describes. Clash files first; the rest keep their input order (newest first).
fn build(id: &str, f: &Finding, first_ms: u64, now: u64, hidden: bool) -> Alert {
    let mut files = f.files.clone();
    files.sort_by_key(|o| !is_clash(&o.how));
    let advice = (f.level != Level::SameFile).then(|| advise(&f.sides[0], &f.sides[1], f.relation));
    Alert {
        id: id.to_string(),
        relation: f.relation,
        level: f.level,
        panes: f.panes.clone(),
        sides: f.sides.clone(),
        files,
        advice,
        first_ms,
        last_ms: now,
        misses: 0,
        hidden,
    }
}

/// Same look for the person: everything except the times, the misses and the hidden flag.
fn same_look(a: &Alert, b: &Alert) -> bool {
    a.relation == b.relation
        && a.level == b.level
        && a.panes == b.panes
        && a.sides == b.sides
        && a.files == b.files
        && a.advice == b.advice
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collision::model::{Advice, PaneRun, Span, Why};

    fn pane(id: &str) -> PaneFact {
        PaneFact {
            pane_id: id.to_string(),
            agent_id: "fake_agent".to_string(),
            label: format!("{id} label"),
            cwd: r"C:\Users\you\proj".to_string(),
            run: PaneRun::Working,
            created_ms: 1,
        }
    }

    fn side(id: &str, working: bool, files: u32, started_ms: u64) -> SideFacts {
        SideFacts { pane_id: id.to_string(), working, files, started_ms }
    }

    /// Two working panes in separate copies: `a` has 4 files, `b` has 10.
    fn finding(pair: (&str, &str), level: Level, files: Vec<FileOverlap>) -> Finding {
        Finding {
            relation: Relation::Copies,
            level,
            panes: [pane(pair.0), pane(pair.1)],
            sides: [side(pair.0, true, 4, 1), side(pair.1, true, 10, 2)],
            files,
        }
    }

    fn lines(path: &str) -> FileOverlap {
        FileOverlap { path: path.to_string(), how: How::Lines(Span { start: 40, end: 80 }) }
    }

    fn apart(path: &str) -> FileOverlap {
        FileOverlap { path: path.to_string(), how: How::Apart }
    }

    fn refs(p: &(String, String)) -> (&str, &str) {
        (p.0.as_str(), p.1.as_str())
    }

    fn none() -> BTreeSet<String> {
        BTreeSet::new()
    }

    fn gone(ids: &[&str]) -> BTreeSet<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    fn id_of(a: &str, b: &str) -> String {
        AlertId::of(&[a, b])
    }

    #[test]
    fn create_then_update_in_place() {
        let mut al = Alerts::default();
        let id = id_of("fake_a", "fake_b");
        let first = al.apply(&[finding(("fake_a", "fake_b"), Level::Clash, vec![lines("src/auth.ts")])], &none(), 100);
        assert_eq!(first, vec![Change::Raised(id.clone())]);

        let second = al.apply(
            &[finding(("fake_a", "fake_b"), Level::Clash, vec![lines("src/auth.ts"), lines("src/db.ts")])],
            &none(),
            200,
        );
        assert_eq!(second, vec![Change::Updated(id.clone())]);

        let same = al.apply(
            &[finding(("fake_a", "fake_b"), Level::Clash, vec![lines("src/auth.ts"), lines("src/db.ts")])],
            &none(),
            300,
        );
        assert!(same.is_empty(), "the same look is not a change");

        let shown = al.visible();
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].id, id);
        assert_eq!(shown[0].first_ms, 100);
        assert_eq!(shown[0].last_ms, 300);
        assert_eq!(shown[0].files.len(), 2);
    }

    #[test]
    fn two_misses_resolve() {
        let mut al = Alerts::default();
        let id = id_of("fake_a", "fake_b");
        al.apply(&[finding(("fake_a", "fake_b"), Level::Clash, vec![lines("src/auth.ts")])], &none(), 100);
        assert!(al.apply(&[], &none(), 200).is_empty());
        let changes = al.apply(&[], &none(), 300);
        assert_eq!(changes, vec![Change::Resolved(id)]);
        assert!(al.visible().is_empty());
    }

    #[test]
    fn one_miss_does_not_resolve() {
        let mut al = Alerts::default();
        al.apply(&[finding(("fake_a", "fake_b"), Level::Clash, vec![lines("src/auth.ts")])], &none(), 100);
        assert!(al.apply(&[], &none(), 200).is_empty());
        assert_eq!(al.visible().len(), 1);
        // A finding again clears the miss, so one more miss is still not enough.
        assert!(al.apply(&[finding(("fake_a", "fake_b"), Level::Clash, vec![lines("src/auth.ts")])], &none(), 300).is_empty());
        assert!(al.apply(&[], &none(), 400).is_empty());
        assert_eq!(al.visible().len(), 1);
    }

    #[test]
    fn pane_gone_resolves_at_once() {
        let mut al = Alerts::default();
        let id = id_of("fake_a", "fake_b");
        al.apply(&[finding(("fake_a", "fake_b"), Level::Clash, vec![lines("src/auth.ts")])], &none(), 100);
        // A finding that names the closed pane is not used either.
        let changes = al.apply(
            &[finding(("fake_a", "fake_b"), Level::Clash, vec![lines("src/auth.ts")])],
            &gone(&["fake_b"]),
            200,
        );
        assert_eq!(changes, vec![Change::Resolved(id)]);
        assert!(al.visible().is_empty());
        assert!(al.records.is_empty());
    }

    #[test]
    fn ignore_hides_the_alert() {
        let mut al = Alerts::default();
        let id = id_of("fake_a", "fake_b");
        al.apply(&[finding(("fake_a", "fake_b"), Level::Clash, vec![lines("src/auth.ts")])], &none(), 100);
        assert!(al.ignore(&id));
        assert!(al.visible().is_empty());
        assert!(!al.ignore("ca-000000000000"), "an unknown id is not found");
        // The same finding does not bring it back, and nothing is reported for it.
        assert!(al.apply(&[finding(("fake_a", "fake_b"), Level::Clash, vec![lines("src/auth.ts")])], &none(), 200).is_empty());
        assert!(al.visible().is_empty());
    }

    #[test]
    fn same_files_stay_hidden() {
        let mut al = Alerts::default();
        al.apply(&[finding(("fake_a", "fake_b"), Level::Clash, vec![lines("src/auth.ts")])], &none(), 100);
        al.ignore(&id_of("fake_a", "fake_b"));
        // Many looks, the clash file grows its line span, the same path: still hidden.
        for step in 0..5u64 {
            let wider = FileOverlap { path: "src/auth.ts".to_string(), how: How::Lines(Span { start: 10, end: 90 + step as u32 }) };
            assert!(al.apply(&[finding(("fake_a", "fake_b"), Level::Clash, vec![wider])], &none(), 200 + step).is_empty());
        }
        assert!(al.visible().is_empty());
    }

    #[test]
    fn new_clash_file_unhides() {
        let mut al = Alerts::default();
        let id = id_of("fake_a", "fake_b");
        al.apply(&[finding(("fake_a", "fake_b"), Level::Clash, vec![lines("src/auth.ts")])], &none(), 100);
        al.ignore(&id);
        let changes = al.apply(
            &[finding(("fake_a", "fake_b"), Level::Clash, vec![lines("src/auth.ts"), lines("src/db.ts")])],
            &none(),
            200,
        );
        assert_eq!(changes, vec![Change::Raised(id.clone())]);
        let shown = al.visible();
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].id, id);
        assert!(!shown[0].hidden);
    }

    #[test]
    fn new_same_file_never_unhides() {
        let mut al = Alerts::default();
        let id = id_of("fake_a", "fake_b");
        al.apply(&[finding(("fake_a", "fake_b"), Level::Clash, vec![lines("src/auth.ts")])], &none(), 100);
        al.ignore(&id);
        let quiet = finding(("fake_a", "fake_b"), Level::SameFile, vec![apart("src/db.ts"), apart("src/auth.ts")]);
        assert!(al.apply(&[quiet], &none(), 200).is_empty());
        assert!(al.visible().is_empty());
    }

    #[test]
    fn cap_of_six_drops_the_lowest_level() {
        let mut al = Alerts::default();
        let names: Vec<(String, String)> = (0..7).map(|i| (format!("fake_a_{i}"), format!("fake_b_{i}"))).collect();

        // Six clashes and one same-file pair, the quiet one first in the input.
        let mut findings = vec![finding(refs(&names[6]), Level::SameFile, vec![apart("src/db.ts")])];
        for p in names.iter().take(6) {
            findings.push(finding(refs(p), Level::Clash, vec![lines("src/auth.ts")]));
        }
        let changes = al.apply(&findings, &none(), 100);
        assert_eq!(changes.len(), 6);
        assert!(changes.iter().all(|c| matches!(c, Change::Raised(_))));
        assert_eq!(al.visible().len(), 6);
        assert!(al.visible().iter().all(|a| a.level == Level::Clash));
        assert!(!al.records.contains_key(&id_of(&names[6].0, &names[6].1)), "the quiet one is dropped");

        // Now the other way round: six quiet pairs before one clash. The clash keeps its place.
        let mut second = Alerts::default();
        let mut findings = Vec::new();
        for p in names.iter().take(6) {
            findings.push(finding(refs(p), Level::SameFile, vec![apart("src/db.ts")]));
        }
        findings.push(finding(refs(&names[6]), Level::Clash, vec![lines("src/auth.ts")]));
        second.apply(&findings, &none(), 100);
        let shown = second.visible();
        assert_eq!(shown.len(), 6);
        assert!(shown.iter().any(|a| a.level == Level::Clash), "the clash is kept");
        assert_eq!(shown.iter().filter(|a| a.level == Level::SameFile).count(), 5);
    }

    #[test]
    fn id_is_stable_for_the_pair_in_either_pane_order() {
        let mut al = Alerts::default();
        let id = id_of("fake_a", "fake_b");
        let first = al.apply(&[finding(("fake_b", "fake_a"), Level::Clash, vec![lines("src/auth.ts")])], &none(), 100);
        assert_eq!(first, vec![Change::Raised(id.clone())]);
        let second = al.apply(&[finding(("fake_a", "fake_b"), Level::Clash, vec![lines("src/auth.ts")])], &none(), 200);
        assert!(!second.iter().any(|c| matches!(c, Change::Raised(_))), "same pair, same alert");
        assert_eq!(al.records.len(), 1);
        let shown = al.visible();
        assert_eq!(shown[0].id, id);
        assert_eq!(shown[0].panes[0].pane_id, "fake_a", "panes are kept in pane id order");
        assert_eq!(shown[0].panes[1].pane_id, "fake_b");
    }

    #[test]
    fn delete_vs_edit_index_follows_the_sorted_pair() {
        let mut al = Alerts::default();
        // In the finding `fake_b` is first and it is the side that deleted the file: index 0.
        let deleted = FileOverlap { path: "src/old.ts".to_string(), how: How::DeleteVsEdit { deleted_by: 0 } };
        al.apply(&[finding(("fake_b", "fake_a"), Level::Clash, vec![deleted])], &none(), 100);
        let shown = al.visible();
        assert_eq!(shown[0].panes[1].pane_id, "fake_b", "fake_b sorts second");
        assert_eq!(shown[0].files[0].how, How::DeleteVsEdit { deleted_by: 1 });
    }

    #[test]
    fn advice_flips_when_the_working_flags_change() {
        let mut al = Alerts::default();
        let both_working = Finding {
            relation: Relation::Copies,
            level: Level::Clash,
            panes: [pane("fake_a"), pane("fake_b")],
            sides: [side("fake_a", true, 4, 1), side("fake_b", true, 10, 2)],
            files: vec![lines("src/auth.ts")],
        };
        al.apply(&[both_working], &none(), 100);
        assert_eq!(al.visible()[0].advice, Some(Advice { first: 0, why: Why::SmallerChange }));

        let b_stopped = Finding {
            relation: Relation::Copies,
            level: Level::Clash,
            panes: [pane("fake_a"), pane("fake_b")],
            sides: [side("fake_a", true, 4, 1), side("fake_b", false, 10, 2)],
            files: vec![lines("src/auth.ts")],
        };
        let changes = al.apply(&[b_stopped], &none(), 200);
        assert_eq!(changes, vec![Change::Updated(id_of("fake_a", "fake_b"))]);
        assert_eq!(al.visible()[0].advice, Some(Advice { first: 1, why: Why::AlreadyDone }));
    }

    #[test]
    fn same_file_alert_has_no_advice() {
        let mut al = Alerts::default();
        al.apply(&[finding(("fake_a", "fake_b"), Level::SameFile, vec![apart("src/auth.ts")])], &none(), 100);
        assert_eq!(al.visible()[0].advice, None);
    }

    #[test]
    fn clear_all_resolves_visible_and_forgets_hidden() {
        let mut al = Alerts::default();
        al.apply(
            &[
                finding(("fake_a", "fake_b"), Level::Clash, vec![lines("src/auth.ts")]),
                finding(("fake_c", "fake_d"), Level::Clash, vec![lines("src/db.ts")]),
            ],
            &none(),
            100,
        );
        let ignored = id_of("fake_a", "fake_b");
        al.ignore(&ignored);

        let changes = al.clear_all();
        assert_eq!(changes, vec![Change::Resolved(id_of("fake_c", "fake_d"))]);
        assert!(al.visible().is_empty());

        // The ignore is forgotten too: the same clash raises again after the setting comes back.
        let again = al.apply(&[finding(("fake_a", "fake_b"), Level::Clash, vec![lines("src/auth.ts")])], &none(), 200);
        assert_eq!(again, vec![Change::Raised(ignored)]);
    }

    #[test]
    fn visible_lists_newest_first_and_skips_hidden() {
        let mut al = Alerts::default();
        al.apply(&[finding(("fake_a", "fake_b"), Level::Clash, vec![lines("src/a.ts")])], &none(), 100);
        al.apply(&[finding(("fake_c", "fake_d"), Level::Clash, vec![lines("src/b.ts")])], &none(), 200);
        al.apply(
            &[
                finding(("fake_a", "fake_b"), Level::Clash, vec![lines("src/a.ts")]),
                finding(("fake_e", "fake_f"), Level::Clash, vec![lines("src/c.ts")]),
            ],
            &none(),
            300,
        );
        al.ignore(&id_of("fake_c", "fake_d"));
        let order: Vec<String> = al.visible().iter().map(|a| a.id.clone()).collect();
        assert_eq!(order, vec![id_of("fake_e", "fake_f"), id_of("fake_a", "fake_b")]);
    }

    #[test]
    fn files_list_clash_first_and_keep_input_order() {
        let mut al = Alerts::default();
        al.apply(
            &[finding(("fake_a", "fake_b"), Level::Clash, vec![apart("src/new.ts"), lines("src/auth.ts"), apart("src/old.ts")])],
            &none(),
            100,
        );
        let paths: Vec<&str> = al.visible()[0].files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, vec!["src/auth.ts", "src/new.ts", "src/old.ts"]);
    }

    #[test]
    fn hidden_alert_goes_quietly_when_a_pane_closes() {
        let mut al = Alerts::default();
        al.apply(&[finding(("fake_a", "fake_b"), Level::Clash, vec![lines("src/auth.ts")])], &none(), 100);
        al.ignore(&id_of("fake_a", "fake_b"));
        let changes = al.apply(&[], &gone(&["fake_a"]), 200);
        assert!(changes.is_empty(), "a hidden alert has no attention item left to clear");
        assert!(al.records.is_empty());
        assert!(al.remembered.is_empty());
    }

    #[test]
    fn a_pair_of_one_pane_is_never_an_alert() {
        let mut al = Alerts::default();
        let changes = al.apply(&[finding(("fake_a", "fake_a"), Level::Clash, vec![lines("src/auth.ts")])], &none(), 100);
        assert!(changes.is_empty());
        assert!(al.visible().is_empty());
    }
}
