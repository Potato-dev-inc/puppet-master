//! Collision Alert: compares the changed files of two sides (SPEC-n33 section 3.4).
//!
//! Copies (two checkouts of one repo) are matched by file name first. Lines are read only for the
//! files both sides changed, and only when both started from the same commit. Same folder is decided
//! by time only. Pure: the git reads come in as a closure, so the tests need no git.

use std::collections::{BTreeMap, BTreeSet};

use super::hunks::clash;
use super::model::{
    FileHunks, FileOverlap, How, Level, PaneFact, TouchKind, Touched, CREATED_SLACK_MS,
    MAX_COMPARE_FILES,
};
use super::noise::{clean_rel, is_noise};
use super::probe::ProbeError;

/// One side of a copies comparison. `touched` must already be filtered with `belongs` (the engine
/// does that, with the root's first-look deleted set).
pub struct SideInput<'a> {
    pub pane: &'a PaneFact,
    /// The checkout's top folder.
    pub root: &'a str,
    /// The checkout's start commit. Empty means HEAD.
    pub start: &'a str,
    pub touched: &'a [Touched],
}

/// True when a changed file is the pane's own work: an edit or new file modified at or after the
/// pane was created (minus `CREATED_SLACK_MS`), or a deletion that was not already there at the
/// first look. Noise and unsafe names never count.
pub fn belongs(t: &Touched, pane: &PaneFact, deleted_at_first_look: &BTreeSet<String>) -> bool {
    if !usable(&t.path) {
        return false;
    }
    match (t.kind, t.mtime_ms) {
        (TouchKind::Deleted, _) => !deleted_at_first_look.contains(&t.path),
        (_, Some(mtime)) => mtime.saturating_add(CREATED_SLACK_MS) >= pane.created_ms,
        (_, None) => false,
    }
}

/// Overlaps between two checkouts of one repo. At most `MAX_COMPARE_FILES` common files, newest
/// change first. `hunks` reads one side's changed lines for one file (cached by the caller).
/// Two checkouts of the same root never compare (those pairs are `SameFolder`).
pub fn copies(
    a: &SideInput,
    b: &SideInput,
    hunks: &mut dyn FnMut(&SideInput, &str) -> Result<FileHunks, ProbeError>,
) -> Vec<FileOverlap> {
    if key(a.root) == key(b.root) {
        return Vec::new();
    }
    let a_files = index(a.touched);
    let b_files = index(b.touched);
    let mut common: Vec<(&Touched, &Touched)> = a_files
        .iter()
        .filter_map(|(k, ta)| b_files.get(k).map(|tb| (*ta, *tb)))
        .collect();
    common.sort_by(|x, y| newest(*y).cmp(&newest(*x)).then_with(|| x.0.path.cmp(&y.0.path)));
    common.truncate(MAX_COMPARE_FILES);

    let mut out = Vec::new();
    for (ta, tb) in common {
        if let Some(how) = overlap(a, b, ta, tb, hunks) {
            out.push(FileOverlap { path: ta.path.clone(), how });
        }
    }
    out
}

/// Files of the checkout both panes were working in: modified inside the window. A file counts
/// only when it is not older than the two panes (minus `CREATED_SLACK_MS`). Newest first.
/// Deleted files have no time, so they never count here.
pub fn shared(a: &PaneFact, b: &PaneFact, window: (u64, u64), touched: &[Touched]) -> Vec<FileOverlap> {
    let earliest = a.created_ms.max(b.created_ms).saturating_sub(CREATED_SLACK_MS);
    let (from, to) = (window.0.max(earliest), window.1);
    let mut hits: Vec<&Touched> = touched
        .iter()
        .filter(|t| t.kind != TouchKind::Deleted && usable(&t.path))
        .filter(|t| matches!(t.mtime_ms, Some(m) if m >= from && m <= to))
        .collect();
    hits.sort_by(|x, y| y.mtime_ms.cmp(&x.mtime_ms).then_with(|| x.path.cmp(&y.path)));
    hits.into_iter()
        .map(|t| FileOverlap { path: t.path.clone(), how: How::Shared })
        .collect()
}

