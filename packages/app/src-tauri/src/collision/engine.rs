//! Collision Alert: one pure step of the watch, run by the owner thread (SPEC-n33 section 3.7).
//!
//! `Engine::step` takes the pane facts, the commands of the last second and a `Probe`, and returns
//! what changed. It has no clock (the caller passes `now_ms`) and reads git only through the probe.
//! The live layer owns one `Engine` on the `collision-watch` thread.
//!
//! One step, in order:
//! 1. The commands. `SetEnabled(false)` resolves every alert and drops all state; nothing is probed
//!    while off.
//! 2. The panes: each folder's repo (`repo_of`, cached for `REPO_CACHE_MS`), each run change (a turn
//!    end tells the schedule), and the panes that left.
//! 3. The groups (two or more panes per repo). A checkout whose group has a running pane is armed
//!    for its first look (`FIRST_POLL_MS`).
//! 4. The due checkouts get one `probe.sample` each. A failed look parks its checkout.
//! 5. After a look, or when a pane left: every pair of every group is compared on the latest samples
//!    and `alerts::apply` runs once. Without a look nothing is compared, so an idle step costs nothing.
//!
//! Step 5 compares every pair on a look, not only the pairs that changed. Then `alerts` counts its
//! misses in looks ("gone after 2 looks in a row"), and an unchanged pair gives the same overlap.
//! Its hunks come from a cache keyed by (checkout, file, modified time), so it reads nothing new.

use std::collections::{BTreeMap, BTreeSet};

use super::alerts::{Alerts, Finding};
use super::compare::{self, SideInput};
use super::groups::{self, Group, Member, Schedule};
use super::model::{
    Alert, FileHunks, PaneFact, PaneRun, Relation, RepoFact, Sample, SideFacts, TouchKind, Touched,
    PROBE_TIMEOUT_MS, REPO_CACHE_MS,
};
use super::probe::{Probe, ProbeError};

pub use super::alerts::Change;

/// A checkout whose git call failed is left alone this long (not a git folder, or another failure).
const FAILED_PARK_MS: u64 = 5 * 60_000;

/// A command for the engine, from the commands of the last second.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cmd {
    /// The person tapped `Ignore` on an alert id.
    Ignore(String),
    /// The setting changed.
    SetEnabled(bool),
}

/// What one step produced: the changes, and every visible alert (for the snapshot).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Out {
    pub changes: Vec<Change>,
    pub alerts: Vec<Alert>,
}

/// The engine's state. One owner, the watch thread; nothing here is shared.
#[derive(Debug)]
pub struct Engine {
    enabled: bool,
    alerts: Alerts,
    schedule: Schedule,
    /// Folder -> the repo it belongs to, and when it was read.
    repos: BTreeMap<String, RepoSlot>,
    /// Pane id -> what the engine saw last.
    panes: BTreeMap<String, PaneState>,
    /// Checkout root -> its baseline and its last look.
    roots: BTreeMap<String, RootState>,
    /// Checkouts whose first look is armed (their group has a running pane).
    armed: BTreeSet<String>,
    /// (checkout, file, modified time) -> the file's changed lines, or why they could not be read.
    hunks: HunkCache,
    /// Pane id -> the family it belongs to (the id of the family's first pane). Panes of one family are a
    /// source and its Branch from here copies. A copy starts with the save point's files, so those files are
    /// new in both sides without either agent having made them: such a pair is never compared.
    kin: BTreeMap<String, String>,
}

type HunkCache = BTreeMap<(String, String, u64), Result<FileHunks, ProbeError>>;

#[derive(Clone, Debug)]
struct RepoSlot {
    fact: Option<RepoFact>,
    read_at: u64,
}

#[derive(Clone, Copy, Debug)]
struct PaneState {
    run: PaneRun,
    /// When the latest working stretch began. Kept after the pane stops.
    since: Option<u64>,
    /// When that stretch ended. `None` while the pane is working.
    stopped_at: Option<u64>,
}

#[derive(Clone, Debug, Default)]
struct RootState {
    /// The start commit the checkout is compared from. Empty means HEAD (no baseline yet).
    start: String,
    /// Set by `StartLost`: the copy's own base commit is not asked for again.
    lost_base: bool,
    /// The first good look has taken `deleted_at_first_look`.
    baseline_done: bool,
    /// Files already deleted at the first look. A deletion of one of them is not blamed on a pane.
    deleted_at_first_look: BTreeSet<String>,
    /// The last good look.
    sample: Option<Sample>,
    /// When the checkout first had a running pane (the "started first" time).
    first_working: Option<u64>,
}

impl Engine {
    pub fn new() -> Engine {
        Engine {
            enabled: true,
            alerts: Alerts::default(),
            schedule: Schedule::new(),
            repos: BTreeMap::new(),
            panes: BTreeMap::new(),
            roots: BTreeMap::new(),
            armed: BTreeSet::new(),
            hunks: BTreeMap::new(),
            kin: BTreeMap::new(),
        }
    }

    /// Tells the engine which panes are copies of each other: `links` are (source pane, copy pane) pairs.
    /// Panes that are linked, directly or through others (two copies of one source), form a family, and the
    /// members of a family are not compared with each other. The list replaces the previous one.
    pub fn set_links(&mut self, links: &[(String, String)]) {
        let mut family: BTreeMap<String, String> = BTreeMap::new();
        for (from, to) in links {
            if from.is_empty() || to.is_empty() || from == to {
                continue;
            }
            let a = family_of(&family, from);
            let b = family_of(&family, to);
            if a != b {
                // The smaller id leads, so the same links always make the same families.
                let (lead, other) = if a < b { (a, b) } else { (b, a) };
                for value in family.values_mut().filter(|v| **v == other) {
                    *value = lead.clone();
                }
                family.insert(other, lead.clone());
                family.insert(from.clone(), lead.clone());
                family.insert(to.clone(), lead);
            }
        }
        self.kin = family;
    }

