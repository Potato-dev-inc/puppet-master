//! Comparing the project now with a snapshot and turning the differences into findings
//! (DESIGN-MISSIONS section 4.3). Cumulative: a skip added three rounds ago and kept is still found.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use super::config;
use super::diff::{diff_text, similarity, LineDiff};
use super::facts::{analyze, collapse_ws, region_only, Facts, Marker};
use super::lexer::LineView;
use super::model::*;
use super::protect::{self, config_active, Loaded};
use super::rules::{ConfigRule, Ecosystem, GlobSet, Rules};
use super::snapshot::now_ms;

#[derive(Debug, Clone, Default)]
pub struct CheckOptions {
    /// "Agents may change tests" is on: every finding is a note.
    pub may_change_tests: bool,
    /// Files the person edited between rounds: their findings are notes about the person.
    pub edited_by_person: Vec<String>,
    /// Counts from the test run now, when there was one.
    pub counts: Option<RunCounts>,
    /// The commands the checks run now, when the caller wants them compared with the start.
    pub check_argvs: Option<Vec<Vec<String>>>,
}

/// One protected path that differs from the snapshot.
struct Item<'a> {
    path: &'a str,
    role: Role,
    eco: Option<&'a Ecosystem>,
    b: Option<&'a FileSnap>,
    n: Option<&'a Loaded>,
    bf: Option<Facts>,
    nf: Option<Facts>,
    /// `None` when a side exists but its text is unavailable (too big, not text, over the cap).
    diff: Option<LineDiff>,
}

impl Item<'_> {
    fn deleted(&self) -> bool {
        self.n.is_none()
    }

    fn scannable(&self) -> bool {
        self.role == Role::Test && self.eco.is_some_and(|e| e.scannable)
    }

    fn counts(&self) -> (u32, u32, u32, u32) {
        // (tests before, tests now, assertions before, assertions now)
        if !self.scannable() {
            return (0, 0, 0, 0);
        }
        let b = self.b.map(|b| (b.tests, b.assertions)).unwrap_or((0, 0));
        let n = match (&self.n, &self.nf) {
            (None, _) => (0, 0),
            (Some(_), Some(f)) => (f.tests, f.assertions),
            (Some(_), None) => b,
        };
        (b.0, n.0, b.1, n.1)
    }
}

fn clip(text: &str, max: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= max {
        text.to_string()
    } else {
        let cut: String = text.chars().take(max).collect();
        format!("{cut}…")
    }
}