/// The loudest level of the files: `Clash` if any file clashes, else `SharedFolder`, else
/// `SameFile`. `None` when there are no files (no alert).
pub fn level_of(files: &[FileOverlap]) -> Option<Level> {
    files.iter().map(|f| level_of_how(f.how)).max()
}

fn level_of_how(how: How) -> Level {
    match how {
        How::Lines(_) | How::LinesUnknown | How::BothNew | How::DeleteVsEdit { .. } | How::Binary => {
            Level::Clash
        }
        How::Shared => Level::SharedFolder,
        How::Apart | How::DifferentStart => Level::SameFile,
    }
}

/// The overlap of one common file, or `None` when it does not count.
fn overlap(
    a: &SideInput,
    b: &SideInput,
    ta: &Touched,
    tb: &Touched,
    hunks: &mut dyn FnMut(&SideInput, &str) -> Result<FileHunks, ProbeError>,
) -> Option<How> {
    match (ta.kind, tb.kind) {
        (TouchKind::Deleted, TouchKind::Deleted) => None,
        (TouchKind::Deleted, _) => Some(How::DeleteVsEdit { deleted_by: 0 }),
        (_, TouchKind::Deleted) => Some(How::DeleteVsEdit { deleted_by: 1 }),
        (TouchKind::Added, TouchKind::Added) => Some(How::BothNew),
        (TouchKind::Modified, TouchKind::Modified) => modified(a, b, ta, tb, hunks),
        // One side made the file new, the other edited the old one. The lines do not line up, so
        // it is not compared: a clash that is "not compared" (the same as a cut file).
        _ => Some(How::LinesUnknown),
    }
}

/// Both sides edited the file. Starts must match; then the changed lines decide.
fn modified(
    a: &SideInput,
    b: &SideInput,
    ta: &Touched,
    tb: &Touched,
    hunks: &mut dyn FnMut(&SideInput, &str) -> Result<FileHunks, ProbeError>,
) -> Option<How> {
    if a.start != b.start {
        return Some(How::DifferentStart);
    }
    let ha = match hunks(a, &ta.path) {
        Ok(h) => h,
        Err(_) => return Some(How::LinesUnknown),
    };
    let hb = match hunks(b, &tb.path) {
        Ok(h) => h,
        Err(_) => return Some(How::LinesUnknown),
    };
    // Binary is checked before `clash`, which answers "no clash" for binary files.
    if ha.binary || hb.binary {
        return Some(How::Binary);
    }
    if ha.cut || hb.cut {
        return Some(How::LinesUnknown);
    }
    // A side with no changed lines changed only its stat (content unchanged): no overlap.
    if ha.spans.is_empty() || hb.spans.is_empty() {
        return None;
    }
    Some(match clash(&ha, &hb) {
        Some(span) => How::Lines(span),
        None => How::Apart,
    })
}

/// Changed files by match key. A later duplicate replaces an earlier one.
fn index(touched: &[Touched]) -> BTreeMap<String, &Touched> {
    touched
        .iter()
        .filter(|t| usable(&t.path))
        .map(|t| (key(&t.path), t))
        .collect()
}

/// The key two paths are matched by: lower case on Windows (its file system ignores case).
fn key(path: &str) -> String {
    if cfg!(windows) {
        path.to_lowercase()
    } else {
        path.to_string()
    }
}

/// A path that is safe to compare and not noise.
fn usable(path: &str) -> bool {
    clean_rel(path).is_some() && !is_noise(path)
}

