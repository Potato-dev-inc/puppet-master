//! The fixture corpus (`fixtures/<case>/{before,after}/` + `expect.toml`) and the guard's own
//! invariants. Every fixture file carries a `.fx` suffix so vitest, cargo and npm never take a
//! fixture for a real test or manifest; the harness strips it.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use super::model::{CheckKind, Kind, Severity, ALL_KINDS};
use super::*;

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("tamper").join("fixtures")
}

fn scratch(name: &str) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("pm-tamper-{name}-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Copy a fixture side into `dst`, dropping the `.fx` suffix.
fn copy_side(src: &Path, dst: &Path) {
    for entry in fs::read_dir(src).unwrap().flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            let sub = dst.join(&name);
            fs::create_dir_all(&sub).unwrap();
            copy_side(&path, &sub);
        } else {
            let real = name.strip_suffix(".fx").unwrap_or(&name);
            fs::write(dst.join(real), fs::read(&path).unwrap()).unwrap();
        }
    }
}

/// Remove everything except `.puppet-master`, so the folder can be refilled with the "after" side.
fn clear(dir: &Path) {
    for entry in fs::read_dir(dir).unwrap().flatten() {
        if entry.file_name() == ".puppet-master" {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            fs::remove_dir_all(&path).unwrap();
        } else {
            fs::remove_file(&path).unwrap();
        }
    }
}

struct Expect {
    blocks: Vec<Kind>,
    notes: Vec<Kind>,
    contains: Vec<String>,
    by_person: Vec<String>,
    in_new_file: bool,
    snapshot: SnapshotOptions,
    check: CheckOptions,
}

fn strings(table: &toml::Table, key: &str) -> Vec<String> {
    table.get(key).and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

fn kinds(table: &toml::Table, key: &str) -> Vec<Kind> {
    strings(table, key).iter().map(|k| Kind::parse(k).unwrap_or_else(|| panic!("unknown kind {k} in expect.toml"))).collect()
}

fn counts(table: &toml::Table, key: &str) -> Option<RunCounts> {
    let t = table.get(key)?.as_table()?;
    let n = |k: &str| t.get(k).and_then(|v| v.as_integer()).map(|v| v as u32);
    Some(RunCounts { total: n("total"), skipped: n("skipped"), ..RunCounts::default() })
}

fn read_expect(case: &Path) -> Expect {
    let table: toml::Table = fs::read_to_string(case.join("expect.toml")).unwrap().parse().unwrap();
    let flag = |k: &str| table.get(k).and_then(|v| v.as_bool()).unwrap_or(false);
    Expect {
        blocks: kinds(&table, "blocks"),
        notes: kinds(&table, "notes"),
        contains: strings(&table, "contains"),
        by_person: strings(&table, "by_person"),
        in_new_file: flag("in_new_file"),
        snapshot: SnapshotOptions {
            checks: strings(&table, "checks").iter().filter_map(|c| CheckKind::parse(c)).collect(),
            counts: counts(&table, "counts_before"),
            ignore: strings(&table, "ignore"),
            ..SnapshotOptions::default()
        },
        check: CheckOptions {
            may_change_tests: flag("may_change_tests"),
            edited_by_person: strings(&table, "edited_by_person"),
            counts: counts(&table, "counts_after"),
            check_argvs: None,
        },
    }
}

/// Snapshot the "before" side, swap in the "after" side, check. Returns the report.
fn run_case(case: &Path, name: &str) -> (Report, Expect) {
    let expect = read_expect(case);
    let dir = scratch(name);
    copy_side(&case.join("before"), &dir);
    let snap = take_and_save(&dir, &expect.snapshot).unwrap();
    clear(&dir);
    copy_side(&case.join("after"), &dir);
    let report = check_saved(&dir, &snap.id, &expect.check).unwrap();
    let _ = fs::remove_dir_all(&dir);
    (report, expect)
}

fn case_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = fs::read_dir(fixtures_dir()).unwrap().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    dirs.sort();
    dirs
}

fn kinds_of(report: &Report, severity: Severity) -> BTreeSet<Kind> {
    report.findings.iter().filter(|f| f.severity == severity).map(|f| f.kind).collect()
}

fn dump(report: &Report) -> String {
    report.findings.iter().map(|f| format!("  [{:?}] {} {}:{:?} {}", f.severity, f.kind.as_str(), f.file, f.line, f.summary)).collect::<Vec<_>>().join("\n")
}

