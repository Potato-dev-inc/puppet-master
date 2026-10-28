//! Collision Alert: panes that share a repo, and the look schedule (SPEC-n33 sections 3.3 and 3.7).
//!
//! Pure: no process and no clock. The engine passes `now` in. A group is two or more agent panes
//! whose folders share one git common dir (`RepoFact.key`). A checkout is only looked at while one
//! of its panes is running, or once after a turn ends.

use std::collections::BTreeMap;
use std::path::Path;

use super::model::{
    poll_ms, PaneFact, PaneRun, Relation, RepoFact, BACKOFF_FACTOR, MAX_GROUPS,
    MAX_PANES_PER_GROUP, POLL_MAX_MS, SLOW_LOOKS_TO_PARK, SLOW_LOOK_MS, TURN_END_DEBOUNCE_MS,
    TURN_END_ONLY_MS,
};
use crate::project_path::comparable_project_path;

/// One agent pane and the repo its folder belongs to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    pub pane: PaneFact,
    pub repo: RepoFact,
}

/// Two or more panes that share one repo. Oldest first, at most `MAX_PANES_PER_GROUP`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    pub key: String,
    pub members: Vec<Member>,
}

impl Group {
    fn has_working(&self) -> bool {
        self.members.iter().any(|m| m.pane.run == PaneRun::Working)
    }
}

/// Builds the groups. Keeps only repos with 2+ panes, the oldest `MAX_PANES_PER_GROUP` of each,
/// and at most `MAX_GROUPS`. Groups with a running pane come first (key order otherwise).
pub fn groups(members: Vec<Member>) -> Vec<Group> {
    let mut by_key: BTreeMap<String, Vec<Member>> = BTreeMap::new();
    for member in members {
        by_key.entry(member.repo.key.clone()).or_default().push(member);
    }
    let mut out: Vec<Group> = by_key
        .into_iter()
        .filter(|(_, list)| list.len() >= 2)
        .map(|(key, mut list)| {
            list.sort_by(|a, b| {
                (a.pane.created_ms, a.pane.pane_id.as_str())
                    .cmp(&(b.pane.created_ms, b.pane.pane_id.as_str()))
            });
            list.truncate(MAX_PANES_PER_GROUP);
            Group { key, members: list }
        })
        .collect();
    // `sort_by_key` is stable: working groups move to the front, the rest keep key order.
    out.sort_by_key(|group| !group.has_working());
    out.truncate(MAX_GROUPS);
    out
}

/// Two panes in the same checkout are `SameFolder`; panes in different checkouts of one repo are
/// `Copies`.
pub fn relation(a: &Member, b: &Member) -> Relation {
    let root_a = comparable_project_path(Path::new(&a.repo.root));
    let root_b = comparable_project_path(Path::new(&b.repo.root));
    if root_a == root_b {
        Relation::SameFolder
    } else {
        Relation::Copies
    }
}

/// What the schedule remembers about one checkout (root folder).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Slot {
    /// When the next regular look is due. `None` until `first_look` or `note_cost`.
    next_due: Option<u64>,
    /// When the last turn ended in this checkout, until a look after the debounce takes it.
    turn_end_at: Option<u64>,
    /// Slow looks in a row (over `SLOW_LOOK_MS`).
    slow_looks: u8,
    /// Until then only turn ends are looked at (after three slow looks).
    turn_only_until: u64,
    /// Until then nothing is looked at (a probe failed, or the folder is not git).
    parked_until: u64,
}

/// Per-checkout look times. Owned by the engine; no clock inside.
#[derive(Debug, Default)]
pub struct Schedule {
    slots: BTreeMap<String, Slot>,
}

impl Schedule {
    pub fn new() -> Schedule {
        Schedule::default()
    }

    /// The checkouts (roots) of `group` that should be looked at now. A checkout is due when:
    /// - one of its panes is `Working`, and its `next_due` has come, and it is not turn-only; or
    /// - a turn ended there at least `TURN_END_DEBOUNCE_MS` ago and the look has not happened.
    ///
    /// A fully parked checkout is never due. A checkout with no look scheduled is not due.
    pub fn due_roots(&self, group: &Group, now: u64) -> Vec<String> {
        let mut due: Vec<String> = Vec::new();
        for member in &group.members {
            let root = member.repo.root.as_str();
            if due.iter().any(|d| d == root) {
                continue;
            }
            let Some(slot) = self.slots.get(root) else { continue };
            if now < slot.parked_until {
                continue;
            }
            let regular = member.pane.run == PaneRun::Working
                && now >= slot.turn_only_until
                && slot.next_due.is_some_and(|at| at <= now);
            let turn_end = slot
                .turn_end_at
                .is_some_and(|at| at + TURN_END_DEBOUNCE_MS <= now);
            if regular || turn_end {
                due.push(root.to_string());
            }
        }
        due
    }