    fn are_kin(&self, a: &str, b: &str) -> bool {
        matches!((self.kin.get(a), self.kin.get(b)), (Some(x), Some(y)) if x == y)
    }

    /// One step of the watch. See the module doc for the order.
    pub fn step(&mut self, panes: &[PaneFact], cmds: &[Cmd], probe: &dyn Probe, now_ms: u64) -> Out {
        let mut changes = Vec::new();
        for cmd in cmds {
            match cmd {
                Cmd::Ignore(id) => {
                    self.alerts.ignore(id);
                }
                Cmd::SetEnabled(true) => self.enabled = true,
                Cmd::SetEnabled(false) => {
                    changes.extend(self.alerts.clear_all());
                    // Everything is dropped except what the engine cannot learn again by looking:
                    // when each pane began working (the "same folder" window starts there) and which
                    // panes are copies of each other.
                    let (panes, kin) = (std::mem::take(&mut self.panes), std::mem::take(&mut self.kin));
                    *self = Engine::new();
                    self.enabled = false;
                    self.panes = panes;
                    self.kin = kin;
                }
            }
        }
        if !self.enabled {
            // Off: no git is read, but the run state of the panes is still followed. Without it a pane
            // that began working while off would count as starting at the moment of switching on, and
            // the files it changed before then would fall outside its window (no alert, ever).
            self.follow_panes_while_off(panes, now_ms);
            return Out { changes, alerts: Vec::new() };
        }

        // 2. The panes. A pane that is not in the list any more has left.
        let present: BTreeSet<&str> = panes.iter().map(|pane| pane.pane_id.as_str()).collect();
        let gone: BTreeSet<String> =
            self.panes.keys().filter(|id| !present.contains(id.as_str())).cloned().collect();
        for id in &gone {
            self.panes.remove(id);
        }
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        let mut members = Vec::new();
        for fact in panes {
            if !seen.insert(fact.pane_id.as_str()) {
                continue;
            }
            let repo = self.repo_of(probe, &fact.cwd, now_ms);
            self.track_pane(fact, repo.as_ref().map(|r| r.root.as_str()), now_ms);
            if let Some(repo) = repo {
                members.push(Member { pane: fact.clone(), repo });
            }
        }

        // 3. Groups, and the first look of a checkout whose group has a running pane.
        let grouped = groups::groups(members);
        let in_group: BTreeSet<String> =
            grouped.iter().flat_map(|g| g.members.iter()).map(|m| m.repo.root.clone()).collect();
        self.armed.retain(|root| in_group.contains(root));
        for group in &grouped {
            if !group.members.iter().any(|m| m.pane.run == PaneRun::Working) {
                continue;
            }
            for member in &group.members {
                if self.armed.insert(member.repo.root.clone()) {
                    self.schedule.first_look(&member.repo.root, now_ms);
                }
            }
        }

        // 4. Each due checkout gets one look.
        let mut due: BTreeSet<String> = BTreeSet::new();
        for group in &grouped {
            due.extend(self.schedule.due_roots(group, now_ms));
        }
        let repos_by_root: BTreeMap<String, RepoFact> = grouped
            .iter()
            .flat_map(|g| g.members.iter())
            .map(|m| (m.repo.root.clone(), m.repo.clone()))
            .collect();
        let mut looked = false;
        for root in &due {
            if let Some(repo) = repos_by_root.get(root) {
                looked = true;
                self.look(probe, root, repo, now_ms);
            }
        }

        // 5. After a look, or a pane leaving: compare every pair once and apply the alerts.
        if looked || !gone.is_empty() {
            let findings = self.findings(probe, &grouped, now_ms);
            changes.extend(self.alerts.apply(&findings, &gone, now_ms));
        }
        Out { changes, alerts: self.alerts.visible().into_iter().cloned().collect() }
    }

    /// The pane bookkeeping of a step, without the repo lookup: leavers dropped, run changes noted.
    fn follow_panes_while_off(&mut self, panes: &[PaneFact], now: u64) {
        let present: BTreeSet<&str> = panes.iter().map(|pane| pane.pane_id.as_str()).collect();
        self.panes.retain(|id, _| present.contains(id.as_str()));
        for fact in panes {
            self.track_pane(fact, None, now);
        }
    }

    /// The repo of a folder. Cached for `REPO_CACHE_MS`, a folder with no repo included.
    fn repo_of(&mut self, probe: &dyn Probe, cwd: &str, now: u64) -> Option<RepoFact> {
        if let Some(slot) = self.repos.get(cwd) {
            if now.saturating_sub(slot.read_at) < REPO_CACHE_MS {
                return slot.fact.clone();
            }
        }
        let fact = probe.repo_of(cwd);
        self.repos.insert(cwd.to_string(), RepoSlot { fact: fact.clone(), read_at: now });
        fact
    }

    /// Records a pane's run state. A working pane marks its checkout's first running time. A turn
    /// that ends tells the schedule, so the checkout is looked at after the debounce.
    fn track_pane(&mut self, fact: &PaneFact, root: Option<&str>, now: u64) {
        let next = match self.panes.get(&fact.pane_id).copied() {
            None => PaneState {
                run: fact.run,
                since: (fact.run == PaneRun::Working).then_some(now),
                stopped_at: None,
            },
            Some(prev) if prev.run == fact.run => prev,
            Some(prev) if fact.run == PaneRun::Working => PaneState {
                run: fact.run,
                since: Some(now),
                stopped_at: None,
            },
            Some(prev) => {
                if let Some(root) = root {
                    self.schedule.note_turn_end(root, now);
                }
                PaneState { run: fact.run, since: prev.since, stopped_at: Some(now) }
            }
        };
        if let Some(root) = root {
            let state = self.roots.entry(root.to_string()).or_default();
            if next.run == PaneRun::Working && state.first_working.is_none() {
                state.first_working = Some(now);
            }
        }
        self.panes.insert(fact.pane_id.clone(), next);
    }