#[test]
fn every_fixture_gives_the_expected_findings() {
    let dirs = case_dirs();
    assert!(dirs.len() >= 35, "the design asks for about 35 fixtures; found {}", dirs.len());
    let mut failures = Vec::new();
    let mut fired: BTreeSet<Kind> = BTreeSet::new();
    for case in &dirs {
        let name = case.file_name().unwrap().to_string_lossy().to_string();
        let (report, expect) = run_case(case, &name);
        fired.extend(report.findings.iter().map(|f| f.kind));
        let mut problems = Vec::new();
        let want_blocks: BTreeSet<Kind> = expect.blocks.iter().copied().collect();
        let got_blocks = kinds_of(&report, Severity::Blocks);
        if got_blocks != want_blocks {
            problems.push(format!("blocking kinds: wanted {want_blocks:?}, got {got_blocks:?}"));
        }
        if report.blocks != !want_blocks.is_empty() {
            problems.push(format!("report.blocks = {}", report.blocks));
        }
        let got_notes = kinds_of(&report, Severity::Note);
        for kind in &expect.notes {
            if !got_notes.contains(kind) {
                problems.push(format!("missing note {kind:?}"));
            }
        }
        for text in &expect.contains {
            if !report.findings.iter().any(|f| f.summary.contains(text.as_str())) {
                problems.push(format!("no sentence contains {text:?}"));
            }
        }
        for file in &expect.by_person {
            if !report.findings.iter().any(|f| &f.file == file && f.by_person) {
                problems.push(format!("{file} is not attributed to the person"));
            }
        }
        if expect.in_new_file && !report.findings.iter().any(|f| f.in_new_file) {
            problems.push("expected a finding in a new file".to_string());
        }
        if !problems.is_empty() {
            failures.push(format!("{name}:\n  {}\n{}", problems.join("\n  "), dump(&report)));
        }
    }
    assert!(failures.is_empty(), "{} of {} fixtures failed:\n\n{}", failures.len(), dirs.len(), failures.join("\n\n"));
    // Every one of the design's eleven kinds is exercised by the corpus.
    for kind in ALL_KINDS {
        assert!(fired.contains(&kind), "no fixture produces {kind:?}");
    }
}

#[test]
fn the_corpus_covers_the_design_names() {
    let have: BTreeSet<String> = case_dirs().iter().map(|p| p.file_name().unwrap().to_string_lossy().to_string()).collect();
    for name in [
        "js_skip_added", "js_only_added", "js_xit", "js_test_todo", "js_assert_removed", "js_toBe_to_toBeDefined", "js_expect_true",
        "js_test_deleted", "js_file_deleted", "js_snapshot_rewritten", "pkg_test_script_changed", "vitest_exclude_added",
        "jest_passWithNoTests", "ts_ignore_with_typecheck", "py_mark_skip", "py_assert_removed", "py_addopts_k", "py_approx_widened",
        "rs_ignore_attr", "rs_assert_eq_removed", "rs_cargo_test_false", "go_t_skip", "fewer_tests_ran", "new_test_added",
        "tests_moved_between_files", "asserts_refactored_same_count", "prettier_reflow", "skip_in_comment", "skip_in_string",
        "skip_present_at_start", "generated_ignored", "human_edited", "test_goal_switch_on", "ts_ignore_without_typecheck", "kotlin_changed",
    ] {
        assert!(have.contains(name), "missing fixture {name}");
    }
}

#[test]
fn no_changes_means_no_findings_and_the_check_writes_nothing() {
    let case = fixtures_dir().join("js_skip_added").join("before");
    let dir = scratch("quiet");
    copy_side(&case, &dir);
    let snap = take(&dir, &SnapshotOptions::default()).unwrap();
    let before = tree_hash(&dir);
    let report = check(&dir, &snap, &CheckOptions::default());
    assert!(report.findings.is_empty(), "{}", dump(&report));
    assert!(!report.blocks);
    assert_eq!(report.headline, "No test changes");
    assert_eq!(tree_hash(&dir), before, "a check must not write anything");
    let _ = fs::remove_dir_all(&dir);
}