/// The newer of the two changes (a deletion counts as oldest).
fn newest(pair: (&Touched, &Touched)) -> u64 {
    pair.0.mtime_ms.unwrap_or(0).max(pair.1.mtime_ms.unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collision::model::{PaneRun, Span};

    fn pane(id: &str, created_ms: u64) -> PaneFact {
        PaneFact {
            pane_id: id.to_string(),
            agent_id: "fake_a".to_string(),
            label: id.to_string(),
            cwd: "C:\\Users\\you\\copy-a".to_string(),
            run: PaneRun::Working,
            created_ms,
        }
    }

    fn edit(path: &str, mtime: u64) -> Touched {
        Touched { path: path.to_string(), kind: TouchKind::Modified, mtime_ms: Some(mtime) }
    }

    fn touch(path: &str, kind: TouchKind, mtime: Option<u64>) -> Touched {
        Touched { path: path.to_string(), kind, mtime_ms: mtime }
    }

    fn hunks_of(spans: &[(u32, u32)]) -> FileHunks {
        FileHunks {
            spans: spans.iter().map(|&(start, end)| Span { start, end }).collect(),
            binary: false,
            cut: false,
        }
    }

    fn side<'a>(pane: &'a PaneFact, root: &'a str, start: &'a str, touched: &'a [Touched]) -> SideInput<'a> {
        SideInput { pane, root, start, touched }
    }

    /// Scripted hunks: the answer for each (root, path). Anything else has no changed lines.
    /// Counts every call, so a test can prove that no git read happened.
    struct Fake {
        calls: usize,
        answers: Vec<(String, String, Result<FileHunks, ProbeError>)>,
    }

    fn scripted(answers: Vec<(&str, &str, Result<FileHunks, ProbeError>)>) -> Fake {
        let answers = answers
            .into_iter()
            .map(|(root, path, answer)| (root.to_string(), path.to_string(), answer))
            .collect();
        Fake { calls: 0, answers }
    }

    impl Fake {
        fn read(&mut self, side: &SideInput, rel: &str) -> Result<FileHunks, ProbeError> {
            self.calls += 1;
            self.answers
                .iter()
                .find(|entry| entry.0 == side.root && entry.1 == rel)
                .map(|entry| entry.2.clone())
                .unwrap_or_else(|| Ok(hunks_of(&[])))
        }
    }

    const ROOT_A: &str = "C:\\Users\\you\\copy-a";
    const ROOT_B: &str = "C:\\Users\\you\\copy-b";

    fn run(a: &SideInput, b: &SideInput, fake: &mut Fake) -> Vec<FileOverlap> {
        copies(a, b, &mut |s, rel| fake.read(s, rel))
    }

    #[test]
    fn same_lines_clash_with_the_union_span() {
        let (pa, pb) = (pane("fake_a-pane", 0), pane("fake_b-pane", 0));
        let (ta, tb) = ([edit("src/auth.ts", 100)], [edit("src/auth.ts", 200)]);
        let mut fake = scripted(vec![
            (ROOT_A, "src/auth.ts", Ok(hunks_of(&[(40, 42)]))),
            (ROOT_B, "src/auth.ts", Ok(hunks_of(&[(41, 41)]))),
        ]);
        let out = run(&side(&pa, ROOT_A, "s1", &ta), &side(&pb, ROOT_B, "s1", &tb), &mut fake);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].path, "src/auth.ts");
        assert_eq!(out[0].how, How::Lines(Span { start: 40, end: 42 }));
        assert_eq!(fake.calls, 2);
        assert_eq!(level_of(&out), Some(Level::Clash));
    }

    #[test]
    fn far_lines_are_apart_and_only_a_same_file_level() {
        let (pa, pb) = (pane("fake_a-pane", 0), pane("fake_b-pane", 0));
        let (ta, tb) = ([edit("src/auth.ts", 100)], [edit("src/auth.ts", 200)]);
        let mut fake = scripted(vec![
            (ROOT_A, "src/auth.ts", Ok(hunks_of(&[(10, 10)]))),
            (ROOT_B, "src/auth.ts", Ok(hunks_of(&[(50, 50)]))),
        ]);
        let out = run(&side(&pa, ROOT_A, "s1", &ta), &side(&pb, ROOT_B, "s1", &tb), &mut fake);
        assert_eq!(out, vec![FileOverlap { path: "src/auth.ts".to_string(), how: How::Apart }]);
        assert_eq!(level_of(&out), Some(Level::SameFile));
    }

    #[test]
    fn both_new_files_clash_without_reading_lines() {
        let (pa, pb) = (pane("fake_a-pane", 0), pane("fake_b-pane", 0));
        let ta = [touch("src/new.ts", TouchKind::Added, Some(100))];
        let tb = [touch("src/new.ts", TouchKind::Added, Some(200))];
        let mut fake = scripted(vec![]);
        let out = run(&side(&pa, ROOT_A, "s1", &ta), &side(&pb, ROOT_B, "s1", &tb), &mut fake);
        assert_eq!(out, vec![FileOverlap { path: "src/new.ts".to_string(), how: How::BothNew }]);
        assert_eq!(fake.calls, 0);
    }

    #[test]
    fn delete_versus_edit_names_the_deleting_side_both_ways() {
        let (pa, pb) = (pane("fake_a-pane", 0), pane("fake_b-pane", 0));
        let del = [touch("src/old.ts", TouchKind::Deleted, None)];
        let edt = [edit("src/old.ts", 200)];
        let mut fake = scripted(vec![]);
        let first = run(&side(&pa, ROOT_A, "s1", &del), &side(&pb, ROOT_B, "s1", &edt), &mut fake);
        assert_eq!(first[0].how, How::DeleteVsEdit { deleted_by: 0 });
        let second = run(&side(&pa, ROOT_A, "s1", &edt), &side(&pb, ROOT_B, "s1", &del), &mut fake);
        assert_eq!(second[0].how, How::DeleteVsEdit { deleted_by: 1 });
        assert_eq!(level_of(&second), Some(Level::Clash));
        assert_eq!(fake.calls, 0);
    }

    #[test]
    fn both_deleted_is_no_overlap() {
        let (pa, pb) = (pane("fake_a-pane", 0), pane("fake_b-pane", 0));
        let del = [touch("src/old.ts", TouchKind::Deleted, None)];
        let mut fake = scripted(vec![]);
        let out = run(&side(&pa, ROOT_A, "s1", &del), &side(&pb, ROOT_B, "s1", &del), &mut fake);
        assert!(out.is_empty());
        assert_eq!(level_of(&out), None);
    }

    #[test]
    fn different_start_is_quiet_and_never_reads_hunks() {
        let (pa, pb) = (pane("fake_a-pane", 0), pane("fake_b-pane", 0));
        let (ta, tb) = ([edit("src/auth.ts", 100)], [edit("src/auth.ts", 200)]);
        let mut fake = scripted(vec![]);
        let out = run(&side(&pa, ROOT_A, "s1", &ta), &side(&pb, ROOT_B, "s2", &tb), &mut fake);
        assert_eq!(out, vec![FileOverlap { path: "src/auth.ts".to_string(), how: How::DifferentStart }]);
        assert_eq!(fake.calls, 0);
        assert_eq!(level_of(&out), Some(Level::SameFile));
    }

    #[test]
    fn binary_on_either_side_clashes_even_when_the_other_side_has_spans() {
        let (pa, pb) = (pane("fake_a-pane", 0), pane("fake_b-pane", 0));
        let (ta, tb) = ([edit("img/logo.bin", 100)], [edit("img/logo.bin", 200)]);
        let binary = FileHunks { spans: vec![], binary: true, cut: false };
        let mut fake = scripted(vec![
            (ROOT_A, "img/logo.bin", Ok(binary)),
            (ROOT_B, "img/logo.bin", Ok(hunks_of(&[(5, 5)]))),
        ]);
        let out = run(&side(&pa, ROOT_A, "s1", &ta), &side(&pb, ROOT_B, "s1", &tb), &mut fake);
        assert_eq!(out, vec![FileOverlap { path: "img/logo.bin".to_string(), how: How::Binary }]);
        assert_eq!(level_of(&out), Some(Level::Clash));
    }

    #[test]
    fn a_cut_side_is_not_compared_and_clashes() {
        let (pa, pb) = (pane("fake_a-pane", 0), pane("fake_b-pane", 0));
        let (ta, tb) = ([edit("src/big.ts", 100)], [edit("src/big.ts", 200)]);
        let cut = FileHunks { spans: vec![Span { start: 5, end: 5 }], binary: false, cut: true };
        let mut fake = scripted(vec![
            (ROOT_A, "src/big.ts", Ok(cut)),
            (ROOT_B, "src/big.ts", Ok(hunks_of(&[(90, 90)]))),
        ]);
        let out = run(&side(&pa, ROOT_A, "s1", &ta), &side(&pb, ROOT_B, "s1", &tb), &mut fake);
        assert_eq!(out[0].how, How::LinesUnknown);
        assert_eq!(level_of(&out), Some(Level::Clash));
    }

    #[test]
    fn a_failed_read_is_not_compared_and_clashes() {
        let (pa, pb) = (pane("fake_a-pane", 0), pane("fake_b-pane", 0));
        let (ta, tb) = ([edit("src/auth.ts", 100)], [edit("src/auth.ts", 200)]);
        let mut fake = scripted(vec![(ROOT_A, "src/auth.ts", Err(ProbeError::TooSlow))]);
        let out = run(&side(&pa, ROOT_A, "s1", &ta), &side(&pb, ROOT_B, "s1", &tb), &mut fake);
        assert_eq!(out[0].how, How::LinesUnknown);
    }

    #[test]
    fn stat_noise_with_no_changed_lines_is_dropped() {
        let (pa, pb) = (pane("fake_a-pane", 0), pane("fake_b-pane", 0));
        let (ta, tb) = ([edit("src/touch.ts", 100)], [edit("src/touch.ts", 200)]);
        // Both sides only changed their stat.
        let mut fake = scripted(vec![]);
        let both = run(&side(&pa, ROOT_A, "s1", &ta), &side(&pb, ROOT_B, "s1", &tb), &mut fake);
        assert!(both.is_empty());
        // One side only changed its stat, the other has real lines: still not a change on both.
        let mut fake = scripted(vec![(ROOT_B, "src/touch.ts", Ok(hunks_of(&[(3, 3)])))]);
        let one = run(&side(&pa, ROOT_A, "s1", &ta), &side(&pb, ROOT_B, "s1", &tb), &mut fake);
        assert!(one.is_empty());
    }

    #[test]
    fn paths_match_case_blind_on_windows_only() {
        let (pa, pb) = (pane("fake_a-pane", 0), pane("fake_b-pane", 0));
        let (ta, tb) = ([edit("Src/Auth.ts", 100)], [edit("src/auth.ts", 200)]);
        let mut fake = scripted(vec![
            (ROOT_A, "Src/Auth.ts", Ok(hunks_of(&[(1, 1)]))),
            (ROOT_B, "src/auth.ts", Ok(hunks_of(&[(9, 9)]))),
        ]);
        let out = run(&side(&pa, ROOT_A, "s1", &ta), &side(&pb, ROOT_B, "s1", &tb), &mut fake);
        if cfg!(windows) {
            assert_eq!(out.len(), 1);
            // The name shown and read is the one from side A.
            assert_eq!(out[0].path, "Src/Auth.ts");
        } else {
            assert!(out.is_empty());
        }
    }

    #[test]
    fn twenty_five_common_files_compare_twenty_newest_first() {
        let (pa, pb) = (pane("fake_a-pane", 0), pane("fake_b-pane", 0));
        let names: Vec<String> = (0..25).map(|i| format!("src/f{i:02}.ts")).collect();
        let ta: Vec<Touched> = names.iter().enumerate().map(|(i, n)| edit(n, 1_000 + i as u64)).collect();
        let tb: Vec<Touched> = names.iter().enumerate().map(|(i, n)| edit(n, 5_000 + i as u64)).collect();
        let mut fake = scripted(vec![]);
        let out = run(&side(&pa, ROOT_A, "s1", &ta), &side(&pb, ROOT_B, "s1", &tb), &mut fake);
        // Empty hunks on both sides: every compared file is stat noise, so none are reported.
        assert!(out.is_empty());
        assert_eq!(fake.calls, 2 * MAX_COMPARE_FILES);

        let answers: Vec<(&str, &str, Result<FileHunks, ProbeError>)> = names
            .iter()
            .flat_map(|n| {
                [
                    (ROOT_A, n.as_str(), Ok(hunks_of(&[(1, 1)]))),
                    (ROOT_B, n.as_str(), Ok(hunks_of(&[(9, 9)]))),
                ]
            })
            .collect();
        let mut fake = scripted(answers);
        let out = run(&side(&pa, ROOT_A, "s1", &ta), &side(&pb, ROOT_B, "s1", &tb), &mut fake);
        assert_eq!(out.len(), MAX_COMPARE_FILES);
        assert_eq!(out[0].path, "src/f24.ts");
        assert_eq!(out[MAX_COMPARE_FILES - 1].path, "src/f05.ts");
        assert_eq!(fake.calls, 2 * MAX_COMPARE_FILES);
    }

    #[test]
    fn noise_and_same_root_never_reach_the_reads() {
        let (pa, pb) = (pane("fake_a-pane", 0), pane("fake_b-pane", 0));
        let noisy = [edit("node_modules/x/index.js", 100), edit("package-lock.json", 100)];
        let mut fake = scripted(vec![]);
        let out = run(&side(&pa, ROOT_A, "s1", &noisy), &side(&pb, ROOT_B, "s1", &noisy), &mut fake);
        assert!(out.is_empty());
        assert_eq!(fake.calls, 0);

        let ta = [edit("src/auth.ts", 100)];
        let tb = [edit("src/auth.ts", 200)];
        let out = run(&side(&pa, ROOT_A, "s1", &ta), &side(&pb, "C:\\Users\\you\\copy-a", "s1", &tb), &mut fake);
        assert!(out.is_empty());
        assert_eq!(fake.calls, 0);
    }

    #[test]
    fn belongs_applies_the_pane_time_and_first_look_rules() {
        let p = pane("fake_a-pane", 100_000);
        let none = BTreeSet::new();
        assert!(!belongs(&edit("src/a.ts", 97_999), &p, &none), "older than created minus 2 s");
        assert!(belongs(&edit("src/a.ts", 99_000), &p, &none), "1 s before creation counts");
        assert!(belongs(&edit("src/a.ts", 100_000), &p, &none));
        assert!(!belongs(&touch("src/a.ts", TouchKind::Added, None), &p, &none), "no time, no blame");

        let gone_before = BTreeSet::from(["src/old.ts".to_string()]);
        assert!(!belongs(&touch("src/old.ts", TouchKind::Deleted, None), &p, &gone_before));
        assert!(belongs(&touch("src/new.ts", TouchKind::Deleted, None), &p, &gone_before));
        assert!(!belongs(&edit("node_modules/a.js", 200_000), &p, &none), "noise never counts");
    }

    #[test]
    fn shared_uses_the_window_edges_and_both_pane_starts() {
        let (pa, pb) = (pane("fake_a-pane", 10_000), pane("fake_b-pane", 10_000));
        let touched = [
            edit("src/in-low.ts", 8_000),   // exactly the earliest allowed (created minus 2 s)
            edit("src/in-high.ts", 20_000), // exactly the window end
            edit("src/too-old.ts", 7_999),  // older than the panes allow
            edit("src/too-late.ts", 20_001),
            touch("src/gone.ts", TouchKind::Deleted, None),
        ];
        let out = shared(&pa, &pb, (1_000, 20_000), &touched);
        let paths: Vec<&str> = out.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, vec!["src/in-high.ts", "src/in-low.ts"], "newest first, edges inside");
        assert!(out.iter().all(|f| f.how == How::Shared));
        assert_eq!(level_of(&out), Some(Level::SharedFolder));
    }

    #[test]
    fn shared_window_start_later_than_the_panes_wins() {
        let (pa, pb) = (pane("fake_a-pane", 0), pane("fake_b-pane", 0));
        let touched = [edit("src/a.ts", 4_999), edit("src/b.ts", 5_000)];
        let out = shared(&pa, &pb, (5_000, 9_000), &touched);
        assert_eq!(out, vec![FileOverlap { path: "src/b.ts".to_string(), how: How::Shared }]);
    }

    #[test]
    fn level_of_takes_the_loudest_file() {
        assert_eq!(level_of(&[]), None);
        let shared_only = vec![FileOverlap { path: "a".to_string(), how: How::Shared }];
        assert_eq!(level_of(&shared_only), Some(Level::SharedFolder));
        let apart = vec![FileOverlap { path: "a".to_string(), how: How::Apart }];
        assert_eq!(level_of(&apart), Some(Level::SameFile));
        let mixed = vec![
            FileOverlap { path: "a".to_string(), how: How::Apart },
            FileOverlap { path: "b".to_string(), how: How::Lines(Span { start: 1, end: 2 }) },
        ];
        assert_eq!(level_of(&mixed), Some(Level::Clash));
        let different = vec![FileOverlap { path: "a".to_string(), how: How::DifferentStart }];
        assert_eq!(level_of(&different), Some(Level::SameFile));
    }
}