fn plural(n: u32, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

pub fn check(root: &Path, snap: &Snapshot, opts: &CheckOptions) -> Report {
    let rules = Rules::builtin();
    let walked = protect::walk(root, rules, &GlobSet::new(&snap.protect), &GlobSet::new(&snap.ignore), Some(&snap.checks));
    let now: BTreeMap<&str, &Loaded> =
        walked.protected.iter().filter(|(_, l)| config_active(l, &snap.checks)).map(|(p, l)| (p.as_str(), l)).collect();

    let mut partial = snap.partial || walked.truncated;
    let paths: BTreeSet<&str> = snap.files.keys().map(String::as_str).chain(now.keys().copied()).collect();
    let mut items: Vec<Item> = Vec::new();
    for path in paths {
        let (b, n) = (snap.files.get(path), now.get(path).copied());
        if let (Some(b), Some(n)) = (b, n) {
            if b.sha256 == n.sha256 {
                continue;
            }
        }
        let role = b.map(|b| b.role).or(n.map(|n| n.role)).unwrap_or(Role::Test);
        let eco = b.and_then(|b| b.ecosystem.as_deref()).or(n.and_then(|n| n.eco.as_deref())).and_then(|id| rules.ecosystem(id));
        let b_text = b.and_then(|b| b.content.as_deref());
        let n_text = n.and_then(|n| n.text.as_deref());
        if (b.is_some() && b_text.is_none()) || (n.is_some() && n_text.is_none()) {
            partial = true;
        }
        let analyzable = role == Role::Test && eco.is_some_and(|e| e.scannable);
        let region = eco.is_some_and(|e| region_only(e, path));
        let bf = if analyzable { eco.zip(b_text).map(|(e, t)| analyze(e, t, region)) } else { None };
        let nf = if analyzable { eco.zip(n_text).map(|(e, t)| analyze(e, t, region)) } else { None };
        let diff = match (b, n) {
            (Some(_), Some(_)) => b_text.zip(n_text).map(|(a, c)| diff_text(Some(a), Some(c))),
            (Some(_), None) => b_text.map(|a| diff_text(Some(a), None)),
            (None, _) => n_text.map(|c| diff_text(None, Some(c))),
        };
        items.push(Item { path, role, eco, b, n, bf, nf, diff });
    }

    let mut run = Run { rules, snap, root, opts, items: &items, findings: Vec::new(), explained: BTreeSet::new(), lost: BTreeMap::new() };
    run.skips_and_focus();
    run.removed_tests();
    run.loosened();
    run.removed_assertions();
    run.snapshots();
    run.config_files(&now);
    run.argv();
    run.fewer_tests();
    run.silenced(&walked.silencers);
    run.cant_read();
    run.notes();
    let mut findings = run.findings;

    for f in &mut findings {
        if opts.edited_by_person.iter().any(|p| p == &f.file) {
            f.by_person = true;
            f.severity = Severity::Note;
        }
        if opts.may_change_tests {
            f.severity = Severity::Note;
        }
    }
    findings.sort_by(|a, b| {
        (a.severity, a.kind.number(), &a.file, a.line.unwrap_or(0)).cmp(&(b.severity, b.kind.number(), &b.file, b.line.unwrap_or(0)))
    });
    findings.truncate(300);

    let blocks = findings.iter().any(|f| f.severity == Severity::Blocks);
    let headline = if blocks {
        "Tests were changed"
    } else if findings.is_empty() {
        "No test changes"
    } else if opts.may_change_tests {
        "Tests changed, and you allowed that"
    } else {
        "Test files changed; nothing was skipped or removed"
    }
    .to_string();
    Report {
        snapshot_id: snap.id.clone(),
        checked_at_ms: now_ms(),
        blocks,
        findings,
        headline,
        files_changed: items.len() as u32,
        files_checked: now.len() as u32,
        partial,
    }
}

struct Run<'a> {
    rules: &'a Rules,
    snap: &'a Snapshot,
    root: &'a Path,
    opts: &'a CheckOptions,
    items: &'a [Item<'a>],
    findings: Vec<Finding>,
    /// Paths that already have a finding, so a plain "changed" note is not added for them.
    explained: BTreeSet<String>,
    /// Assertions that a loosened line stopped being (`t.Errorf` became `t.Logf`), per file: the
    /// loosened finding already says so, so "removed assertions" does not count them again.
    lost: BTreeMap<String, u32>,
}

impl<'a> Run<'a> {
    fn push(&mut self, f: Finding) {
        if !f.file.is_empty() {
            self.explained.insert(f.file.clone());
        }
        self.findings.push(f);
    }

    // ---- 1 and 2: skip and focus markers --------------------------------------------------------

    fn skips_and_focus(&mut self) {
        let items = self.items;
        for (kind, pick) in [(Kind::SkipAdded, Pick::Skips), (Kind::FocusAdded, Pick::Focus)] {
            // Removed markers cancel added ones with the same key: the test moved or was reflowed.
            let mut pool: HashMap<String, u32> = HashMap::new();
            let mut added: Vec<(&Item, &Marker)> = Vec::new();
            for item in items.iter().filter(|i| i.scannable()) {
                let (a, r) = markers_of(item, pick);
                for m in r {
                    *pool.entry(m.key()).or_default() += 1;
                }
                added.extend(a.into_iter().map(|m| (item, m)));
            }
            for (item, marker) in added {
                if let Some(left) = pool.get_mut(&marker.key()) {
                    if *left > 0 {
                        *left -= 1;
                        continue;
                    }
                }
                let target = match &marker.name {
                    Some(name) => format!("\"{name}\" in {}", item.path),
                    None => format!("in {}", item.path),
                };
                let summary = match kind {
                    Kind::SkipAdded => format!("Added {} {}{target}", marker.label, if marker.name.is_some() { "to " } else { "" }),
                    _ => format!(
                        "Added {} {}{target}, so the other tests don't run",
                        marker.label,
                        if marker.name.is_some() { "to " } else { "" }
                    ),
                };
                let mut f = Finding::new(kind, Severity::Blocks, item.path, summary);
                f.line = Some(marker.line);
                f.after = Some(clip(&marker.text, 200));
                f.in_new_file = item.b.is_none();
                self.push(f);
            }
        }
    }