    /// One look at a checkout (`probe.sample`), and what it means for the baseline and the schedule.
    fn look(&mut self, probe: &dyn Probe, root: &str, repo: &RepoFact, now: u64) {
        let state = self.roots.entry(root.to_string()).or_default();
        let base = if state.lost_base { None } else { probe.start_of(repo) };
        let start = base.unwrap_or_else(|| state.start.clone());
        match probe.sample(root, &start) {
            Ok(sample) => {
                // The first good look fixes the baseline: the start used, or HEAD when none is known.
                let baseline = if start.is_empty() { sample.head.clone() } else { start };
                if state.start != baseline {
                    state.start = baseline;
                    self.hunks.retain(|key, _| key.0 != root);
                }
                if !state.baseline_done {
                    state.deleted_at_first_look = sample
                        .files
                        .iter()
                        .filter(|t| t.kind == TouchKind::Deleted)
                        .map(|t| t.path.clone())
                        .collect();
                    state.baseline_done = true;
                }
                // Cached hunks of a file this look no longer lists at that time are dropped.
                let listed: BTreeSet<(String, u64)> =
                    sample.files.iter().map(|t| (t.path.clone(), t.mtime_ms.unwrap_or(0))).collect();
                self.hunks.retain(|key, _| key.0 != root || listed.contains(&(key.1.clone(), key.2)));
                self.schedule.note_cost(root, sample.cost_ms, now);
                state.sample = Some(sample);
            }
            Err(ProbeError::StartLost) => {
                // The start is gone: compare from HEAD from now on, and take a new baseline.
                state.start.clear();
                state.lost_base = true;
                state.baseline_done = false;
                state.deleted_at_first_look.clear();
                state.sample = None;
                self.hunks.retain(|key, _| key.0 != root);
            }
            // The git call was killed at its deadline: a slow look.
            Err(ProbeError::TooSlow) => self.schedule.note_cost(root, PROBE_TIMEOUT_MS, now),
            Err(ProbeError::NotGit) | Err(ProbeError::Failed(_)) => {
                self.schedule.park(root, now + FAILED_PARK_MS);
            }
        }
    }

    /// The findings of every pair of every group, from the latest samples. A pair that does not
    /// overlap has no finding.
    fn findings(&mut self, probe: &dyn Probe, grouped: &[Group], now: u64) -> Vec<Finding> {
        let mut out = Vec::new();
        for group in grouped {
            for (i, a) in group.members.iter().enumerate() {
                for b in &group.members[i + 1..] {
                    if self.are_kin(&a.pane.pane_id, &b.pane.pane_id) {
                        continue;
                    }
                    let found = pair_finding(&self.roots, &self.panes, &mut self.hunks, probe, a, b, now);
                    out.extend(found);
                }
            }
        }
        out
    }
}

/// The family a pane is in so far (itself when it has none).
fn family_of(family: &BTreeMap<String, String>, pane: &str) -> String {
    family.get(pane).cloned().unwrap_or_else(|| pane.to_string())
}

/// The overlap of two panes, as a finding. `None` when they do not overlap, or a side has no look yet.
fn pair_finding(
    roots: &BTreeMap<String, RootState>,
    panes: &BTreeMap<String, PaneState>,
    hunks: &mut HunkCache,
    probe: &dyn Probe,
    a: &Member,
    b: &Member,
    now: u64,
) -> Option<Finding> {
    let root_a = roots.get(&a.repo.root)?;
    let root_b = roots.get(&b.repo.root)?;
    let sample_a = root_a.sample.as_ref()?;
    let sample_b = root_b.sample.as_ref()?;
    let relation = groups::relation(a, b);
    // Only the pane's own changes count. `belongs` is applied before each side is built.
    let touched_a: Vec<Touched> = sample_a
        .files
        .iter()
        .filter(|t| compare::belongs(t, &a.pane, &root_a.deleted_at_first_look))
        .cloned()
        .collect();
    let touched_b: Vec<Touched> = sample_b
        .files
        .iter()
        .filter(|t| compare::belongs(t, &b.pane, &root_b.deleted_at_first_look))
        .cloned()
        .collect();
    let files = match relation {
        Relation::Copies => {
            let side_a = SideInput {
                pane: &a.pane,
                root: &a.repo.root,
                start: &root_a.start,
                touched: &touched_a,
            };
            let side_b = SideInput {
                pane: &b.pane,
                root: &b.repo.root,
                start: &root_b.start,
                touched: &touched_b,
            };
            compare::copies(&side_a, &side_b, &mut |side: &SideInput, rel: &str| {
                cached_hunks(hunks, probe, side, rel)
            })
        }
        Relation::SameFolder => {
            let window = shared_window(panes.get(&a.pane.pane_id)?, panes.get(&b.pane.pane_id)?, now)?;
            compare::shared(&a.pane, &b.pane, window, &sample_a.files)
        }
    };
    let level = compare::level_of(&files)?;
    Some(Finding {
        relation,
        level,
        panes: [a.pane.clone(), b.pane.clone()],
        sides: [side_facts(a, root_a, &touched_a), side_facts(b, root_b, &touched_b)],
        files,
    })
}

/// What the advice rules need about one side.
fn side_facts(member: &Member, root: &RootState, touched: &[Touched]) -> SideFacts {
    SideFacts {
        pane_id: member.pane.pane_id.clone(),
        working: member.pane.run == PaneRun::Working,
        files: u32::try_from(touched.len()).unwrap_or(u32::MAX),
        started_ms: root.first_working.unwrap_or(member.pane.created_ms),
    }
}