    /// Records a finished look that started at `now` and took `cost_ms`. The next regular look
    /// comes after `clamp(max(POLL, cost x BACKOFF_FACTOR), POLL, POLL_MAX)`. Three slow looks in
    /// a row make the checkout turn-only for `TURN_END_ONLY_MS`. A turn end that is at least the
    /// debounce old is consumed by this look.
    pub fn note_cost(&mut self, root: &str, cost_ms: u64, now: u64) {
        let (_, regular) = poll_ms();
        let slot = self.slot(root);
        let gap = cost_ms
            .saturating_mul(BACKOFF_FACTOR)
            .max(regular)
            .min(POLL_MAX_MS);
        slot.next_due = Some(now + gap);
        if slot.turn_end_at.is_some_and(|at| at + TURN_END_DEBOUNCE_MS <= now) {
            slot.turn_end_at = None;
        }
        if cost_ms > SLOW_LOOK_MS {
            slot.slow_looks = slot.slow_looks.saturating_add(1);
            if slot.slow_looks >= SLOW_LOOKS_TO_PARK {
                slot.turn_only_until = now + TURN_END_ONLY_MS;
                slot.slow_looks = 0;
            }
        } else {
            slot.slow_looks = 0;
        }
    }

    /// A turn ended in this checkout at `now`. The look is due after the debounce.
    pub fn note_turn_end(&mut self, root: &str, now: u64) {
        self.slot(root).turn_end_at = Some(now);
    }

    /// The group formed: the first regular look is `FIRST_POLL_MS` after `now`. The engine calls
    /// this once when a group forms, not on every step.
    pub fn first_look(&mut self, root: &str, now: u64) {
        let (first, _) = poll_ms();
        self.slot(root).next_due = Some(now + first);
    }

    /// Nothing in this checkout is looked at before `until` (a probe failed or it is not git).
    pub fn park(&mut self, root: &str, until: u64) {
        self.slot(root).parked_until = until;
    }