    // ---- 3 and 4: tests and assertions removed ---------------------------------------------------

    fn removed_tests(&mut self) {
        let items = self.items;
        let scan: Vec<&Item> = items.iter().filter(|i| i.scannable()).collect();
        let before: u32 = scan.iter().map(|i| i.counts().0).sum();
        let now: u32 = scan.iter().map(|i| i.counts().1).sum();
        if before <= now {
            return;
        }
        let mut left = before - now;
        let mut candidates: Vec<(&Item, u32)> = scan.iter().filter_map(|i| {
            let (b, n, _, _) = i.counts();
            (b > n).then_some((*i, b - n))
        }).collect();
        // Whole files first, then the biggest drops.
        candidates.sort_by_key(|(i, drop)| (!i.deleted(), std::cmp::Reverse(*drop)));
        for (item, drop) in candidates {
            if left == 0 {
                break;
            }
            let take = drop.min(left);
            left -= take;
            let summary = if item.deleted() {
                if self.root.join(item.path).is_file() {
                    format!("Renamed {} so it is no longer a test file", item.path)
                } else {
                    format!("Deleted {} ({})", item.path, plural(take, "test", "tests"))
                }
            } else {
                format!("Removed {} from {}", plural(take, "test", "tests"), item.path)
            };
            let mut f = Finding::new(Kind::TestsRemoved, Severity::Blocks, item.path, summary);
            f.count = take;
            if let Some((line, text)) = removed_line(item, |e, v| e.test.iter().any(|p| p.regex.is_match(&v.code))) {
                f.line = Some(line);
                f.before = Some(clip(&text, 200));
            }
            self.push(f);
        }
    }

    fn removed_assertions(&mut self) {
        let items = self.items;
        let scan: Vec<&Item> = items.iter().filter(|i| i.scannable()).collect();
        let before: u32 = scan.iter().map(|i| i.counts().2).sum();
        let now: u32 = scan.iter().map(|i| i.counts().3).sum();
        let lost_total: u32 = self.lost.values().sum();
        let drop_of = |i: &Item, lost: &BTreeMap<String, u32>| {
            let (_, _, b, n) = i.counts();
            b.saturating_sub(n).saturating_sub(lost.get(i.path).copied().unwrap_or(0))
        };
        if before <= now + lost_total {
            return;
        }
        // Assertions that went away with removed tests are part of that finding.
        let reported_tests: BTreeSet<&str> =
            self.findings.iter().filter(|f| f.kind == Kind::TestsRemoved).map(|f| f.file.as_str()).collect();
        let explained: u32 = scan
            .iter()
            .filter(|i| reported_tests.contains(i.path))
            .map(|i| i.counts().2.saturating_sub(i.counts().3))
            .sum();
        let mut left = (before - now).saturating_sub(lost_total).saturating_sub(explained);
        let mut candidates: Vec<(&Item, u32)> = scan
            .iter()
            .filter(|i| !reported_tests.contains(i.path))
            .filter_map(|i| {
                let drop = drop_of(i, &self.lost);
                (drop > 0).then_some((*i, drop))
            })
            .collect();
        candidates.sort_by_key(|(_, drop)| std::cmp::Reverse(*drop));
        for (item, drop) in candidates {
            if left == 0 {
                break;
            }
            let take = drop.min(left);
            left -= take;
            let mut f = Finding::new(
                Kind::AssertionsRemoved,
                Severity::Blocks,
                item.path,
                format!("Removed {} in {}", plural(take, "assertion", "assertions"), item.path),
            );
            f.count = take;
            if let Some((line, text)) = removed_line(item, |e, v| e.assertion.iter().any(|p| p.regex.is_match(&v.code))) {
                f.line = Some(line);
                f.before = Some(clip(&text, 200));
            }
            self.push(f);
        }
    }

