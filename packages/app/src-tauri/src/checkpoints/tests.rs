//! Integration tests: real `git`, real temp folders. No app, no windows.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use sha2::{Digest, Sha256};

use super::engine::{self, CheckpointError, Kind, Limits, Snapshot};
use super::runtime::{last_turn_start, Env, Outcome, PaneFacts, Runtime, Settings, DEBOUNCE_MS};

struct Tmp(PathBuf);

impl Tmp {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("pm-ckpt-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        Tmp(dir)
    }
    fn p(&self) -> &Path {
        &self.0
    }
    fn write(&self, rel: &str, text: &str) {
        let path = self.0.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    fn read(&self, rel: &str) -> Option<String> {
        std::fs::read_to_string(self.0.join(rel)).ok()
    }
    fn exists(&self, rel: &str) -> bool {
        self.0.join(rel).exists()
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        // git object files are read-only on Windows.
        fn unlock(dir: &Path) {
            if let Ok(rd) = std::fs::read_dir(dir) {
                for e in rd.flatten() {
                    let p = e.path();
                    if p.is_dir() {
                        unlock(&p);
                    } else if let Ok(m) = std::fs::metadata(&p) {
                        let mut perms = m.permissions();
                        #[allow(clippy::permissions_set_readonly_false)]
                        perms.set_readonly(false);
                        let _ = std::fs::set_permissions(&p, perms);
                    }
                }
            }
        }
        unlock(&self.0);
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn limits() -> Limits {
    Limits::default()
}

fn created(s: Snapshot) -> engine::Checkpoint {
    match s {
        Snapshot::Created(c) => c,
        Snapshot::Unchanged(_) => panic!("expected a new checkpoint"),
    }
}

fn snap(t: &Tmp, pane: &str, kind: Kind, at: u64) -> engine::Checkpoint {
    created(engine::snapshot(t.p(), pane, kind, None, at, &limits()).expect("snapshot"))
}

fn user_git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_CONFIG_GLOBAL", "")
        .output()
        .expect("git");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn sha(path: &Path) -> String {
    let bytes = std::fs::read(path).unwrap();
    format!("{:x}", Sha256::digest(bytes))
}

#[test]
fn non_git_folder_snapshot_modify_restore() {
    let t = Tmp::new("plain");
    t.write("a.txt", "one");
    t.write("sub/b.txt", "bee");
    t.write(".gitignore", "ignored.log\n");
    t.write("ignored.log", "secret v1");
    let first = snap(&t, "p1", Kind::Before, 1_000);
    assert_eq!(first.label, "Before turn 1");
    assert!(!t.p().join(".git").exists(), "the project must stay a non-git folder");

    t.write("a.txt", "two");
    std::fs::remove_file(t.p().join("sub/b.txt")).unwrap();
    t.write("new.txt", "fresh");
    t.write("sub/deep/c.txt", "deep");
    t.write("ignored.log", "secret v2");
    let second = snap(&t, "p1", Kind::After, 20_000);
    assert_eq!(second.label, "After turn 1");

    let report = engine::restore(t.p(), "p1", &first.id, false, 40_000, &limits()).expect("restore");
    assert_eq!(t.read("a.txt").as_deref(), Some("one"));
    assert_eq!(t.read("sub/b.txt").as_deref(), Some("bee"));
    assert!(!t.exists("new.txt"), "files created after the checkpoint are removed");
    assert!(!t.exists("sub/deep"), "emptied folders are removed too");
    assert_eq!(t.read("ignored.log").as_deref(), Some("secret v2"), "ignored files are never touched");
    assert!(report.removed >= 2 && report.written >= 2, "{report:?}");
    assert!(report.failed.is_empty());

    // The safety snapshot makes the undo undoable.
    let items = engine::list(t.p(), "p1").unwrap();
    let safety = items.iter().find(|c| Some(&c.id) == report.safety_id.as_ref());
    assert!(safety.is_none() || safety.unwrap().kind == "after" || safety.unwrap().kind == "undo");
    engine::restore(t.p(), "p1", &second.id, false, 60_000, &limits()).expect("redo");
    assert_eq!(t.read("a.txt").as_deref(), Some("two"));
    assert!(t.exists("new.txt"));
    assert!(t.exists("sub/deep/c.txt"));
    assert!(!t.exists("sub/b.txt"));
}

#[test]
fn restore_takes_a_before_undo_safety_snapshot_first() {
    let t = Tmp::new("safety");
    t.write("a.txt", "v1");
    let first = snap(&t, "p1", Kind::Before, 1_000);
    t.write("a.txt", "v2 unsaved by any checkpoint");
    t.write("extra.txt", "x");
    let report = engine::restore(t.p(), "p1", &first.id, false, 30_000, &limits()).unwrap();
    assert_eq!(t.read("a.txt").as_deref(), Some("v1"));
    let safety_id = report.safety_id.expect("safety id");
    let items = engine::list(t.p(), "p1").unwrap();
    let safety = items.iter().find(|c| c.id == safety_id).expect("safety listed");
    assert_eq!(safety.kind, "undo");
    assert_eq!(safety.label, "Before undo");
    engine::restore(t.p(), "p1", &safety_id, false, 50_000, &limits()).unwrap();
    assert_eq!(t.read("a.txt").as_deref(), Some("v2 unsaved by any checkpoint"));
    assert_eq!(t.read("extra.txt").as_deref(), Some("x"));
}

#[test]
fn users_git_repo_is_byte_identical_after_snapshot_and_restore() {
    let t = Tmp::new("usergit");
    user_git(t.p(), &["init", "-q", "-b", "main"]);
    user_git(t.p(), &["config", "user.email", "u@example.com"]);
    user_git(t.p(), &["config", "user.name", "U"]);
    t.write("tracked.txt", "t1");
    t.write("staged.txt", "s1");
    user_git(t.p(), &["add", "tracked.txt"]);
    user_git(t.p(), &["commit", "-q", "-m", "init"]);
    user_git(t.p(), &["branch", "side"]);
    user_git(t.p(), &["add", "staged.txt"]);
    t.write("untracked.txt", "u1");

    let index_before = sha(&t.p().join(".git/index"));
    let head_before = user_git(t.p(), &["rev-parse", "HEAD"]);
    let branches_before = user_git(t.p(), &["branch", "-a", "--format=%(refname) %(objectname)"]);
    let status_before = user_git(t.p(), &["status", "--porcelain"]);
    let refs_before = user_git(t.p(), &["for-each-ref"]);

    let first = snap(&t, "p1", Kind::Before, 1_000);
    // The agent edits files, then we restore.
    t.write("tracked.txt", "t2 changed by agent");
    t.write("untracked.txt", "u2");
    t.write("created-by-agent.txt", "n");
    let _second = snap(&t, "p1", Kind::After, 20_000);
    engine::restore(t.p(), "p1", &first.id, false, 40_000, &limits()).unwrap();

    assert_eq!(t.read("tracked.txt").as_deref(), Some("t1"));
    assert!(!t.exists("created-by-agent.txt"));
    assert_eq!(user_git(t.p(), &["rev-parse", "HEAD"]), head_before);
    assert_eq!(user_git(t.p(), &["branch", "-a", "--format=%(refname) %(objectname)"]), branches_before);
    assert_eq!(user_git(t.p(), &["for-each-ref"]), refs_before, "no pm refs leak into the user's repo");
    // `.puppet-master/` (also home of the operations store) shows as untracked in the user's
    // status, as it already does today; everything else must be exactly as before.
    let status_after: Vec<String> = user_git(t.p(), &["status", "--porcelain"])
        .lines()
        .filter(|l| !l.contains(".puppet-master/"))
        .map(String::from)
        .collect();
    assert_eq!(status_after.join("
"), status_before);
    assert_eq!(sha(&t.p().join(".git/index")), index_before, "the user's index file is byte-identical");
    assert!(!t.p().join(".git/refs/pm").exists());
}

#[test]
fn unchanged_tree_is_skipped_and_ids_keep_counting() {
    let t = Tmp::new("unchanged");
    t.write("a.txt", "1");
    let a = snap(&t, "p1", Kind::Before, 1_000);
    let again = engine::snapshot(t.p(), "p1", Kind::After, None, 2_000, &limits()).unwrap();
    assert!(matches!(again, Snapshot::Unchanged(Some(ref c)) if c.id == a.id));
    t.write("a.txt", "2");
    let b = snap(&t, "p1", Kind::After, 3_000);
    assert_eq!(b.id, "2");
    assert_eq!((b.files, b.added, b.deleted), (1, 1, 1));
    let changes = engine::diff(t.p(), "p1", &b.id).unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!((changes[0].path.as_str(), changes[0].status.as_str()), ("a.txt", "M"));
    let first_diff = engine::diff(t.p(), "p1", &a.id).unwrap();
    assert_eq!(first_diff[0].status, "A");
}

#[test]
fn retention_keeps_only_the_newest_n_per_pane() {
    let t = Tmp::new("retain");
    let small = Limits { keep: 5, ..Limits::default() };
    for i in 0..9 {
        t.write("a.txt", &format!("v{i}"));
        created(engine::snapshot(t.p(), "p1", Kind::After, None, 1_000 + i, &small).unwrap());
    }
    t.write("other.txt", "o");
    created(engine::snapshot(t.p(), "p2", Kind::Before, None, 5_000, &small).unwrap());
    let ids: Vec<String> = engine::list(t.p(), "p1").unwrap().into_iter().map(|c| c.id).collect();
    assert_eq!(ids, ["5", "6", "7", "8", "9"]);
    assert_eq!(engine::list(t.p(), "p2").unwrap().len(), 1, "another pane's refs are not pruned");
    assert_eq!(engine::prune(t.p(), "p1", 2).unwrap(), 3);
    assert_eq!(engine::list(t.p(), "p1").unwrap().len(), 2);
}

#[test]
fn big_files_are_left_out_and_never_deleted() {
    let t = Tmp::new("big");
    let tiny = Limits { max_file_bytes: 100, ..Limits::default() };
    t.write("small.txt", "ok");
    t.write("huge.bin", &"x".repeat(500));
    let first = created(engine::snapshot(t.p(), "p1", Kind::Before, None, 1_000, &tiny).unwrap());
    let changes = engine::diff(t.p(), "p1", &first.id).unwrap();
    assert_eq!(changes.iter().map(|c| c.path.as_str()).collect::<Vec<_>>(), ["small.txt"]);
    t.write("small.txt", "changed");
    engine::restore(t.p(), "p1", &first.id, false, 30_000, &tiny).unwrap();
    assert_eq!(t.read("small.txt").as_deref(), Some("ok"));
    assert!(t.exists("huge.bin"), "an oversized file is never removed by undo");
}

#[test]
fn total_size_guard_refuses_a_giant_snapshot() {
    let t = Tmp::new("total");
    let cap = Limits { max_total_bytes: 50, ..Limits::default() };
    t.write("a.txt", &"x".repeat(80));
    let err = engine::snapshot(t.p(), "p1", Kind::Before, None, 1_000, &cap).unwrap_err();
    assert!(matches!(err, CheckpointError::TooLarge(_)), "{err}");
}

#[test]
fn restore_is_refused_while_the_pane_runs() {
    let t = Tmp::new("running");
    t.write("a.txt", "1");
    let first = snap(&t, "p1", Kind::Before, 1_000);
    t.write("a.txt", "2");
    let err = engine::restore(t.p(), "p1", &first.id, true, 2_000, &limits()).unwrap_err();
    assert!(matches!(err, CheckpointError::Running));
    assert_eq!(t.read("a.txt").as_deref(), Some("2"), "nothing is touched");
    assert_eq!(engine::list(t.p(), "p1").unwrap().len(), 1, "not even a safety snapshot");
}

// ---- runtime (turn boundaries) ------------------------------------------------------------

struct FakeEnv {
    panes: Mutex<HashMap<String, PaneFacts>>,
    now: AtomicU64,
    enabled: AtomicBool,
    opted_out: Mutex<Vec<String>>,
    run_open: AtomicBool,
}

impl FakeEnv {
    fn new(dir: &Path) -> Arc<Self> {
        let env = Arc::new(Self {
            panes: Mutex::new(HashMap::new()),
            now: AtomicU64::new(100_000),
            enabled: AtomicBool::new(true),
            opted_out: Mutex::new(vec![]),
            run_open: AtomicBool::new(false),
        });
        env.add("p1", dir, "claude", "idle");
        env
    }
    fn add(&self, id: &str, dir: &Path, agent: &str, status: &str) {
        self.panes.lock().insert(
            id.into(),
            PaneFacts { cwd: dir.to_string_lossy().into(), agent_type: agent.into(), status: status.into() },
        );
    }
    fn set_status(&self, id: &str, status: &str) {
        self.panes.lock().get_mut(id).unwrap().status = status.into();
    }
    fn advance(&self, ms: u64) {
        self.now.fetch_add(ms, Ordering::SeqCst);
    }
}

impl Env for FakeEnv {
    fn pane(&self, id: &str) -> Option<PaneFacts> {
        self.panes.lock().get(id).cloned()
    }
    fn settings(&self) -> Settings {
        Settings { enabled: self.enabled.load(Ordering::SeqCst), keep: 50 }
    }
    fn now_ms(&self) -> u64 {
        self.now.load(Ordering::SeqCst)
    }
    fn run_open(&self, _pane_id: &str, _pane: &PaneFacts) -> bool {
        self.run_open.load(Ordering::SeqCst)
    }
    fn agent_takes_part(&self, agent: &str) -> bool {
        !self.opted_out.lock().iter().any(|a| a == agent)
    }
}

#[test]
fn status_transitions_map_to_turn_boundaries() {
    let t = Tmp::new("decide");
    let rt = Runtime::new(FakeEnv::new(t.p()));
    assert_eq!(rt.decide("p1", "idle"), None);
    assert_eq!(rt.decide("p1", "running"), Some(Kind::Before));
    assert_eq!(rt.decide("p1", "running"), None, "no change, no snapshot");
    assert_eq!(rt.decide("p1", "waiting_input"), Some(Kind::After));
    assert_eq!(rt.decide("p1", "running"), Some(Kind::Before));
    assert_eq!(rt.decide("p1", "idle"), Some(Kind::After));
    assert_eq!(rt.decide("p1", "error"), None, "idle -> error is not a turn end");
}

#[test]
fn debounce_allows_one_snapshot_per_pane_per_ten_seconds() {
    let t = Tmp::new("debounce");
    let env = FakeEnv::new(t.p());
    let rt = Runtime::new(env.clone());
    t.write("a.txt", "1");
    assert!(matches!(rt.capture("p1", Kind::Before, Some("fix the bug")), Outcome::Created(_)));
    t.write("a.txt", "2");
    env.advance(3_000);
    match rt.capture("p1", Kind::After, None) {
        Outcome::Debounced { retry_in_ms } => assert_eq!(retry_in_ms, DEBOUNCE_MS - 3_000),
        other => panic!("expected debounce, got {other:?}"),
    }
    assert_eq!(engine::list(t.p(), "p1").unwrap().len(), 1);
    env.advance(DEBOUNCE_MS);
    assert!(matches!(rt.capture("p1", Kind::After, None), Outcome::Created(_)));
    // Same tree again, window open: skipped as unchanged, not duplicated.
    env.advance(DEBOUNCE_MS + 1);
    assert!(matches!(rt.capture("p1", Kind::Before, None), Outcome::Unchanged));
    assert_eq!(engine::list(t.p(), "p1").unwrap().len(), 2);
    // Another pane has its own window.
    env.add("p2", t.p(), "claude", "idle");
    assert!(!matches!(rt.capture("p2", Kind::Before, None), Outcome::Debounced { .. }));
}

#[test]
fn off_switch_and_profile_opt_out_skip_snapshots() {
    let t = Tmp::new("optout");
    let env = FakeEnv::new(t.p());
    let rt = Runtime::new(env.clone());
    t.write("a.txt", "1");
    env.enabled.store(false, Ordering::SeqCst);
    assert!(matches!(rt.capture("p1", Kind::Before, None), Outcome::Skipped("off")));
    assert!(matches!(rt.list("p1"), Err(CheckpointError::Disabled)));
    env.enabled.store(true, Ordering::SeqCst);
    env.opted_out.lock().push("claude".into());
    assert!(matches!(rt.capture("p1", Kind::Before, None), Outcome::Skipped("agent opted out")));
    assert!(!t.p().join(".puppet-master").exists(), "nothing was created");
}

#[test]
fn runtime_restore_refuses_a_running_pane_and_undo_last_turn_works() {
    let t = Tmp::new("undo");
    let env = FakeEnv::new(t.p());
    let rt = Runtime::new(env.clone());
    t.write("a.txt", "original");
    assert!(matches!(rt.capture("p1", Kind::Before, Some("rewrite a")), Outcome::Created(_)));
    env.set_status("p1", "running");
    t.write("a.txt", "agent rewrote it");
    t.write("junk.txt", "j");
    env.advance(DEBOUNCE_MS + 1);
    assert!(matches!(rt.capture("p1", Kind::After, None), Outcome::Created(_)));
    let id = rt.list("p1").unwrap()[0].id.clone();
    let err = rt.restore("p1", &id).unwrap_err();
    assert!(matches!(err, CheckpointError::Running));
    assert!(err.to_string().contains("Interrupt"));
    assert_eq!(t.read("a.txt").as_deref(), Some("agent rewrote it"));

    env.set_status("p1", "idle");
    env.advance(1_000);
    let items = rt.list("p1").unwrap();
    assert_eq!(last_turn_start(&items).unwrap().id, id);
    assert_eq!(items[0].note, "rewrite a");
    rt.undo_last_turn("p1").unwrap();
    assert_eq!(t.read("a.txt").as_deref(), Some("original"));
    assert!(!t.exists("junk.txt"));
}

#[test]
fn two_panes_in_one_folder_keep_separate_refs() {
    let t = Tmp::new("twopanes");
    t.write("a.txt", "1");
    let a = snap(&t, "p1", Kind::Before, 1_000);
    t.write("b.txt", "2");
    let b = snap(&t, "p2", Kind::Before, 2_000);
    assert_eq!((a.id.as_str(), b.id.as_str()), ("1", "1"));
    assert_eq!(engine::list(t.p(), "p1").unwrap().len(), 1);
    assert_eq!(engine::list(t.p(), "p2").unwrap().len(), 1);
    assert_eq!(engine::diff(t.p(), "p2", "1").unwrap().len(), 2, "p2's first checkpoint holds both files");
}

// ---- HTTP routes (in-process, no socket) -------------------------------------------------

#[test]
fn http_routes_list_diff_and_restore_with_error_codes() {
    use super::http::respond;
    let t = Tmp::new("http");
    let env = FakeEnv::new(t.p());
    let rt = Runtime::new(env.clone());
    t.write("a.txt", "v1");
    assert!(matches!(rt.capture("p1", Kind::Before, Some("first")), Outcome::Created(_)));
    t.write("a.txt", "v2");
    env.advance(DEBOUNCE_MS + 1);
    assert!(matches!(rt.capture("p1", Kind::After, None), Outcome::Created(_)));

    let (code, body) = respond(&rt, "GET", &["panes", "p1", "checkpoints"]).unwrap().unwrap();
    assert_eq!(code, 200);
    assert_eq!(body["state"], "ok");
    assert_eq!(body["items"][0]["label"], "After turn 1", "newest first: {body}");
    assert_eq!(body["items"][1]["note"], "first");

    let (code, body) = respond(&rt, "GET", &["panes", "p1", "checkpoints", "2", "diff"]).unwrap().unwrap();
    assert_eq!((code, body["changes"][0]["path"].as_str(), body["changes"][0]["status"].as_str()), (200, Some("a.txt"), Some("M")));

    env.set_status("p1", "running");
    let (code, body) = respond(&rt, "POST", &["panes", "p1", "checkpoints", "1", "restore"]).unwrap().unwrap_err();
    assert_eq!((code, body["code"].as_str()), (409, Some("PANE_RUNNING")));
    assert_eq!(t.read("a.txt").as_deref(), Some("v2"));

    env.set_status("p1", "idle");
    let (code, body) = respond(&rt, "POST", &["panes", "p1", "checkpoints", "1", "restore"]).unwrap().unwrap();
    assert_eq!(code, 200);
    assert!(body["safetyId"].is_string(), "{body}");
    assert_eq!(t.read("a.txt").as_deref(), Some("v1"));

    let (code, body) = respond(&rt, "POST", &["panes", "p1", "checkpoints", "99", "restore"]).unwrap().unwrap_err();
    assert_eq!((code, body["code"].as_str()), (404, Some("CHECKPOINT_NOT_FOUND")));

    env.enabled.store(false, Ordering::SeqCst);
    let (_, body) = respond(&rt, "GET", &["panes", "p1", "checkpoints"]).unwrap().unwrap();
    assert_eq!(body["state"], "off");
    assert!(respond(&rt, "GET", &["panes", "p1", "input"]).is_none());
}

#[test]
fn profiles_decide_participation_not_agent_names() {
    for shell in ["cmd", "powershell", "bash"] {
        assert!(!crate::tui::profile_for(shell).checkpoints(), "{shell} has no turns");
    }
    for agent in ["claude", "codex", "cursor_agent", "opencode"] {
        assert!(crate::tui::profile_for(agent).checkpoints(), "{agent}");
    }
}

#[test]
fn turn_changes_diffs_before_turn_against_after_turn() {
    let t = Tmp::new("turnchanges");
    t.write("a.txt", "one\ntwo\n");
    t.write("keep.txt", "same");
    // Turn 1: the baseline, then edits during the turn, then the end-of-turn snapshot.
    snap(&t, "p1", Kind::Before, 1_000);
    t.write("a.txt", "one\ntwo\nthree\nfour\n");
    t.write("new.txt", "fresh\n");
    snap(&t, "p1", Kind::Answer, 5_000);
    t.write("new.txt", "fresh\nmore\n");
    snap(&t, "p1", Kind::After, 20_000);
    // Asked at the end of turn 1, as the app does.
    let one = engine::turn_changes(t.p(), "p1", 1_000).unwrap().expect("baseline exists");
    let paths: Vec<&str> = one.iter().map(|c| c.path.as_str()).collect();
    assert_eq!(paths, vec!["a.txt", "new.txt"], "turn 1 touched exactly these files");
    assert_eq!(one.iter().map(|c| c.added).sum::<u32>(), 4);

    // Turn 2 changes one more file; its summary must not include turn 1's files.
    // The tree has not changed since the last "after", so this snapshot is skipped (no checkpoint).
    assert!(matches!(engine::snapshot(t.p(), "p1", Kind::Before, None, 40_000, &limits()).unwrap(), Snapshot::Unchanged(_)));
    t.write("keep.txt", "changed");
    snap(&t, "p1", Kind::After, 60_000);

    let two = engine::turn_changes(t.p(), "p1", 40_000).unwrap().expect("baseline exists");
    assert_eq!(two.iter().map(|c| c.path.as_str()).collect::<Vec<_>>(), vec!["keep.txt"]);

    // No checkpoint at or before the start (checkpoints off, or a pane never snapshotted): unknown.
    assert!(engine::turn_changes(t.p(), "other-pane", 1_000).unwrap().is_none());
    // A turn that started well before the pane's first checkpoint has no baseline either.
    let late = Tmp::new("turnlate");
    late.write("a.txt", "one");
    snap(&late, "p1", Kind::Before, 50_000);
    assert!(engine::turn_changes(late.p(), "p1", 10_000).unwrap().is_none());
}

#[test]
fn turn_changes_is_empty_when_nothing_changed() {
    let t = Tmp::new("turnnochange");
    t.write("a.txt", "one");
    snap(&t, "p1", Kind::Before, 1_000);
    let none = engine::turn_changes(t.p(), "p1", 1_000).unwrap().expect("baseline exists");
    assert!(none.is_empty());
}

#[test]
fn last_turn_patch_is_the_newest_after_against_where_that_turn_started() {
    let t = Tmp::new("turnpatch");
    t.write("a.txt", "one\ntwo\n");
    assert!(engine::last_turn_patch(t.p(), "p1", 1 << 20).unwrap().is_none(), "never snapshotted");
    snap(&t, "p1", Kind::Before, 1_000);
    t.write("a.txt", "one\ntwo\nthree\n");
    // Still running: no "after" yet.
    assert!(engine::last_turn_patch(t.p(), "p1", 1 << 20).unwrap().is_none());
    snap(&t, "p1", Kind::After, 20_000);
    let one = engine::last_turn_patch(t.p(), "p1", 1 << 20).unwrap().expect("a finished turn");
    assert!(one.patch.contains("diff --git a/a.txt b/a.txt") && one.patch.contains("+three"));
    assert!(!one.truncated);

    // Turn 2: its "before" is skipped (nothing changed since turn 1 ended) but the patch must be
    // turn 2's change only.
    assert!(matches!(engine::snapshot(t.p(), "p1", Kind::Before, None, 40_000, &limits()).unwrap(), Snapshot::Unchanged(_)));
    t.write("b.txt", "new file\n");
    snap(&t, "p1", Kind::After, 60_000);
    let two = engine::last_turn_patch(t.p(), "p1", 1 << 20).unwrap().expect("a finished turn");
    assert!(two.patch.contains("b/b.txt") && two.patch.contains("+new file"));
    assert!(!two.patch.contains("a.txt"), "turn 1's change is not part of turn 2");

    // The cap cuts and says so.
    let cut = engine::last_turn_patch(t.p(), "p1", 20).unwrap().unwrap();
    assert!(cut.truncated && cut.patch.len() <= 20);
    // Another pane has none.
    assert!(engine::last_turn_patch(t.p(), "p2", 1 << 20).unwrap().is_none());
}

#[test]
fn a_caller_supplied_turn_number_labels_the_checkpoints() {
    // Live bug #5: the pane's third turn was its first with changes, and History said "turn 1".
    let t = Tmp::new("hint");
    t.write("a.txt", "one");
    let lim = Limits::default();
    let before = engine::snapshot_numbered(t.p(), "p1", Kind::Before, None, 1_000, &lim, Some(3)).unwrap();
    let Snapshot::Created(before) = before else { panic!("created") };
    assert_eq!((before.turn, before.label.as_str()), (3, "Before turn 3"));
    t.write("a.txt", "two");
    let after = engine::snapshot_numbered(t.p(), "p1", Kind::After, None, 2_000, &lim, Some(3)).unwrap();
    let Snapshot::Created(after) = after else { panic!("created") };
    assert_eq!(after.label, "After turn 3");
    // Without a hint the old numbering still applies.
    t.write("a.txt", "three");
    let next = engine::snapshot(t.p(), "p1", Kind::Before, None, 3_000, &lim).unwrap();
    let Snapshot::Created(next) = next else { panic!("created") };
    assert_eq!(next.turn, 4);
}

// ---- live-hunt bug: restore refused on an idle pane ---------------------------------------

/// An `Env` backed by a REAL `PaneRegistry` and the same `pane_facts` the live app uses, so the
/// status the runtime sees is exactly what `checkpoint_pane_facts` reports for a registered pane.
struct RegistryEnv {
    registry: Mutex<crate::pty::PaneRegistry>,
}

impl Env for RegistryEnv {
    fn pane(&self, id: &str) -> Option<PaneFacts> {
        self.registry.lock().panes.get(id).map(crate::bridge::pane_facts)
    }
    fn settings(&self) -> Settings {
        Settings { enabled: true, keep: 50 }
    }
    fn now_ms(&self) -> u64 {
        200_000
    }
    fn agent_takes_part(&self, _agent: &str) -> bool {
        true
    }
}

#[test]
fn restore_succeeds_on_an_idle_pane_whose_spawn_snapshot_says_running() {
    use crate::pty::status::PaneStatus;
    let t = Tmp::new("liveidle");
    t.write("a.txt", "v1");
    let first = snap(&t, "dispatched", Kind::Before, 1_000);
    t.write("a.txt", "v2");

    // A pane as the registry holds it after a dispatched run finished: `info.status` was set to
    // "running" at spawn and never changes; the live status lock says idle.
    let mut pane = crate::pty::PaneRegistry::test_pane_stub("dispatched");
    pane.info.cwd = t.p().to_string_lossy().into();
    pane.info.agent_type = "claude".into();
    assert_eq!(pane.info.status, "running", "the spawn snapshot is stale by construction");
    *pane.status.lock() = PaneStatus::Idle;
    let env = Arc::new(RegistryEnv { registry: Mutex::new(crate::pty::PaneRegistry::new()) });
    env.registry.lock().panes.insert("dispatched".into(), pane);

    let rt = Runtime::new(env.clone());
    assert_eq!(env.pane("dispatched").unwrap().status, "idle");
    rt.restore("dispatched", &first.id).expect("an idle pane must be restorable");
    assert_eq!(t.read("a.txt").as_deref(), Some("v1"));

    // The same pane really working is still refused.
    *env.registry.lock().panes.get("dispatched").unwrap().status.lock() = PaneStatus::Running;
    t.write("a.txt", "v3");
    assert!(matches!(rt.restore("dispatched", &first.id), Err(CheckpointError::Running)));
    assert_eq!(t.read("a.txt").as_deref(), Some("v3"));
}

#[test]
fn restore_names_an_open_run_precisely() {
    use super::http::respond;
    let t = Tmp::new("runopen");
    let env = FakeEnv::new(t.p());
    let rt = Runtime::new(env.clone());
    t.write("a.txt", "v1");
    assert!(matches!(rt.capture("p1", Kind::Before, None), Outcome::Created(_)));
    t.write("a.txt", "v2");

    env.set_status("p1", "running");
    let (code, body) = respond(&rt, "POST", &["panes", "p1", "checkpoints", "1", "restore"]).unwrap().unwrap_err();
    assert_eq!((code, body["code"].as_str()), (409, Some("PANE_RUNNING")));

    env.run_open.store(true, Ordering::SeqCst);
    let (code, body) = respond(&rt, "POST", &["panes", "p1", "checkpoints", "1", "restore"]).unwrap().unwrap_err();
    assert_eq!((code, body["code"].as_str()), (409, Some("RUN_OPEN")));
    assert_eq!(body["error"], "A run is still open on this pane. Interrupt it first, then undo.");
    assert_eq!(t.read("a.txt").as_deref(), Some("v2"), "nothing is touched");

    // An idle pane is restorable even if an operation record lingers.
    env.set_status("p1", "idle");
    assert!(respond(&rt, "POST", &["panes", "p1", "checkpoints", "1", "restore"]).unwrap().is_ok());
}

#[test]
fn a_debounced_capture_waits_for_the_snapshot_that_is_being_written() {
    // VERIFY #5: the capture for the end of a turn arrived while the capture for the approval
    // prompt was writing its snapshot, was debounced at once, and "settled" the turn summary before
    // the snapshot existed. A capture now queues behind the one in flight, so when it reports
    // `Debounced` the newest snapshot is already there.
    let t = Tmp::new("inflight");
    for n in 0..300 {
        t.write(&format!("src/file{n}.txt"), &format!("content {n}"));
    }
    let env = FakeEnv::new(t.p());
    let rt = Arc::new(Runtime::new(env.clone()));
    assert!(matches!(rt.capture("p1", Kind::Before, None), Outcome::Created(_)));
    env.advance(DEBOUNCE_MS + 1);
    t.write("src/file0.txt", "edited after the approval");

    let first = {
        let rt = rt.clone();
        std::thread::spawn(move || rt.capture("p1", Kind::After, None))
    };
    std::thread::sleep(std::time::Duration::from_millis(30));
    let second = {
        let (rt, dir) = (rt.clone(), t.p().to_path_buf());
        std::thread::spawn(move || {
            let outcome = rt.capture("p1", Kind::After, None);
            (outcome, engine::list(&dir, "p1").unwrap().len())
        })
    };
    let first = first.join().unwrap();
    let (second, listed_when_it_returned) = second.join().unwrap();
    let debounced = |o: &Outcome| matches!(o, Outcome::Debounced { .. });
    assert!(
        debounced(&first) != debounced(&second),
        "exactly one of the two captures wrote the snapshot: {first:?} / {second:?}"
    );
    assert_eq!(
        listed_when_it_returned, 2,
        "the second capture returned ({second:?}) before the snapshot of the first one existed"
    );
}

#[test]
fn a_pane_that_goes_from_boot_into_its_first_task_still_gets_an_after_snapshot() {
    // NEW #3: a pane the run spawned is "running" from its start, reads no `idle` before the task
    // and so never reports `running` for it. Its first status event is the `idle` that ends the
    // turn, which used to be no turn end at all (no "after" snapshot, no summary).
    let t = Tmp::new("firstturn");
    let env = FakeEnv::new(t.p());
    let rt = Runtime::new(env.clone());
    assert_eq!(rt.decide("p1", "idle"), None, "events alone cannot tell: the pane's first event is idle");
    let rt = Runtime::new(env);
    rt.note_dispatched("p1");
    assert_eq!(rt.decide("p1", "idle"), Some(Kind::After), "the prompt counts as the start of the turn");
    assert_eq!(rt.decide("p1", "idle"), None, "and only once");
}

#[test]
fn an_isolated_runs_pane_is_checkpointed_in_its_worktree() {
    // NEW #3: Receipt said "Nothing yet" and History "No checkpoints yet" for a run in a git
    // worktree. The worktree is the pane's folder and the checkpoint root; the main folder's
    // checkpoints are not touched.
    let t = Tmp::new("worktree");
    user_git(t.p(), &["init", "-q", "-b", "master"]);
    user_git(t.p(), &["config", "user.email", "a@b.c"]);
    user_git(t.p(), &["config", "user.name", "x"]);
    t.write("a.txt", "one");
    user_git(t.p(), &["add", "-A"]);
    user_git(t.p(), &["commit", "-q", "-m", "init"]);
    let wt = t.p().join(".puppet-master").join("worktrees").join("w1");
    std::fs::create_dir_all(wt.parent().unwrap()).unwrap();
    user_git(t.p(), &["worktree", "add", "-q", "-b", "pm/w1", wt.to_str().unwrap()]);

    let env = FakeEnv::new(t.p());
    env.add("pw", &wt, "claude", "running");
    let rt = Runtime::new(env.clone());
    // What the dispatch does for the run's first task on its fresh pane.
    assert!(matches!(rt.before_dispatch("pw", "create wt-note.txt"), Outcome::Created(_)));
    rt.note_dispatched("pw");
    std::fs::write(wt.join("wt-note.txt"), "hi").unwrap();
    assert_eq!(rt.decide("pw", "idle"), Some(Kind::After));
    env.advance(DEBOUNCE_MS + 1);
    assert!(matches!(rt.capture("pw", Kind::After, None), Outcome::Created(_)));

    let items = rt.list("pw").unwrap();
    assert_eq!(items.iter().map(|c| c.kind.as_str()).collect::<Vec<_>>(), ["before", "after"]);
    let changed = rt.turn_changes("pw", items[0].at_ms).unwrap().expect("a baseline");
    assert_eq!(changed.iter().map(|c| c.path.as_str()).collect::<Vec<_>>(), ["wt-note.txt"]);
    assert!(wt.join(".puppet-master").join("checkpoints.git").is_dir(), "rooted at the worktree");
    assert!(
        engine::list(t.p(), "pw").unwrap().is_empty(),
        "the main folder keeps no checkpoints of the isolated pane"
    );
}

// ---- hygiene: gc off the hot path, git off the PTY reader thread, too big, panic ------------------

/// The pane env of `FakeEnv`, with a low size cap and a record of which threads asked it things
/// (`turn_hint` is called on the thread that is about to run git).
struct SpyEnv {
    inner: Arc<FakeEnv>,
    cap: AtomicU64,
    threads: Mutex<Vec<String>>,
}

impl SpyEnv {
    fn new(dir: &Path) -> Arc<Self> {
        Arc::new(Self {
            inner: FakeEnv::new(dir),
            cap: AtomicU64::new(Limits::default().max_total_bytes),
            threads: Mutex::new(vec![]),
        })
    }
}

impl Env for SpyEnv {
    fn pane(&self, id: &str) -> Option<PaneFacts> {
        self.inner.pane(id)
    }
    fn settings(&self) -> Settings {
        self.inner.settings()
    }
    fn now_ms(&self) -> u64 {
        self.inner.now_ms()
    }
    fn agent_takes_part(&self, agent: &str) -> bool {
        self.inner.agent_takes_part(agent)
    }
    fn turn_hint(&self, _pane_id: &str, _kind: Kind) -> Option<u32> {
        self.threads.lock().push(std::thread::current().name().unwrap_or("").to_string());
        None
    }
    fn max_total_bytes(&self) -> u64 {
        self.cap.load(Ordering::SeqCst)
    }
}

#[test]
fn gc_runs_on_its_own_thread_never_inside_the_snapshot() {
    let t = Tmp::new("gc");
    let git_dir = t.p().join(".puppet-master").join("checkpoints.git");
    let dir = t.p().to_path_buf();
    let caller = std::thread::Builder::new()
        .name("snapshot-caller".into())
        .spawn(move || {
            for i in 1..=10_u64 {
                std::fs::write(dir.join("a.txt"), format!("version {i}")).unwrap();
                let c = created(engine::snapshot(&dir, "p1", Kind::After, None, i * 1_000, &limits()).unwrap());
                assert_eq!(c.id, i.to_string());
            }
        })
        .unwrap();
    caller.join().unwrap();
    // The tenth snapshot has returned. Housekeeping is asked for, on a thread of its own.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let ran = loop {
        let hit = engine::gc_test_log::RAN.lock().iter().find(|(dir, _)| *dir == git_dir).cloned();
        if let Some(hit) = hit {
            break hit;
        }
        assert!(std::time::Instant::now() < deadline, "gc never ran");
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    assert_eq!(ran.1.as_deref(), Some(engine::GC_THREAD), "gc must not run on the snapshot's thread");
    // Only the tenth snapshot asked for it.
    assert_eq!(engine::gc_test_log::RAN.lock().iter().filter(|(dir, _)| *dir == git_dir).count(), 1);
}

#[test]
fn git_refuses_to_run_on_a_pty_reader_thread() {
    let t = Tmp::new("readerguard");
    t.write("a.txt", "1");
    let dir = t.p().to_path_buf();
    let outcome = std::thread::Builder::new()
        .name("pty-reader-test-pane".into())
        .spawn(move || engine::snapshot(&dir, "p1", Kind::Before, None, 1_000, &limits()).map(|_| ()))
        .unwrap()
        .join();
    assert!(outcome.is_err(), "the guard must trip (panic) when git is asked for on a pty-reader thread");
    assert!(engine::list(t.p(), "p1").unwrap().is_empty(), "and nothing was snapshotted");
}

#[test]
fn the_status_hook_called_from_the_pty_reader_does_its_git_work_on_the_checkpoint_thread() {
    use super::runtime::{on_status_with, CHECKPOINT_THREAD};
    let t = Tmp::new("readerhook");
    t.write("a.txt", "1");
    let env = SpyEnv::new(t.p());
    let rt: &'static Runtime = Box::leak(Box::new(Runtime::new(env.clone())));
    let reader = std::thread::Builder::new()
        .name("pty-reader-test-pane".into())
        .spawn(move || {
            assert!(on_status_with(rt, "p1", "idle").is_none(), "idle is not a turn boundary");
            // The pane starts working: the hook returns at once with the worker's handle.
            on_status_with(rt, "p1", "running").expect("a snapshot worker")
        })
        .unwrap();
    let worker = reader.join().expect("the hook must not trip the reader-thread guard");
    worker.join().expect("worker finished");
    assert_eq!(*env.threads.lock(), [CHECKPOINT_THREAD], "git work ran on the checkpoint thread only");
    let items = engine::list(t.p(), "p1").unwrap();
    assert_eq!(items.len(), 1, "and the snapshot exists");
}

#[test]
fn a_folder_that_is_too_big_is_reported_once_as_a_card_not_on_every_turn() {
    let t = Tmp::new("toobig");
    t.write("big.bin", &"x".repeat(200));
    let env = SpyEnv::new(t.p());
    env.cap.store(100, Ordering::SeqCst);
    let rt = Runtime::new(env.clone());

    // First attempt measures the folder and refuses.
    assert!(matches!(rt.capture("p1", Kind::Before, None), Outcome::TooBig(bytes) if bytes >= 200));
    let view = super::list_view_with(&rt, "p1").unwrap();
    assert_eq!(view.state, "too_big");
    assert_eq!(view.message, "Undo is off for this folder: it is too big. Add big folders to .gitignore.");
    assert!(rt.folder_too_big("p1").is_some());

    // Later turns do not walk the folder again: even after the data is gone, the answer is the
    // remembered one until the re-check interval has passed.
    std::fs::remove_file(t.p().join("big.bin")).unwrap();
    let before_calls = env.threads.lock().len();
    env.inner.advance(DEBOUNCE_MS + 1);
    assert!(matches!(rt.capture("p1", Kind::After, None), Outcome::TooBig(_)));
    assert_eq!(env.threads.lock().len(), before_calls, "no snapshot attempt (no git) while the answer is remembered");

    // The log line is only wanted when a folder first becomes too big.
    let project = t.p().to_path_buf();
    assert!(!rt.note_too_big(&project, 300, 1), "already known: no new log line");

    // After the interval the folder is measured again, and Undo comes back.
    env.inner.advance(super::runtime::TOO_BIG_RECHECK_MS + 1);
    assert!(matches!(rt.capture("p1", Kind::After, None), Outcome::Created(_)));
    assert!(rt.folder_too_big("p1").is_none());
    assert_eq!(super::list_view_with(&rt, "p1").unwrap().state, "ok");
}

#[test]
fn the_first_too_big_note_for_a_folder_is_the_only_one_that_asks_for_a_log_line() {
    let t = Tmp::new("toobiglog");
    let rt = Runtime::new(FakeEnv::new(t.p()));
    assert!(rt.note_too_big(t.p(), 1, 0), "first time");
    for i in 1..20 {
        assert!(!rt.note_too_big(t.p(), 1 + i, i * 10_000), "repeat {i}");
    }
}

#[test]
fn the_restore_error_for_a_too_big_folder_is_the_same_plain_sentence() {
    let message = CheckpointError::TooLarge(3_808 * 1024 * 1024).to_string();
    assert_eq!(message, "Undo is off for this folder: it is too big. Add big folders to .gitignore.");
}

#[test]
fn before_stop_takes_a_checkpoint_even_inside_the_debounce_window() {
    let t = Tmp::new("stop");
    let env = FakeEnv::new(t.p());
    let rt = Runtime::new(env.clone());
    t.write("a.txt", "start");
    assert!(matches!(rt.capture("p1", Kind::Before, Some("big task")), Outcome::Created(_)));
    // The agent has been writing for a second when the user hits "Stop all agents".
    t.write("a.txt", "half done by the agent");
    t.write("new.txt", "created mid-turn");
    env.advance(1_000);
    assert!(matches!(rt.capture("p1", Kind::After, None), Outcome::Debounced { .. }), "an ordinary snapshot would be skipped here");

    let id = rt.before_stop("p1").expect("a checkpoint id");
    let items = rt.list("p1").unwrap();
    let stop = items.iter().find(|c| c.id == id).expect("listed");
    assert_eq!(stop.kind, "stop");
    assert_eq!(stop.label, "Before stopping (turn 1)");
    let changed = rt.diff("p1", &id).unwrap();
    assert_eq!(changed.iter().map(|c| c.path.as_str()).collect::<Vec<_>>(), ["a.txt", "new.txt"], "it holds the work in progress");

    // Restoring the first checkpoint and then the stop checkpoint brings the half-done work back.
    env.set_status("p1", "idle");
    rt.restore("p1", &items[0].id).unwrap();
    assert!(!t.exists("new.txt"));
    rt.restore("p1", &id).unwrap();
    assert_eq!(t.read("a.txt").as_deref(), Some("half done by the agent"));
    assert_eq!(t.read("new.txt").as_deref(), Some("created mid-turn"));
}

#[test]
fn before_stop_returns_the_newest_checkpoint_when_nothing_changed_and_none_when_off() {
    let t = Tmp::new("stopsame");
    let env = FakeEnv::new(t.p());
    let rt = Runtime::new(env.clone());
    t.write("a.txt", "same");
    let Outcome::Created(first) = rt.capture("p1", Kind::Before, None) else { panic!("created") };
    assert_eq!(rt.before_stop("p1").as_deref(), Some(first.id.as_str()), "unchanged tree: the existing checkpoint is the way back");
    assert_eq!(rt.list("p1").unwrap().len(), 1, "no duplicate");
    env.enabled.store(false, Ordering::SeqCst);
    assert_eq!(rt.before_stop("p1"), None, "checkpoints off: the stop goes ahead without one");
}