    fn slot(&mut self, root: &str) -> &mut Slot {
        self.slots.entry(root.to_string()).or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(id: &str, created: u64, run: PaneRun, key: &str, root: &str) -> Member {
        Member {
            pane: PaneFact {
                pane_id: id.to_string(),
                agent_id: "fake_a".to_string(),
                label: format!("Fake {id}"),
                cwd: root.to_string(),
                run,
                created_ms: created,
            },
            repo: RepoFact { key: key.to_string(), root: root.to_string(), linked: false },
        }
    }

    fn one_group(members: Vec<Member>) -> Group {
        let mut all = groups(members);
        assert_eq!(all.len(), 1, "expected exactly one group");
        all.remove(0)
    }

    const ROOT: &str = "/fixture/copy-a";
    const OTHER_ROOT: &str = "/fixture/copy-b";

    #[test]
    fn lone_pane_makes_no_group() {
        assert!(groups(vec![member("p1", 1, PaneRun::Working, "repo", ROOT)]).is_empty());
        let apart = vec![
            member("p1", 1, PaneRun::Working, "repo-1", ROOT),
            member("p2", 2, PaneRun::Working, "repo-2", OTHER_ROOT),
        ];
        assert!(groups(apart).is_empty());
    }

    #[test]
    fn seven_panes_are_cut_to_the_oldest_six() {
        let members: Vec<Member> = (1..=7)
            .map(|n| member(&format!("p{n}"), n * 10, PaneRun::Resting, "repo", ROOT))
            .collect();
        let group = one_group(members);
        assert_eq!(group.members.len(), MAX_PANES_PER_GROUP);
        assert_eq!(group.members.len(), 6);
        assert_eq!(group.members[0].pane.pane_id, "p1");
        assert_eq!(group.members[5].pane.pane_id, "p6");
        assert!(group.members.iter().all(|m| m.pane.pane_id != "p7"));
    }

    #[test]
    fn five_groups_are_cut_to_four_keeping_the_running_one() {
        let mut members = Vec::new();
        for key in ["k0", "k1", "k2", "k3"] {
            members.push(member(&format!("{key}-a"), 1, PaneRun::Resting, key, ROOT));
            members.push(member(&format!("{key}-b"), 2, PaneRun::Resting, key, ROOT));
        }
        // The fifth group sorts last by key, but it is the only one with a running pane.
        members.push(member("k4-a", 1, PaneRun::Working, "k4", ROOT));
        members.push(member("k4-b", 2, PaneRun::Resting, "k4", ROOT));
        let all = groups(members);
        assert_eq!(all.len(), MAX_GROUPS);
        assert_eq!(all.len(), 4);
        let keys: Vec<&str> = all.iter().map(|g| g.key.as_str()).collect();
        assert_eq!(keys, vec!["k4", "k0", "k1", "k2"]);
        assert!(all.iter().all(|g| g.key != "k3"));
    }

    #[test]
    fn same_root_is_same_folder_and_different_root_is_copies() {
        let dir = std::env::temp_dir();
        let plain = dir.to_string_lossy().to_string();
        let with_slash = format!("{}/", plain.replace('\\', "/").trim_end_matches('/'));
        let a = member("p1", 1, PaneRun::Working, "repo", &plain);
        let same = member("p2", 2, PaneRun::Working, "repo", &with_slash);
        let other = member("p3", 3, PaneRun::Working, "repo", OTHER_ROOT);
        assert_eq!(relation(&a, &same), Relation::SameFolder);
        assert_eq!(relation(&a, &other), Relation::Copies);
        assert_eq!(relation(&other, &a), Relation::Copies);
    }

    #[test]
    fn due_only_with_a_working_pane() {
        let (first, _) = poll_ms();
        let mut schedule = Schedule::new();
        schedule.first_look(ROOT, 0);
        let resting = one_group(vec![
            member("p1", 1, PaneRun::Resting, "repo", ROOT),
            member("p2", 2, PaneRun::Resting, "repo", ROOT),
        ]);
        assert!(schedule.due_roots(&resting, first + 1_000).is_empty());

        let running = one_group(vec![
            member("p1", 1, PaneRun::Working, "repo", ROOT),
            member("p2", 2, PaneRun::Resting, "repo", ROOT),
        ]);
        assert_eq!(schedule.due_roots(&running, first), vec![ROOT.to_string()]);
    }

    #[test]
    fn first_look_then_one_regular_gap() {
        // Default gaps: first look at 8 s, then every 20 s (the e2e override shortens both).
        let (first, regular) = poll_ms();
        let group = one_group(vec![
            member("p1", 1, PaneRun::Working, "repo", ROOT),
            member("p2", 2, PaneRun::Resting, "repo", ROOT),
        ]);
        let mut schedule = Schedule::new();
        schedule.first_look(ROOT, 0);
        assert!(schedule.due_roots(&group, first - 1).is_empty());
        assert_eq!(schedule.due_roots(&group, first), vec![ROOT.to_string()]);

        schedule.note_cost(ROOT, 200, first);
        assert!(schedule.due_roots(&group, first + regular - 1).is_empty());
        assert_eq!(schedule.due_roots(&group, first + regular), vec![ROOT.to_string()]);
    }

    #[test]
    fn backoff_follows_the_cost_within_the_limits() {
        let (_, regular) = poll_ms();
        let group = one_group(vec![
            member("p1", 1, PaneRun::Working, "repo", ROOT),
            member("p2", 2, PaneRun::Resting, "repo", ROOT),
        ]);
        let cases: [(u64, u64); 3] = [
            // 200 ms x 20 = 4 s, so the regular gap (20 s) wins.
            (200, regular),
            // 2 s x 20 = 40 s.
            (2_000, 40_000),
            // 10 s x 20 = 200 s, capped at POLL_MAX_MS (2 min).
            (10_000, POLL_MAX_MS),
        ];
        for (cost, gap) in cases {
            let mut schedule = Schedule::new();
            schedule.note_cost(ROOT, cost, 0);
            assert!(schedule.due_roots(&group, gap - 1).is_empty(), "cost {cost} looked too early");
            assert_eq!(schedule.due_roots(&group, gap), vec![ROOT.to_string()], "cost {cost}");
        }
    }

    #[test]
    fn three_slow_looks_park_the_checkout_to_turn_ends() {
        let group = one_group(vec![
            member("p1", 1, PaneRun::Working, "repo", ROOT),
            member("p2", 2, PaneRun::Working, "repo", ROOT),
        ]);
        let mut schedule = Schedule::new();
        schedule.first_look(ROOT, 0);
        let slow = SLOW_LOOK_MS + 500;

        // Two slow looks: still regular looks.
        schedule.note_cost(ROOT, slow, 0);
        schedule.note_cost(ROOT, slow, 100_000);
        let next = schedule.slots[ROOT].next_due.unwrap();
        assert_eq!(schedule.due_roots(&group, next), vec![ROOT.to_string()]);

        // The third slow look parks it: no regular look, even when the gap has passed.
        schedule.note_cost(ROOT, slow, next);
        let later = next + 10 * POLL_MAX_MS;
        assert!(schedule.due_roots(&group, later).is_empty());
    }

    #[test]
    fn turn_end_is_due_after_the_debounce_even_when_parked() {
        let group = one_group(vec![
            member("p1", 1, PaneRun::Working, "repo", ROOT),
            member("p2", 2, PaneRun::Resting, "repo", ROOT),
        ]);
        let mut schedule = Schedule::new();
        let slow = SLOW_LOOK_MS + 500;
        schedule.note_cost(ROOT, slow, 0);
        schedule.note_cost(ROOT, slow, 100_000);
        schedule.note_cost(ROOT, slow, 200_000);

        let turn = 1_000_000;
        schedule.note_turn_end(ROOT, turn);
        assert!(schedule.due_roots(&group, turn + TURN_END_DEBOUNCE_MS - 1).is_empty());
        assert_eq!(
            schedule.due_roots(&group, turn + TURN_END_DEBOUNCE_MS),
            vec![ROOT.to_string()]
        );
    }

    #[test]
    fn a_look_after_the_debounce_consumes_the_turn_end() {
        let group = one_group(vec![
            member("p1", 1, PaneRun::Resting, "repo", ROOT),
            member("p2", 2, PaneRun::Resting, "repo", ROOT),
        ]);
        let mut schedule = Schedule::new();
        schedule.note_turn_end(ROOT, 5_000);
        assert_eq!(schedule.due_roots(&group, 7_000), vec![ROOT.to_string()]);
        schedule.note_cost(ROOT, 100, 7_000);
        assert!(schedule.due_roots(&group, 60_000).is_empty());
    }

    #[test]
    fn a_turn_end_during_the_debounce_waits_for_its_own_look() {
        let group = one_group(vec![
            member("p1", 1, PaneRun::Working, "repo", ROOT),
            member("p2", 2, PaneRun::Resting, "repo", ROOT),
        ]);
        let mut schedule = Schedule::new();
        schedule.first_look(ROOT, 0);
        schedule.note_turn_end(ROOT, 7_000);
        // A look that started inside the debounce does not take the turn end.
        schedule.note_cost(ROOT, 100, 7_500);
        assert_eq!(schedule.due_roots(&group, 9_000), vec![ROOT.to_string()]);
    }

    #[test]
    fn a_parked_checkout_is_never_due() {
        let group = one_group(vec![
            member("p1", 1, PaneRun::Working, "repo", ROOT),
            member("p2", 2, PaneRun::Working, "repo", ROOT),
        ]);
        let mut schedule = Schedule::new();
        schedule.first_look(ROOT, 0);
        schedule.park(ROOT, 300_000);
        assert!(schedule.due_roots(&group, 8_000).is_empty());
        schedule.note_turn_end(ROOT, 9_000);
        assert!(schedule.due_roots(&group, 20_000).is_empty());
        assert_eq!(schedule.due_roots(&group, 300_000), vec![ROOT.to_string()]);
    }

    #[test]
    fn a_checkout_without_a_schedule_is_not_due() {
        let group = one_group(vec![
            member("p1", 1, PaneRun::Working, "repo", ROOT),
            member("p2", 2, PaneRun::Working, "repo", ROOT),
        ]);
        assert!(Schedule::new().due_roots(&group, u64::MAX / 2).is_empty());
    }
}