    // ---- 5: weaker assertions ----------------------------------------------------------------------

    fn loosened(&mut self) {
        let items = self.items;
        let mut out = Vec::new();
        let mut lost: BTreeMap<String, u32> = BTreeMap::new();
        // Lines that left one file may arrive in another (a moved test keeps its assertions as they
        // were), so a removed line cancels one identical added line anywhere.
        let mut removed_pool: HashMap<String, u32> = HashMap::new();
        for item in items.iter().filter(|i| i.scannable()) {
            if let (Some(bf), Some(diff)) = (&item.bf, &item.diff) {
                for n in &diff.removed {
                    *removed_pool.entry(collapse_ws(&bf.lines[*n as usize - 1].raw)).or_default() += 1;
                }
            }
        }
        let mut cancelled = |text: &str| -> bool {
            match removed_pool.get_mut(&collapse_ws(text)) {
                Some(left) if *left > 0 => {
                    *left -= 1;
                    true
                }
                _ => false,
            }
        };
        for item in items.iter().filter(|i| i.scannable()) {
            let (Some(eco), Some(nf), Some(diff)) = (item.eco, &item.nf, &item.diff) else { continue };
            let old_for_new: HashMap<u32, u32> = diff.pairs.iter().map(|(o, n)| (*n, *o)).collect();
            let mut flagged: BTreeSet<u32> = BTreeSet::new();
            if let Some(bf) = &item.bf {
                for (o, n) in &diff.pairs {
                    let (old, new) = (&bf.lines[*o as usize - 1], &nf.lines[*n as usize - 1]);
                    if let Some(summary) = loosened_pair(eco, item.path, old, new) {
                        let mut f = Finding::new(Kind::AssertionLoosened, Severity::Blocks, item.path, summary);
                        f.line = Some(*n);
                        f.before = Some(clip(&old.raw, 200));
                        f.after = Some(clip(&new.raw, 200));
                        flagged.insert(*n);
                        if !eco.assertion.iter().any(|p| p.regex.is_match(&new.code)) {
                            *lost.entry(item.path.to_string()).or_default() += 1;
                        }
                        out.push(f);
                    }
                }
            }
            for n in &diff.added {
                if flagged.contains(n) {
                    continue;
                }
                let new = &nf.lines[*n as usize - 1];
                let old = old_for_new.get(n).and_then(|o| item.bf.as_ref().map(|bf| &bf.lines[*o as usize - 1]));
                let finding = if let Some(t) = nf.tautologies.iter().find(|t| t.line == *n) {
                    Some(tautology_finding(item.path, t))
                } else if item.bf.is_some() {
                    self_compare(eco, item.path, old, new).map(|summary| {
                        let mut f = Finding::new(Kind::AssertionLoosened, Severity::Blocks, item.path, summary);
                        f.line = Some(*n);
                        f.before = old.map(|o| clip(&o.raw, 200));
                        f.after = Some(clip(&new.raw, 200));
                        f
                    })
                } else {
                    None
                };
                if let Some(f) = finding {
                    if !cancelled(&new.raw) {
                        out.push(f);
                    }
                }
            }
        }
        self.lost = lost;
        for f in out {
            self.push(f);
        }
    }

    // ---- 6: snapshot files -------------------------------------------------------------------------