/// The time both panes were working: from the later start to the earlier stop (or to now while
/// still working). `None` when a pane has not been seen working yet.
fn shared_window(a: &PaneState, b: &PaneState, now: u64) -> Option<(u64, u64)> {
    let from = a.since?.max(b.since?);
    let to = a.stopped_at.unwrap_or(now).min(b.stopped_at.unwrap_or(now));
    Some((from, to))
}

/// The changed lines of one file of one side, read once per (checkout, file, modified time).
fn cached_hunks(
    cache: &mut HunkCache,
    probe: &dyn Probe,
    side: &SideInput,
    rel: &str,
) -> Result<FileHunks, ProbeError> {
    let mtime = side.touched.iter().find(|t| t.path == rel).and_then(|t| t.mtime_ms).unwrap_or(0);
    let key = (side.root.to_string(), rel.to_string(), mtime);
    if let Some(hit) = cache.get(&key) {
        return hit.clone();
    }
    let got = probe.hunks(side.root, side.start, rel);
    cache.insert(key, got.clone());
    got
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collision::model::{poll_ms, AlertId, FileOverlap, How, Level, Span, POLL_MAX_MS};
    use crate::collision::probe::FakeProbe;

    const ROOT_A: &str = r"C:\Users\you\copy-a";
    const ROOT_B: &str = r"C:\Users\you\copy-b";
    const ROOT_C: &str = r"C:\Users\you\copy-c";
    const A: &str = "fake_a";
    const B: &str = "fake_b";
    const C: &str = "fake_c";
    const AUTH: &str = "src/auth.ts";

    fn pane(id: &str, root: &str, run: PaneRun, created: u64) -> PaneFact {
        PaneFact {
            pane_id: id.to_string(),
            agent_id: "fake_agent".to_string(),
            label: format!("Fake {id}"),
            cwd: root.to_string(),
            run,
            created_ms: created,
        }
    }

    /// A probe where each root is a checkout of one repo (key `repo`), starting at commit `s1`.
    fn world(roots: &[&str]) -> FakeProbe {
        let probe = FakeProbe::new();
        for root in roots {
            probe.set_repo(root, RepoFact { key: "repo".to_string(), root: root.to_string(), linked: false });
            probe.set_start(root, "s1");
        }
        probe
    }

    fn touched(path: &str, mtime: u64) -> Touched {
        Touched { path: path.to_string(), kind: TouchKind::Modified, mtime_ms: Some(mtime) }
    }

    fn look(files: Vec<Touched>) -> Sample {
        Sample { head: "h1".to_string(), files, truncated: false, cost_ms: 50 }
    }

    fn lines(spans: &[(u32, u32)]) -> FileHunks {
        FileHunks {
            spans: spans.iter().map(|&(start, end)| Span { start, end }).collect(),
            binary: false,
            cut: false,
        }
    }

    fn run(engine: &mut Engine, panes: &[PaneFact], probe: &FakeProbe, now: u64) -> Out {
        engine.step(panes, &[], probe, now)
    }

    fn pair_id() -> String {
        AlertId::of(&[A, B])
    }

    fn added(path: &str, mtime: u64) -> Touched {
        Touched { path: path.to_string(), kind: TouchKind::Added, mtime_ms: Some(mtime) }
    }

    /// Two copies of one repo where both sides list the same untracked file: what a Branch from here copy looks like
    /// (the copy is seeded with the save point's files).
    fn seeded_copy_world() -> (FakeProbe, [PaneFact; 2]) {
        let probe = world(&[ROOT_A, ROOT_B]);
        probe.push_sample(ROOT_A, Ok(look(vec![added("notes.txt", 5_000)])));
        probe.push_sample(ROOT_B, Ok(look(vec![added("notes.txt", 5_000)])));
        let panes = [pane(A, ROOT_A, PaneRun::Working, 1_000), pane(B, ROOT_B, PaneRun::Working, 1_000)];
        (probe, panes)
    }

    #[test]
    fn two_unrelated_copies_that_both_have_a_new_file_clash() {
        let (first, _) = poll_ms();
        let (probe, panes) = seeded_copy_world();
        let mut engine = Engine::new();
        run(&mut engine, &panes, &probe, 0);
        let out = run(&mut engine, &panes, &probe, first);
        assert_eq!(out.changes, vec![Change::Raised(pair_id())]);
        assert_eq!(out.alerts[0].files[0].how, How::BothNew);
    }

    #[test]
    fn a_branch_copy_and_its_source_are_not_compared() {
        let (first, _) = poll_ms();
        let (probe, panes) = seeded_copy_world();
        let mut engine = Engine::new();
        engine.set_links(&[(A.to_string(), B.to_string())]);
        run(&mut engine, &panes, &probe, 0);
        let out = run(&mut engine, &panes, &probe, first);
        assert!(out.changes.is_empty() && out.alerts.is_empty(), "no alert for a Branch from here copy");
    }

    #[test]
    fn two_copies_of_one_source_and_a_copy_of_a_copy_are_one_family() {
        let mut engine = Engine::new();
        engine.set_links(&[
            ("src".to_string(), "c1".to_string()),
            ("src".to_string(), "c2".to_string()),
            ("c2".to_string(), "c3".to_string()),
            ("x".to_string(), "y".to_string()),
        ]);
        for (a, b) in [("src", "c1"), ("c1", "c2"), ("c1", "c3"), ("src", "c3")] {
            assert!(engine.are_kin(a, b), "{a} and {b}");
        }
        assert!(!engine.are_kin("src", "x") && !engine.are_kin("c3", "y"));
        assert!(!engine.are_kin("src", "stranger") && !engine.are_kin("stranger", "other"));
        engine.set_links(&[]);
        assert!(!engine.are_kin("src", "c1"), "a new list replaces the old one");
    }

    #[test]
    fn idle_groups_and_lone_panes_are_not_looked_at() {
        let probe = world(&[ROOT_A, ROOT_B]);
        let mut engine = Engine::new();
        let idle = [pane(A, ROOT_A, PaneRun::Resting, 1_000), pane(B, ROOT_B, PaneRun::Resting, 1_000)];
        for now in (0..=59_000u64).step_by(1_000) {
            let out = run(&mut engine, &idle, &probe, now);
            assert!(out.changes.is_empty() && out.alerts.is_empty());
        }
        let calls = probe.calls();
        assert_eq!(calls.repo_of, 2, "the repo cache answers every later step");
        assert_eq!((calls.sample, calls.start_of, calls.hunks), (0, 0, 0));

        // The repo cache is read again once it is a minute old.
        run(&mut engine, &idle, &probe, 60_000);
        assert_eq!(probe.calls().repo_of, 4);

        // One lone pane that is working: still no look.
        let mut lone = Engine::new();
        let alone = [pane(A, ROOT_A, PaneRun::Working, 1_000)];
        for now in (0..=120_000u64).step_by(1_000) {
            run(&mut lone, &alone, &probe, now);
        }
        assert_eq!(probe.calls().sample, 0);
        assert_eq!(probe.calls().start_of, 0);
    }

    #[test]
    fn first_look_at_eight_seconds_then_every_twenty() {
        let (first, regular) = poll_ms();
        let probe = world(&[ROOT_A, ROOT_B]);
        probe.push_sample(ROOT_A, Ok(look(vec![])));
        let panes = [pane(A, ROOT_A, PaneRun::Working, 1_000), pane(B, ROOT_B, PaneRun::Resting, 1_000)];
        let mut engine = Engine::new();
        run(&mut engine, &panes, &probe, 0);
        run(&mut engine, &panes, &probe, first - 1);
        assert_eq!(probe.calls().sample, 0);
        run(&mut engine, &panes, &probe, first);
        assert_eq!(probe.calls().sample, 1);
        run(&mut engine, &panes, &probe, first + regular - 1);
        assert_eq!(probe.calls().sample, 1);
        run(&mut engine, &panes, &probe, first + regular);
        assert_eq!(probe.calls().sample, 2);
    }

    #[test]
    fn a_slow_look_backs_off() {
        let (first, regular) = poll_ms();
        let probe = world(&[ROOT_A, ROOT_B]);
        // 2 s of git: the next gap is 20 x 2 s = 40 s (not slow, so no parking).
        probe.push_sample(ROOT_A, Ok(Sample { cost_ms: 2_000, ..look(vec![]) }));
        let panes = [pane(A, ROOT_A, PaneRun::Working, 1_000), pane(B, ROOT_B, PaneRun::Resting, 1_000)];
        let mut engine = Engine::new();
        run(&mut engine, &panes, &probe, 0);
        run(&mut engine, &panes, &probe, first);
        assert_eq!(probe.calls().sample, 1);
        run(&mut engine, &panes, &probe, first + 2 * regular - 1);
        assert_eq!(probe.calls().sample, 1, "the gap grew to 40 s");
        run(&mut engine, &panes, &probe, first + 2 * regular);
        assert_eq!(probe.calls().sample, 2);
    }

    #[test]
    fn a_turn_end_is_looked_at_two_seconds_later() {
        let (first, _) = poll_ms();
        let probe = world(&[ROOT_A, ROOT_B]);
        probe.push_sample(ROOT_A, Ok(look(vec![])));
        let working = [pane(A, ROOT_A, PaneRun::Working, 1_000), pane(B, ROOT_B, PaneRun::Resting, 1_000)];
        let resting = [pane(A, ROOT_A, PaneRun::Resting, 1_000), pane(B, ROOT_B, PaneRun::Resting, 1_000)];
        let mut engine = Engine::new();
        run(&mut engine, &working, &probe, 0);
        run(&mut engine, &working, &probe, first);
        assert_eq!(probe.calls().sample, 1);

        let turn_end = first + 2_000;
        run(&mut engine, &resting, &probe, turn_end);
        run(&mut engine, &resting, &probe, turn_end + 1_999);
        assert_eq!(probe.calls().sample, 1);
        run(&mut engine, &resting, &probe, turn_end + 2_000);
        assert_eq!(probe.calls().sample, 2);
    }

    #[test]
    fn two_copies_changing_the_same_lines_raise_one_clash() {
        let (first, regular) = poll_ms();
        let probe = world(&[ROOT_A, ROOT_B]);
        probe.push_sample(ROOT_A, Ok(look(vec![touched(AUTH, 5_000)])));
        probe.push_sample(ROOT_B, Ok(look(vec![touched(AUTH, 5_000)])));
        probe.set_hunks(ROOT_A, AUTH, Ok(lines(&[(50, 50)])));
        probe.set_hunks(ROOT_B, AUTH, Ok(lines(&[(50, 50)])));
        let panes = [pane(A, ROOT_A, PaneRun::Working, 1_000), pane(B, ROOT_B, PaneRun::Working, 1_000)];
        let mut engine = Engine::new();
        assert!(run(&mut engine, &panes, &probe, 0).changes.is_empty());

        let out = run(&mut engine, &panes, &probe, first);
        assert_eq!(out.changes, vec![Change::Raised(pair_id())]);
        assert_eq!(out.alerts.len(), 1);
        assert_eq!(out.alerts[0].level, Level::Clash);
        let line_50 = FileOverlap { path: AUTH.to_string(), how: How::Lines(Span { start: 50, end: 50 }) };
        assert_eq!(out.alerts[0].files, vec![line_50]);
        assert_eq!(probe.calls().hunks, 2);

        // The next look reads the same file at the same time: no new hunk read, no change.
        let out = run(&mut engine, &panes, &probe, first + regular);
        assert!(out.changes.is_empty());
        assert_eq!(probe.calls().hunks, 2, "the hunks came from the cache");
    }

    #[test]
    fn different_files_raise_nothing() {
        let (first, _) = poll_ms();
        let probe = world(&[ROOT_A, ROOT_B]);
        probe.push_sample(ROOT_A, Ok(look(vec![touched("src/a.ts", 5_000)])));
        probe.push_sample(ROOT_B, Ok(look(vec![touched("src/b.ts", 5_000)])));
        let panes = [pane(A, ROOT_A, PaneRun::Working, 1_000), pane(B, ROOT_B, PaneRun::Working, 1_000)];
        let mut engine = Engine::new();
        run(&mut engine, &panes, &probe, 0);
        let out = run(&mut engine, &panes, &probe, first);
        assert!(out.changes.is_empty() && out.alerts.is_empty());
        assert_eq!(probe.calls().hunks, 0);
    }

    #[test]
    fn same_file_far_lines_is_quiet() {
        let (first, _) = poll_ms();
        let probe = world(&[ROOT_A, ROOT_B]);
        probe.push_sample(ROOT_A, Ok(look(vec![touched(AUTH, 5_000)])));
        probe.push_sample(ROOT_B, Ok(look(vec![touched(AUTH, 5_000)])));
        probe.set_hunks(ROOT_A, AUTH, Ok(lines(&[(10, 10)])));
        probe.set_hunks(ROOT_B, AUTH, Ok(lines(&[(80, 80)])));
        let panes = [pane(A, ROOT_A, PaneRun::Working, 1_000), pane(B, ROOT_B, PaneRun::Working, 1_000)];
        let mut engine = Engine::new();
        run(&mut engine, &panes, &probe, 0);
        let out = run(&mut engine, &panes, &probe, first);
        assert_eq!(out.changes, vec![Change::Raised(pair_id())]);
        assert_eq!(out.alerts[0].level, Level::SameFile, "quiet: not a clash");
        assert_eq!(out.alerts[0].advice, None);
        assert_eq!(out.alerts[0].files[0].how, How::Apart);
    }

    #[test]
    fn different_starts_are_same_file_and_lines_are_not_read() {
        let (first, _) = poll_ms();
        let probe = world(&[ROOT_A, ROOT_B]);
        probe.set_start(ROOT_B, "s2");
        probe.push_sample(ROOT_A, Ok(look(vec![touched(AUTH, 5_000)])));
        probe.push_sample(ROOT_B, Ok(look(vec![touched(AUTH, 5_000)])));
        let panes = [pane(A, ROOT_A, PaneRun::Working, 1_000), pane(B, ROOT_B, PaneRun::Working, 1_000)];
        let mut engine = Engine::new();
        run(&mut engine, &panes, &probe, 0);
        let out = run(&mut engine, &panes, &probe, first);
        assert_eq!(out.alerts[0].level, Level::SameFile);
        assert_eq!(out.alerts[0].files[0].how, How::DifferentStart);
        assert_eq!(probe.calls().hunks, 0);
    }

    #[test]
    fn same_folder_both_working_with_a_file_changed_in_the_window_is_a_shared_folder() {
        let (first, _) = poll_ms();
        let probe = world(&[ROOT_A]);
        probe.push_sample(ROOT_A, Ok(look(vec![touched(AUTH, 5_000)])));
        let panes = [pane(A, ROOT_A, PaneRun::Working, 1_000), pane(B, ROOT_A, PaneRun::Working, 1_000)];
        let mut engine = Engine::new();
        run(&mut engine, &panes, &probe, 0);
        let out = run(&mut engine, &panes, &probe, first);
        assert_eq!(out.changes, vec![Change::Raised(pair_id())]);
        assert_eq!(out.alerts[0].relation, Relation::SameFolder);
        assert_eq!(out.alerts[0].level, Level::SharedFolder);
        assert_eq!(out.alerts[0].files[0].how, How::Shared);
    }

    #[test]
    fn a_file_older_than_the_pane_is_not_blamed_on_it() {
        let (first, _) = poll_ms();
        let start = 100_000;
        let panes = [pane(A, ROOT_A, PaneRun::Working, start), pane(B, ROOT_B, PaneRun::Working, start)];

        // Copies: A's auth.ts was changed before pane A existed (the person's own work).
        let probe = world(&[ROOT_A, ROOT_B]);
        probe.push_sample(ROOT_A, Ok(look(vec![touched(AUTH, 1_000)])));
        probe.push_sample(ROOT_B, Ok(look(vec![touched(AUTH, start + 5_000)])));
        let mut engine = Engine::new();
        run(&mut engine, &panes, &probe, start);
        let out = run(&mut engine, &panes, &probe, start + first);
        assert!(out.changes.is_empty() && out.alerts.is_empty());
        assert_eq!(probe.calls().hunks, 0);

        // Same folder: the old change is not inside the window either.
        let probe = world(&[ROOT_A]);
        probe.push_sample(ROOT_A, Ok(look(vec![touched(AUTH, 1_000)])));
        let same = [pane(A, ROOT_A, PaneRun::Working, start), pane(B, ROOT_A, PaneRun::Working, start)];
        let mut engine = Engine::new();
        run(&mut engine, &same, &probe, start);
        let out = run(&mut engine, &same, &probe, start + first);
        assert!(out.changes.is_empty() && out.alerts.is_empty());
    }

    #[test]
    fn one_side_closing_resolves_the_alert() {
        let (first, _) = poll_ms();
        let probe = world(&[ROOT_A, ROOT_B]);
        probe.push_sample(ROOT_A, Ok(look(vec![touched(AUTH, 5_000)])));
        probe.push_sample(ROOT_B, Ok(look(vec![touched(AUTH, 5_000)])));
        probe.set_hunks(ROOT_A, AUTH, Ok(lines(&[(50, 50)])));
        probe.set_hunks(ROOT_B, AUTH, Ok(lines(&[(50, 50)])));
        let panes = [pane(A, ROOT_A, PaneRun::Working, 1_000), pane(B, ROOT_B, PaneRun::Working, 1_000)];
        let mut engine = Engine::new();
        run(&mut engine, &panes, &probe, 0);
        assert_eq!(run(&mut engine, &panes, &probe, first).changes, vec![Change::Raised(pair_id())]);

        let only_a = [panes[0].clone()];
        let out = run(&mut engine, &only_a, &probe, first + 1_000);
        assert_eq!(out.changes, vec![Change::Resolved(pair_id())]);
        assert!(out.alerts.is_empty());
    }

    #[test]
    fn ignore_hides_and_a_new_clash_file_brings_it_back() {
        let (first, regular) = poll_ms();
        let probe = world(&[ROOT_A, ROOT_B]);
        // A has only auth.ts at the first look; from the third look it has db.ts too.
        probe.push_sample(ROOT_A, Ok(look(vec![touched(AUTH, 5_000)])));
        probe.push_sample(ROOT_A, Ok(look(vec![touched(AUTH, 5_000)])));
        probe.push_sample(ROOT_A, Ok(look(vec![touched(AUTH, 5_000), touched("src/db.ts", 6_000)])));
        probe.push_sample(ROOT_B, Ok(look(vec![touched(AUTH, 5_000), touched("src/db.ts", 6_000)])));
        for root in [ROOT_A, ROOT_B] {
            probe.set_hunks(root, AUTH, Ok(lines(&[(50, 50)])));
            probe.set_hunks(root, "src/db.ts", Ok(lines(&[(20, 20)])));
        }
        let panes = [pane(A, ROOT_A, PaneRun::Working, 1_000), pane(B, ROOT_B, PaneRun::Working, 1_000)];
        let mut engine = Engine::new();
        run(&mut engine, &panes, &probe, 0);
        assert_eq!(run(&mut engine, &panes, &probe, first).changes, vec![Change::Raised(pair_id())]);

        let out = engine.step(&panes, &[Cmd::Ignore(pair_id())], &probe, first + 1_000);
        assert!(out.changes.is_empty() && out.alerts.is_empty());

        // The same clash file: it stays hidden.
        let out = run(&mut engine, &panes, &probe, first + regular);
        assert!(out.changes.is_empty() && out.alerts.is_empty());

        // A new clash file: the alert comes back.
        let out = run(&mut engine, &panes, &probe, first + 2 * regular);
        assert_eq!(out.changes, vec![Change::Raised(pair_id())]);
        assert!(out.alerts[0].files.iter().any(|f| f.path == "src/db.ts"));
    }

    #[test]
    fn switching_off_resolves_everything_and_probes_nothing_until_on_again() {
        let (first, regular) = poll_ms();
        let probe = world(&[ROOT_A, ROOT_B]);
        probe.push_sample(ROOT_A, Ok(look(vec![touched(AUTH, 5_000)])));
        probe.push_sample(ROOT_B, Ok(look(vec![touched(AUTH, 5_000)])));
        probe.set_hunks(ROOT_A, AUTH, Ok(lines(&[(50, 50)])));
        probe.set_hunks(ROOT_B, AUTH, Ok(lines(&[(50, 50)])));
        let panes = [pane(A, ROOT_A, PaneRun::Working, 1_000), pane(B, ROOT_B, PaneRun::Working, 1_000)];
        let mut engine = Engine::new();
        run(&mut engine, &panes, &probe, 0);
        run(&mut engine, &panes, &probe, first);

        let out = engine.step(&panes, &[Cmd::SetEnabled(false)], &probe, first + 1_000);
        assert_eq!(out.changes, vec![Change::Resolved(pair_id())]);
        assert!(out.alerts.is_empty());
        let off = probe.calls();
        for now in [first + regular, first + 2 * regular, first + 3 * regular] {
            let out = run(&mut engine, &panes, &probe, now);
            assert!(out.changes.is_empty() && out.alerts.is_empty());
        }
        assert_eq!(probe.calls(), off, "nothing is probed while off");

        // On again: the group is armed afresh and the first look comes FIRST_POLL_MS later. Both
        // checkouts have a running pane, so both are looked at.
        let on = 200_000;
        engine.step(&panes, &[Cmd::SetEnabled(true)], &probe, on);
        assert_eq!(probe.calls().sample, off.sample);
        let out = run(&mut engine, &panes, &probe, on + first);
        assert_eq!(probe.calls().sample, off.sample + 2);
        assert_eq!(out.changes, vec![Change::Raised(pair_id())]);
    }

    #[test]
    fn panes_that_began_working_while_off_still_clash_on_a_file_changed_while_off() {
        let (first, _) = poll_ms();
        let probe = world(&[ROOT_A]);
        // The file was changed while the switch was off (after both panes began working).
        probe.push_sample(ROOT_A, Ok(look(vec![touched(AUTH, 12_000)])));
        let panes = [pane(A, ROOT_A, PaneRun::Working, 1_000), pane(B, ROOT_A, PaneRun::Working, 1_000)];
        let mut engine = Engine::new();
        engine.step(&[], &[Cmd::SetEnabled(false)], &probe, 0);
        // Both panes begin working while off; nothing is probed.
        let out = run(&mut engine, &panes, &probe, 10_000);
        assert!(out.changes.is_empty() && out.alerts.is_empty());
        assert_eq!(probe.calls().sample, 0);
        // Switched on a minute later: the group is armed afresh and the first look raises the alert.
        let on = 70_000;
        engine.step(&panes, &[Cmd::SetEnabled(true)], &probe, on);
        let out = run(&mut engine, &panes, &probe, on + first);
        assert_eq!(out.changes, vec![Change::Raised(pair_id())]);
        assert_eq!(out.alerts[0].relation, Relation::SameFolder);
    }

    #[test]
    fn start_lost_recovers_from_head_without_asking_for_the_base_again() {
        let (first, regular) = poll_ms();
        let probe = world(&[ROOT_A, ROOT_B]);
        probe.push_sample(ROOT_A, Ok(look(vec![touched(AUTH, 5_000)])));
        probe.push_sample(ROOT_A, Err(ProbeError::StartLost));
        probe.push_sample(ROOT_A, Ok(look(vec![touched(AUTH, 5_000)])));
        probe.push_sample(ROOT_B, Ok(look(vec![touched(AUTH, 5_000)])));
        for root in [ROOT_A, ROOT_B] {
            probe.set_hunks(root, AUTH, Ok(lines(&[(50, 50)])));
        }
        let panes = [pane(A, ROOT_A, PaneRun::Working, 1_000), pane(B, ROOT_B, PaneRun::Working, 1_000)];
        let mut engine = Engine::new();
        run(&mut engine, &panes, &probe, 0);
        assert_eq!(run(&mut engine, &panes, &probe, first).changes, vec![Change::Raised(pair_id())]);

        // The base commit is gone: no sample for A this look, so no overlap this look.
        let out = run(&mut engine, &panes, &probe, first + regular);
        assert!(out.changes.is_empty());
        assert_eq!(out.alerts.len(), 1, "one miss does not resolve it");

        // The retry takes a HEAD baseline. The copy's base commit is not asked for again.
        let (sample_before, start_before) = (probe.calls().sample, probe.calls().start_of);
        let out = run(&mut engine, &panes, &probe, first + regular + 1_000);
        assert_eq!(probe.calls().sample, sample_before + 1);
        assert_eq!(probe.calls().start_of, start_before);
        // A now starts at HEAD (h1) and B at s1: the lines are no longer compared.
        assert_eq!(out.changes, vec![Change::Updated(pair_id())]);
        assert_eq!(out.alerts[0].files[0].how, How::DifferentStart);
    }

    #[test]
    fn not_git_parks_the_checkout_for_five_minutes() {
        let (first, _) = poll_ms();
        let probe = world(&[ROOT_A, ROOT_B]);
        probe.push_sample(ROOT_A, Err(ProbeError::NotGit));
        let panes = [pane(A, ROOT_A, PaneRun::Working, 1_000), pane(B, ROOT_B, PaneRun::Resting, 1_000)];
        let mut engine = Engine::new();
        run(&mut engine, &panes, &probe, 0);
        run(&mut engine, &panes, &probe, first);
        assert_eq!(probe.calls().sample, 1);
        run(&mut engine, &panes, &probe, first + FAILED_PARK_MS - 1);
        assert_eq!(probe.calls().sample, 1, "parked");
        run(&mut engine, &panes, &probe, first + FAILED_PARK_MS);
        assert_eq!(probe.calls().sample, 2);
    }

    #[test]
    fn a_timed_out_look_waits_the_longest_gap() {
        let (first, _) = poll_ms();
        let probe = world(&[ROOT_A, ROOT_B]);
        probe.push_sample(ROOT_A, Err(ProbeError::TooSlow));
        probe.push_sample(ROOT_A, Ok(look(vec![])));
        let panes = [pane(A, ROOT_A, PaneRun::Working, 1_000), pane(B, ROOT_B, PaneRun::Resting, 1_000)];
        let mut engine = Engine::new();
        run(&mut engine, &panes, &probe, 0);
        run(&mut engine, &panes, &probe, first);
        // A killed git call costs 10 s: 20 x 10 s is over POLL_MAX_MS, so the gap is 2 minutes.
        run(&mut engine, &panes, &probe, first + POLL_MAX_MS - 1);
        assert_eq!(probe.calls().sample, 1);
        run(&mut engine, &panes, &probe, first + POLL_MAX_MS);
        assert_eq!(probe.calls().sample, 2);
    }

    #[test]
    fn a_failed_probe_never_panics_the_step() {
        let (first, _) = poll_ms();
        let probe = world(&[ROOT_A, ROOT_B]);
        probe.push_sample(ROOT_A, Err(ProbeError::Failed("boom".to_string())));
        probe.push_sample(ROOT_B, Err(ProbeError::Failed("boom".to_string())));
        let panes = [pane(A, ROOT_A, PaneRun::Working, 1_000), pane(B, ROOT_B, PaneRun::Working, 1_000)];
        let mut engine = Engine::new();
        run(&mut engine, &panes, &probe, 0);
        let out = run(&mut engine, &panes, &probe, first);
        assert!(out.changes.is_empty() && out.alerts.is_empty());
    }

    #[test]
    fn three_panes_give_three_alerts_at_most() {
        let (first, _) = poll_ms();
        let roots = [ROOT_A, ROOT_B, ROOT_C];
        let probe = world(&roots);
        for root in roots {
            probe.push_sample(root, Ok(look(vec![touched(AUTH, 5_000)])));
            probe.set_hunks(root, AUTH, Ok(lines(&[(50, 50)])));
        }
        let panes = [
            pane(A, ROOT_A, PaneRun::Working, 1_000),
            pane(B, ROOT_B, PaneRun::Working, 1_000),
            pane(C, ROOT_C, PaneRun::Working, 1_000),
        ];
        let mut engine = Engine::new();
        run(&mut engine, &panes, &probe, 0);
        let out = run(&mut engine, &panes, &probe, first);
        assert_eq!(out.changes.len(), 3);
        assert_eq!(out.alerts.len(), 3);
        assert!(out.alerts.iter().all(|a| a.level == Level::Clash));
    }
}