fn tree_hash(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut all = Vec::new();
    fn walk(root: &Path, dir: &Path, all: &mut Vec<(String, Vec<u8>)>) {
        for entry in fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, all);
            } else {
                all.push((path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"), fs::read(&path).unwrap()));
            }
        }
    }
    walk(dir, dir, &mut all);
    all.sort();
    all
}

#[test]
fn findings_are_deterministic() {
    let case = fixtures_dir().join("js_test_todo");
    let (a, _) = run_case(&case, "det-a");
    let (b, _) = run_case(&case, "det-b");
    let strip = |r: &Report| r.findings.clone();
    assert_eq!(strip(&a), strip(&b));
}

#[test]
fn a_skip_added_in_an_early_round_is_still_found_later() {
    // Cumulative against the snapshot: the check does not care how many rounds ago the skip was added.
    let case = fixtures_dir().join("js_skip_added");
    let dir = scratch("cumulative");
    copy_side(&case.join("before"), &dir);
    let snap = take(&dir, &SnapshotOptions::default()).unwrap();
    clear(&dir);
    copy_side(&case.join("after"), &dir);
    for _ in 0..3 {
        // Later rounds change other files only.
        fs::write(dir.join("cart.ts"), "export const total = (p: number[]) => p.reduce((a, b) => a + b, 0);\n").unwrap();
        let report = check(&dir, &snap, &CheckOptions::default());
        assert!(report.blocks, "{}", dump(&report));
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn undo_restores_exactly_the_flagged_files_and_leaves_implementation_alone() {
    let case = fixtures_dir().join("js_skip_added");
    let dir = scratch("undo");
    copy_side(&case.join("before"), &dir);
    let original = fs::read(dir.join("cart.test.ts")).unwrap();
    let snap = take(&dir, &SnapshotOptions::default()).unwrap();
    clear(&dir);
    copy_side(&case.join("after"), &dir);
    // The agent also changed the implementation, and added a new test file with a skip.
    fs::write(dir.join("cart.ts"), "export const total = (p: number[]) => p.length;\n").unwrap();
    fs::write(dir.join("extra.test.ts"), "it.skip('later', () => {});\n").unwrap();
    fs::write(dir.join("notes.test.ts"), "it('fine', () => { expect(1).toBe(1); });\n").unwrap();
    let report = check(&dir, &snap, &CheckOptions::default());
    let files = report.blocking_files();
    assert_eq!(files, ["cart.test.ts", "extra.test.ts"]);
    let undone = undo(&dir, &snap, &files);
    assert_eq!(undone.restored, ["cart.test.ts"]);
    assert_eq!(undone.removed, ["extra.test.ts"]);
    assert!(undone.skipped.is_empty());
    assert_eq!(fs::read(dir.join("cart.test.ts")).unwrap(), original, "byte-equal to the start");
    assert_eq!(fs::read_to_string(dir.join("cart.ts")).unwrap(), "export const total = (p: number[]) => p.length;\n");
    assert!(dir.join("notes.test.ts").exists(), "a new test file without a finding stays");
    assert!(!check(&dir, &snap, &CheckOptions::default()).blocks);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn undo_refuses_paths_outside_the_project() {
    let dir = scratch("undo-escape");
    fs::write(dir.join("a.test.ts"), "it('a', () => { expect(1).toBe(1); });\n").unwrap();
    let snap = take(&dir, &SnapshotOptions::default()).unwrap();
    let report = undo(&dir, &snap, &["../evil.txt".to_string(), "/etc/passwd".to_string(), "C:\\x".to_string()]);
    assert!(report.restored.is_empty() && report.removed.is_empty());
    assert_eq!(report.skipped.len(), 3);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn snapshots_survive_a_save_and_load_and_reject_bad_ids() {
    let case = fixtures_dir().join("js_skip_added");
    let dir = scratch("store");
    copy_side(&case.join("before"), &dir);
    let snap = take_and_save(&dir, &SnapshotOptions { id: Some("mission-7_start".into()), ..SnapshotOptions::default() }).unwrap();
    assert_eq!(load(&dir, "mission-7_start").unwrap(), snap);
    assert_eq!(list(&dir), ["mission-7_start"]);
    assert!(matches!(load(&dir, "../x"), Err(TamperError::BadId(_))));
    assert!(matches!(load(&dir, "nope"), Err(TamperError::NotFound(_))));
    assert!(matches!(take(&dir, &SnapshotOptions { id: Some("a/b".into()), ..SnapshotOptions::default() }), Err(TamperError::BadId(_))));
    delete(&dir, "mission-7_start").unwrap();
    assert!(list(&dir).is_empty());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_corrupt_snapshot_is_an_error_not_a_panic() {
    let dir = scratch("corrupt");
    fs::create_dir_all(dir.join(".puppet-master").join("tamper")).unwrap();
    fs::write(dir.join(".puppet-master").join("tamper").join("bad.json"), "{not json").unwrap();
    assert!(matches!(load(&dir, "bad"), Err(TamperError::Corrupt(_))));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn project_toml_can_protect_and_ignore() {
    let dir = scratch("project-toml");
    fs::create_dir_all(dir.join(".puppet-master")).unwrap();
    fs::write(dir.join(".puppet-master").join("project.toml"), "[tamper]\nprotect = [\"checks/**/*.js\"]\nignore = [\"**/gen/**\"]\n").unwrap();
    fs::create_dir_all(dir.join("checks")).unwrap();
    fs::create_dir_all(dir.join("gen")).unwrap();
    fs::write(dir.join("checks").join("money.js"), "it('a', () => { expect(1).toBe(1); });\n").unwrap();
    fs::write(dir.join("gen").join("x.test.js"), "it('g', () => {});\n").unwrap();
    let snap = take(&dir, &SnapshotOptions::default()).unwrap();
    assert!(snap.files.contains_key("checks/money.js"));
    assert!(!snap.files.contains_key("gen/x.test.js"));
    fs::write(dir.join("checks").join("money.js"), "it.skip('a', () => { expect(1).toBe(1); });\n").unwrap();
    assert!(check(&dir, &snap, &CheckOptions::default()).blocks);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn files_in_dependency_and_build_folders_are_never_protected() {
    let dir = scratch("never");
    for d in ["node_modules/pkg", "target/debug", "dist", ".venv/lib"] {
        fs::create_dir_all(dir.join(d)).unwrap();
        fs::write(dir.join(d).join("x.test.js"), "it('x', () => { expect(1).toBe(1); });\n").unwrap();
    }
    fs::write(dir.join("real.test.js"), "it('r', () => { expect(1).toBe(1); });\n").unwrap();
    let snap = take(&dir, &SnapshotOptions::default()).unwrap();
    assert_eq!(snap.files.keys().collect::<Vec<_>>(), ["real.test.js"]);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_file_over_the_size_limit_keeps_its_hash_and_the_report_says_it_is_partial() {
    let dir = scratch("big");
    let big = format!("it('a', () => {{ expect(1).toBe(1); }});\n{}", "// padding\n".repeat(120_000));
    fs::write(dir.join("big.test.js"), &big).unwrap();
    let snap = take(&dir, &SnapshotOptions::default()).unwrap();
    assert!(snap.partial);
    assert!(snap.files["big.test.js"].content.is_none());
    fs::write(dir.join("big.test.js"), format!("{big}// more\n")).unwrap();
    let report = check(&dir, &snap, &CheckOptions::default());
    assert!(report.partial);
    assert!(report.findings.iter().any(|f| f.kind == Kind::CantRead), "{}", dump(&report));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_command_changes_are_compared_when_the_caller_passes_them() {
    let dir = scratch("argv");
    fs::write(dir.join("a.test.js"), "it('a', () => { expect(1).toBe(1); });\n").unwrap();
    let snap = take(
        &dir,
        &SnapshotOptions { check_argvs: vec![vec!["npx".into(), "vitest".into(), "run".into()]], ..SnapshotOptions::default() },
    )
    .unwrap();
    let same = CheckOptions { check_argvs: Some(snap.check_argvs.clone()), ..CheckOptions::default() };
    assert!(!check(&dir, &snap, &same).blocks);
    let filtered = CheckOptions {
        check_argvs: Some(vec![vec!["npx".into(), "vitest".into(), "run".into(), "-t".into(), "cart".into()]]),
        ..CheckOptions::default()
    };
    let report = check(&dir, &snap, &filtered);
    assert!(report.blocks && report.findings[0].kind == Kind::FocusAdded, "{}", dump(&report));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn skipped_count_rising_is_a_fewer_tests_finding() {
    let dir = scratch("skipped-count");
    fs::write(dir.join("a.test.js"), "it('a', () => { expect(1).toBe(1); });\n").unwrap();
    let snap = take(&dir, &SnapshotOptions { counts: Some(RunCounts { total: Some(5), skipped: Some(0), ..Default::default() }), ..Default::default() }).unwrap();
    let report = check(&dir, &snap, &CheckOptions { counts: Some(RunCounts { total: Some(5), skipped: Some(2), ..Default::default() }), ..Default::default() });
    assert_eq!(report.findings.len(), 1);
    assert_eq!(report.findings[0].summary, "0 tests were skipped at the start; 2 tests were skipped now");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn words_follow_the_ux_vocabulary() {
    // No finding speaks of cheating, tampering, baselines, argv or ids (UX-PHASE1 section 0.3).
    let banned = ["cheat", "tamper", "baseline", "argv", "snapshot id", "worktree"];
    let mut all = String::new();
    for case in case_dirs() {
        let name = case.file_name().unwrap().to_string_lossy().to_string();
        let (report, _) = run_case(&case, &name);
        for f in &report.findings {
            all.push_str(&f.summary.to_lowercase());
            all.push('\n');
        }
        assert!(!report.headline.is_empty());
    }
    for word in banned {
        assert!(!all.contains(word), "a finding says {word:?}");
    }
}

#[test]
fn http_route_checks_a_saved_snapshot_and_is_read_only() {
    let case = fixtures_dir().join("js_skip_added");
    let dir = scratch("http");
    copy_side(&case.join("before"), &dir);
    take_and_save(&dir, &SnapshotOptions { id: Some("start".into()), ..Default::default() }).unwrap();
    clear_except_store(&dir);
    copy_side(&case.join("after"), &dir);
    let before = tree_hash(&dir);
    let project = dir.to_string_lossy().to_string();
    let param = |p: &str| match p {
        "project" => Some(project.clone()),
        "since" => Some("start".to_string()),
        _ => None,
    };
    let (status, body) = http::respond("GET", &["tamper", "check"], &param).unwrap().unwrap();
    assert_eq!(status, 200);
    assert_eq!(body["blocks"], true);
    assert_eq!(body["findings"][0]["kind"], "skip_added");
    assert_eq!(body["findings"][0]["severity"], "blocks");
    assert_eq!(body["findings"][0]["file"], "cart.test.ts");
    assert_eq!(tree_hash(&dir), before, "the route must not write");

    // Not a tamper route, or not GET: left to the other route groups.
    assert!(http::respond("POST", &["tamper", "check"], &param).is_none());
    assert!(http::respond("GET", &["tamper"], &param).is_none());
    // Missing parameters, a bad id, an unknown snapshot, a folder without snapshots.
    let none = |_: &str| None;
    assert_eq!(http::respond("GET", &["tamper", "check"], &none).unwrap().unwrap_err().0, 400);
    let bad = |p: &str| if p == "since" { Some("../x".to_string()) } else { Some(project.clone()) };
    assert_eq!(http::respond("GET", &["tamper", "check"], &bad).unwrap().unwrap_err().0, 400);
    let unknown = |p: &str| if p == "since" { Some("nope".to_string()) } else { Some(project.clone()) };
    let (status, err) = http::respond("GET", &["tamper", "check"], &unknown).unwrap().unwrap_err();
    assert_eq!((status, err["code"].as_str()), (404, Some("TAMPER_SNAPSHOT_NOT_FOUND")));
    let elsewhere = scratch("http-elsewhere");
    let other = |p: &str| if p == "project" { Some(elsewhere.to_string_lossy().to_string()) } else { Some("start".to_string()) };
    assert_eq!(http::respond("GET", &["tamper", "check"], &other).unwrap().unwrap_err().0, 404);
    let _ = fs::remove_dir_all(&dir);
    let _ = fs::remove_dir_all(&elsewhere);
}

fn clear_except_store(dir: &Path) {
    clear(dir);
}

#[test]
fn the_engine_never_merges_pushes_or_opens_a_pull_request() {
    // DESIGN-MISSIONS section 7: the tamper sources contain none of these.
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("tamper");
    for entry in fs::read_dir(&src).unwrap().flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") || path.file_name().unwrap() == "tests.rs" {
            continue;
        }
        let text = fs::read_to_string(&path).unwrap();
        for banned in ["worktrees::merge", "merge_keep", "pr::create", "review_worktree", "git push", "Command::new"] {
            assert!(!text.contains(banned), "{} mentions {banned}", path.display());
        }
    }
}

#[test]
fn no_language_is_named_in_the_engine_code() {
    // Language knowledge lives in rules/*.toml. Comments may explain; code may not match on it.
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("tamper");
    for entry in fs::read_dir(&src).unwrap().flatten() {
        let path = entry.path();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") || name == "tests.rs" || name == "rules.rs" {
            continue;
        }
        let text = fs::read_to_string(&path).unwrap();
        // Unit tests inside a module legitimately hold sample code in several languages.
        let code = text.split("#[cfg(test)]").next().unwrap_or("");
        for line in code.lines().filter(|l| !l.trim_start().starts_with("//")) {
            for banned in ["pytest", "ts-ignore", "eslint", "jest", "vitest", "it.skip", "#[ignore]", "t.Skip", "xit(", "package.json"] {
                assert!(!line.contains(banned), "{name} matches on {banned:?}: {line}");
            }
        }
    }
}

/// Manual: `TAMPER_BEFORE=<dir> TAMPER_AFTER=<dir> cargo test --lib compare_two_folders -- --ignored --nocapture`.
/// Prints what the guard says about two real trees (for example two commits exported with `git archive`).
#[test]
#[ignore]
fn compare_two_folders() {
    let (Ok(before), Ok(after)) = (std::env::var("TAMPER_BEFORE"), std::env::var("TAMPER_AFTER")) else { return };
    let started = std::time::Instant::now();
    let work = scratch("compare");
    copy_plain(Path::new(&before), &work);
    let snap = take(&work, &SnapshotOptions::default()).unwrap();
    println!("snapshot: {} protected files, {} ms", snap.files.len(), started.elapsed().as_millis());
    clear(&work);
    copy_plain(Path::new(&after), &work);
    let started = std::time::Instant::now();
    let report = check(&work, &snap, &CheckOptions::default());
    println!("check: {} ms, {} files changed, blocks = {}", started.elapsed().as_millis(), report.files_changed, report.blocks);
    println!("{}", dump(&report));
    let _ = fs::remove_dir_all(&work);
}

fn copy_plain(src: &Path, dst: &Path) {
    for entry in fs::read_dir(src).unwrap().flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            if name == ".git" || name == "node_modules" || name == "target" {
                continue;
            }
            let sub = dst.join(&name);
            fs::create_dir_all(&sub).unwrap();
            copy_plain(&path, &sub);
        } else {
            let _ = fs::copy(&path, dst.join(&name));
        }
    }
}

#[test]
fn windows_line_endings_survive_a_snapshot_and_an_undo_byte_for_byte() {
    let dir = scratch("crlf");
    let original = "it('a', () => {\r\n  expect(1 + 1).toBe(2);\r\n});\r\n";
    fs::write(dir.join("a.test.js"), original).unwrap();
    let snap = take(&dir, &SnapshotOptions::default()).unwrap();
    fs::write(dir.join("a.test.js"), original.replace("it('a'", "it.skip('a'")).unwrap();
    let report = check(&dir, &snap, &CheckOptions::default());
    assert_eq!(report.findings.len(), 1, "{}", dump(&report));
    assert_eq!(report.findings[0].kind, Kind::SkipAdded);
    undo(&dir, &snap, &report.blocking_files());
    assert_eq!(fs::read(dir.join("a.test.js")).unwrap(), original.as_bytes());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn rust_unit_tests_are_counted_from_the_first_test_marker_only() {
    // Covered by the `rs_impl_assert_removed` fixture; this checks the snapshot's own counts.
    let dir = scratch("region");
    fs::write(
        dir.join("lib.rs"),
        "pub fn f(x: i32) -> i32 { assert!(x > 0); x }\n#[cfg(test)]\nmod t {\n    #[test]\n    fn a() { assert_eq!(super::f(1), 1); }\n}\n",
    )
    .unwrap();
    let snap = take(&dir, &SnapshotOptions::default()).unwrap();
    assert_eq!((snap.files["lib.rs"].tests, snap.files["lib.rs"].assertions), (1, 1));
    let _ = fs::remove_dir_all(&dir);
}