    fn snapshots(&mut self) {
        let items = self.items;
        let mut out = Vec::new();
        for item in items.iter().filter(|i| i.role == Role::Snapshot) {
            let name = item.path.rsplit('/').next().unwrap_or(item.path);
            match (item.b, item.n) {
                (Some(_), Some(_)) => {
                    out.push(Finding::new(Kind::SnapshotsRewritten, Severity::Blocks, item.path, format!("Rewrote 1 snapshot file ({name})")))
                }
                (Some(_), None) => out.push(Finding::new(
                    Kind::SnapshotsRewritten,
                    Severity::Blocks,
                    item.path,
                    format!("Deleted 1 snapshot file ({name}), so its tests start fresh"),
                )),
                _ => {}
            }
        }
        for f in out {
            self.push(f);
        }
    }

    // ---- 7: how checks run ---------------------------------------------------------------------------

    fn config_files(&mut self, now: &BTreeMap<&str, &Loaded>) {
        let items = self.items;
        let checks = &self.snap.checks;
        let stem = |p: &str| p.rsplit_once('.').map(|(s, _)| s.to_string()).unwrap_or_else(|| p.to_string());
        // `jest.config.js` renamed to `jest.config.ts`: the old file is compared with the new one.
        let mut sibling_of: BTreeMap<&str, &str> = BTreeMap::new();
        for item in items.iter().filter(|i| i.role == Role::Config && i.deleted()) {
            let sibling = now
                .iter()
                .find(|(p, l)| l.role == Role::Config && stem(p) == stem(item.path) && !self.snap.files.contains_key(**p))
                .map(|(p, _)| *p);
            if let Some(p) = sibling {
                sibling_of.insert(item.path, p);
            }
        }
        let paired: BTreeSet<&str> = sibling_of.values().copied().collect();
        let mut out: Vec<Finding> = Vec::new();
        for item in items.iter().filter(|i| i.role == Role::Config) {
            if paired.contains(item.path) {
                continue;
            }
            let before = item.b.and_then(|b| b.content.as_deref());
            let mut after = item.n.and_then(|n| n.text.as_deref());
            if (item.b.is_some() && before.is_none()) || (item.n.is_some() && after.is_none()) {
                continue; // reported as "couldn't check"
            }
            let rules = matching_rules(self.rules, item.path, checks);
            if item.deleted() {
                match sibling_of.get(item.path) {
                    Some(p) => after = now.get(p).and_then(|l| l.text.as_deref()),
                    None => {
                        let how = rules.first().map(|(_, c)| how_phrase(*c)).unwrap_or("how tests run");
                        out.push(Finding::new(Kind::ChecksChanged, Severity::Blocks, item.path, format!("Deleted {}, which sets {how}", item.path)));
                        continue;
                    }
                }
            }
            for (rule, _) in rules {
                out.extend(config::compare(item.path, rule, before, after, checks).findings);
            }
        }
        for f in out {
            self.push(f);
        }
    }

    fn argv(&mut self) {
        let Some(now) = &self.opts.check_argvs else { return };
        let flags: Vec<&super::rules::Flag> = self
            .rules
            .ecosystems
            .iter()
            .flat_map(|e| e.config.iter())
            .filter(|c| c.check == CheckKind::Tests)
            .flat_map(|c| c.flags.iter())
            .filter(|f| !f.on_removed)
            .collect();
        let old: Vec<String> = self.snap.check_argvs.iter().map(|a| a.join(" ")).collect();
        let mut out = Vec::new();
        for (i, argv) in now.iter().enumerate() {
            let line = argv.join(" ");
            if old.get(i) == Some(&line) {
                continue;
            }
            if let Some(flag) = flags.iter().find(|f| f.regex.is_match(&line) && !old.get(i).is_some_and(|o| f.regex.is_match(o))) {
                let kind = match flag.kind {
                    super::rules::FlagKind::Focus => Kind::FocusAdded,
                    super::rules::FlagKind::Snapshots => Kind::SnapshotsRewritten,
                    super::rules::FlagKind::Checks => Kind::ChecksChanged,
                };
                let mut f = Finding::new(kind, Severity::Blocks, "", format!("Changed how tests run: the test command now {}", flag.say));
                f.before = old.get(i).cloned();
                f.after = Some(clip(&line, 200));
                out.push(f);
            }
        }
        for f in out {
            self.push(f);
        }
    }

    // ---- 8: fewer tests ran ------------------------------------------------------------------------------

    fn fewer_tests(&mut self) {
        let (Some(before), Some(now)) = (self.snap.counts, self.opts.counts) else { return };
        let mut out = Vec::new();
        if let (Some(b), Some(n)) = (before.total, now.total) {
            if n < b {
                out.push(Finding::new(Kind::FewerTestsRan, Severity::Blocks, "", format!("{} at the start; {n} ran now", ran(b))));
            }
        }
        if let (Some(b), Some(n)) = (before.skipped, now.skipped) {
            if n > b {
                let skipped = |c: u32| if c == 1 { "1 test was skipped".to_string() } else { format!("{c} tests were skipped") };
                out.push(Finding::new(Kind::FewerTestsRan, Severity::Blocks, "", format!("{} at the start; {} now", skipped(b), skipped(n))));
            }
        }
        for f in out {
            self.push(f);
        }
    }

    // ---- 9: markers that hide check errors ----------------------------------------------------------------

    fn silenced(&mut self, now: &BTreeMap<String, Vec<Marker>>) {
        let mut pool: HashMap<String, u32> = HashMap::new();
        for texts in self.snap.silencers.values() {
            for t in texts {
                *pool.entry(t.clone()).or_default() += 1;
            }
        }
        // (file, label, check) -> (count, first line, first text)
        let mut groups: BTreeMap<(String, String, CheckKind), (u32, u32, String)> = BTreeMap::new();
        for (path, markers) in now {
            for m in markers {
                let Some(check) = m.check.filter(|c| self.snap.checks.contains(c)) else { continue };
                if let Some(left) = pool.get_mut(&m.key()) {
                    if *left > 0 {
                        *left -= 1;
                        continue;
                    }
                }
                let entry = groups.entry((path.clone(), m.label.clone(), check)).or_insert((0, m.line, clip(&m.text, 200)));
                entry.0 += 1;
            }
        }
        for ((file, label, check), (count, line, text)) in groups {
            let summary = format!(
                "Added {count} {label} {} in {file}, which {} {} errors",
                if count == 1 { "line" } else { "lines" },
                if count == 1 { "hides" } else { "hide" },
                check.label()
            );
            let mut f = Finding::new(Kind::ChecksSilenced, Severity::Blocks, &file, summary);
            f.count = count;
            f.line = Some(line);
            f.after = Some(text);
            self.push(f);
        }
    }

    // ---- 11 and 10 ------------------------------------------------------------------------------------------

    fn cant_read(&mut self) {
        let items = self.items;
        let mut out = Vec::new();
        for item in items {
            let unreadable = match item.role {
                Role::Test => !item.eco.is_some_and(|e| e.scannable) || item.diff.is_none(),
                Role::Config => (item.b.is_some() && item.b.and_then(|b| b.content.as_ref()).is_none()) || (item.n.is_some() && item.n.and_then(|n| n.text.as_ref()).is_none()),
                Role::Snapshot => false,
            };
            if unreadable {
                let what = if item.role == Role::Config { "how checks run" } else { "skipped tests" };
                out.push(Finding::new(Kind::CantRead, Severity::Note, item.path, format!("Couldn't check changes in {} for {what}", basename(item.path))));
            }
        }
        for f in out {
            self.push(f);
        }
    }

    fn notes(&mut self) {
        // Renames: a deleted test file and a new one with alike content.
        let items = self.items;
        let mut renamed_new: BTreeSet<&str> = BTreeSet::new();
        let mut notes = Vec::new();
        let new_files: Vec<&Item> = items.iter().filter(|i| i.b.is_none() && i.n.is_some() && i.role == Role::Test).collect();
        for old in items.iter().filter(|i| i.deleted() && i.role == Role::Test) {
            if self.explained.contains(old.path) {
                continue;
            }
            let Some(old_text) = old.b.and_then(|b| b.content.as_deref()) else { continue };
            let partner = new_files.iter().find(|n| {
                !renamed_new.contains(n.path) && n.n.and_then(|l| l.text.as_deref()).is_some_and(|t| similarity(old_text, t) >= 0.6)
            });
            if let Some(partner) = partner {
                renamed_new.insert(partner.path);
                notes.push(Finding::new(Kind::TestsChanged, Severity::Note, old.path, format!("Renamed {} to {}", old.path, partner.path)));
            }
        }
        for item in items {
            if self.explained.contains(item.path) || notes.iter().any(|n| n.file == item.path) {
                continue;
            }
            let (plus, minus) = item.diff.as_ref().map(|d| (d.plus(), d.minus())).unwrap_or((0, 0));
            let summary = match (item.b, item.n, item.role) {
                (None, Some(_), Role::Snapshot) => format!("Added the snapshot file {}", basename(item.path)),
                (None, Some(_), _) => {
                    if renamed_new.contains(item.path) {
                        continue;
                    }
                    format!("Added {} (+{plus})", item.path)
                }
                (Some(_), None, Role::Test) => format!("Removed {}; its tests are still in the other test files", item.path),
                (Some(_), None, _) => format!("Removed {}", item.path),
                (Some(_), Some(_), Role::Test) => {
                    if plus == 0 && minus == 0 {
                        format!("Reformatted {}", item.path)
                    } else {
                        format!("Changed {} (+{plus} −{minus}); no tests were skipped or removed", item.path)
                    }
                }
                (Some(_), Some(_), Role::Config) => {
                    // Only worth a note when the compared sections changed.
                    let changed = matching_rules(self.rules, item.path, &self.snap.checks).iter().any(|(rule, _)| {
                        let b = item.b.and_then(|b| b.content.as_deref());
                        let n = item.n.and_then(|n| n.text.as_deref());
                        config::compare(item.path, rule, b, n, &self.snap.checks).changed
                    });
                    if !changed {
                        continue;
                    }
                    format!("Changed {} (+{plus} −{minus})", item.path)
                }
                (Some(_), Some(_), Role::Snapshot) => continue,
                (None, None, _) => continue,
            };
            notes.push(Finding::new(Kind::TestsChanged, Severity::Note, item.path, summary));
        }
        for f in notes {
            self.push(f);
        }
    }
}

fn how_phrase(check: CheckKind) -> &'static str {
    match check {
        CheckKind::Tests => "how tests run",
        CheckKind::TypeCheck => "how the type check runs",
        CheckKind::Lint => "how lint runs",
        CheckKind::Build => "how the build runs",
    }
}

fn ran(total: u32) -> String {
    format!("{total} {}", if total == 1 { "test ran" } else { "tests ran" })
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn matching_rules<'r>(rules: &'r Rules, path: &str, checks: &[CheckKind]) -> Vec<(&'r ConfigRule, CheckKind)> {
    rules
        .ecosystems
        .iter()
        .flat_map(|e| e.config.iter())
        .filter(|r| r.globs.is_match(path) && checks.contains(&r.check))
        .map(|r| (r, r.check))
        .collect()
}

#[derive(Clone, Copy)]
enum Pick {
    Skips,
    Focus,
}

/// Markers added and markers removed in one file, by the line diff.
fn markers_of<'a>(item: &'a Item, pick: Pick) -> (Vec<&'a Marker>, Vec<&'a Marker>) {
    let list = |f: &'a Option<Facts>| -> &'a [Marker] {
        match (f, pick) {
            (Some(f), Pick::Skips) => &f.skips,
            (Some(f), Pick::Focus) => &f.focus,
            (None, _) => &[],
        }
    };
    let (old, new) = (list(&item.bf), list(&item.nf));
    match &item.diff {
        Some(d) => (
            new.iter().filter(|m| d.added.contains(&m.line)).collect(),
            old.iter().filter(|m| d.removed.contains(&m.line)).collect(),
        ),
        None => {
            // No text on one side: fall back to the stored counts.
            let before = item.b.map(|b| if matches!(pick, Pick::Skips) { b.skips } else { b.focus }).unwrap_or(0) as usize;
            let extra = new.len().saturating_sub(before);
            (new.iter().skip(new.len() - extra).collect(), Vec::new())
        }
    }
}

/// The first removed line (in the old file) that `matches`: where a removed test or assertion was.
fn removed_line(item: &Item, matches: impl Fn(&Ecosystem, &LineView) -> bool) -> Option<(u32, String)> {
    let (eco, bf, diff) = (item.eco?, item.bf.as_ref()?, item.diff.as_ref()?);
    diff.removed
        .iter()
        .find(|n| matches(eco, &bf.lines[**n as usize - 1]))
        .map(|n| (*n, bf.lines[*n as usize - 1].raw.clone()))
}

fn tautology_finding(file: &str, marker: &Marker) -> Finding {
    let mut f = Finding::new(
        Kind::AssertionLoosened,
        Severity::Blocks,
        file,
        format!("Added an assertion in {file} that can't fail: {}", clip(&marker.text, 80)),
    );
    f.line = Some(marker.line);
    f.after = Some(clip(&marker.text, 200));
    f
}

/// The call that starts at `start`, with its balanced parentheses (`toBe(10.01)`), else the line.
fn snippet(raw: &str, start: usize) -> String {
    let rest = raw.get(start..).unwrap_or(raw);
    let rest = rest.trim_start_matches(['.', ' ']);
    let mut depth = 0usize;
    let mut end = None;
    for (i, ch) in rest.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    end = Some(i + 1);
                    break;
                }
            }
            _ => {}
        }
    }
    clip(&rest[..end.unwrap_or(rest.len())], 60)
}

fn loosened_pair(eco: &Ecosystem, file: &str, old: &LineView, new: &LineView) -> Option<String> {
    for rule in &eco.loosen {
        let Some(to) = rule.to.find(&new.code) else { continue };
        if rule.to.is_match(&old.code) {
            continue;
        }
        let from_start = match &rule.from {
            Some(from) => from.find(&old.code).map(|m| m.start()),
            None => eco.assertion.iter().find_map(|p| p.regex.find(&old.code)).map(|m| m.start()),
        };
        let Some(from_start) = from_start else { continue };
        let (old_snip, new_snip) = if rule.from.is_some() {
            (snippet(&old.raw, from_start), snippet(&new.raw, to.start()))
        } else {
            (clip(&old.raw, 60), clip(&new.raw, 60))
        };
        return Some(format!("Loosened an assertion in {file}: {old_snip} → {new_snip}"));
    }
    for tol in &eco.tolerance {
        let (Some(o), Some(n)) = (tol.regex.captures(&old.code), tol.regex.captures(&new.code)) else { continue };
        let num = |c: &regex::Captures| c.name("n").and_then(|m| m.as_str().replace('_', "").parse::<f64>().ok());
        let (Some(old_n), Some(new_n)) = (num(&o), num(&n)) else { continue };
        let looser = if tol.looser_when_smaller { new_n < old_n } else { new_n > old_n };
        if looser {
            return Some(format!("Loosened an assertion in {file}: {} → {}", clip(&old.raw, 60), clip(&new.raw, 60)));
        }
    }
    None
}

/// An assertion that compares a value with itself.
fn self_compare(eco: &Ecosystem, file: &str, old: Option<&LineView>, new: &LineView) -> Option<String> {
    let same = |view: &LineView| -> bool {
        eco.self_compare.iter().any(|re| {
            re.captures(&view.code).is_some_and(|c| match (c.name("actual"), c.name("expected")) {
                (Some(a), Some(e)) => {
                    let (a, e) = (view.raw[a.range()].trim(), view.raw[e.range()].trim());
                    !a.is_empty() && a == e
                }
                _ => false,
            })
        })
    };
    if same(new) && !old.is_some_and(same) {
        Some(format!("Loosened an assertion in {file}: it now compares a value with itself, {}", clip(&new.raw, 60)))
    } else {
        None
    }
}
