//! Integration tests: every test builds a real temp git repository and drives the real `git` CLI.

use super::*;
use std::process::Command;

struct Repo {
    root: PathBuf,
}

impl Repo {
    fn path(&self) -> String {
        self.root.to_string_lossy().into_owned()
    }
    fn git(&self, args: &[&str]) -> String {
        git_in(&self.root, args)
    }
    fn write(&self, rel: &str, text: &str) {
        let path = self.root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.root.join(rel)).unwrap_or_default()
    }
    fn commit_all(&self, message: &str) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", message]);
    }
    fn head(&self) -> String {
        self.git(&["rev-parse", "HEAD"])
    }
    fn branches(&self) -> String {
        self.git(&["branch", "--list", "--format=%(refname:short)"])
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        // read-only git objects block remove_dir_all on Windows
        clear_readonly(&self.root);
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn git_in(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pmwt-{tag}-{}", &uuid::Uuid::new_v4().to_string()[..8]));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A repo on `main` with `a.txt`, `b.txt` and `src/lib.rs` committed.
fn repo() -> Repo {
    let root = temp_dir("repo");
    let repo = Repo { root };
    repo.git(&["init", "-q", "-b", "main"]);
    repo.git(&["config", "user.name", "Test"]);
    repo.git(&["config", "user.email", "test@example.com"]);
    repo.git(&["config", "commit.gpgsign", "false"]);
    repo.git(&["config", "core.autocrlf", "false"]);
    repo.write("a.txt", "alpha\n");
    repo.write("b.txt", "bravo\n");
    repo.write("src/lib.rs", "pub fn one() {}\n");
    repo.commit_all("initial");
    repo
}

#[test]
fn create_makes_a_worktree_on_its_own_branch_and_hides_it_from_git() {
    let repo = repo();
    let wt = create(&repo.path(), "run-1").unwrap();
    assert!(Path::new(&wt.path).join("a.txt").is_file(), "checkout has the files");
    assert_eq!(wt.branch, "pm/run-1");
    assert_eq!(wt.base, "main");
    assert_eq!(wt.base_commit, repo.head());
    assert_eq!(wt.cwd, wt.path, "project at the repo root: cwd is the worktree root");
    assert!(repo.branches().lines().any(|b| b == "pm/run-1"));
    assert!(Path::new(&wt.path).starts_with(repo.root.join(".puppet-master").join("worktrees")));
    // short names keep Windows paths well under MAX_PATH
    assert!(dir_name_of(&wt).len() <= 20, "{}", dir_name_of(&wt));
    // ignored through .git/info/exclude, the user's .gitignore is untouched
    let exclude = std::fs::read_to_string(repo.root.join(".git/info/exclude")).unwrap();
    assert!(exclude.lines().any(|l| l == ".puppet-master/worktrees/"), "{exclude}");
    assert!(!repo.root.join(".gitignore").exists());
    assert_eq!(repo.git(&["status", "--porcelain", "--untracked-files=all"]), "", "origin stays clean");
    // retry for the same handle returns the same worktree
    let again = create(&repo.path(), "run-1").unwrap();
    assert_eq!(again, wt);
    assert_eq!(list(&repo.path()).len(), 1);
}

#[test]
fn changes_in_the_worktree_do_not_touch_the_origin_and_the_diff_shows_them() {
    let repo = repo();
    let head = repo.head();
    let wt = create(&repo.path(), "run-2").unwrap();
    let work = PathBuf::from(&wt.path);
    std::fs::write(work.join("a.txt"), "alpha\nagent line\n").unwrap(); // modified, uncommitted
    std::fs::write(work.join("new.txt"), "fresh\nfile\n").unwrap(); // untracked
    std::fs::remove_file(work.join("b.txt")).unwrap(); // deleted
    git_in(&work, &["add", "-A"]);
    git_in(&work, &["commit", "-q", "-m", "committed part"]);
    std::fs::write(work.join("src/lib.rs"), "pub fn one() {}\npub fn two() {}\n").unwrap(); // later edit

    // origin untouched
    assert_eq!(repo.read("a.txt"), "alpha\n");
    assert!(!repo.root.join("new.txt").exists());
    assert!(repo.root.join("b.txt").exists());
    assert_eq!(repo.head(), head);
    assert_eq!(repo.git(&["status", "--porcelain", "--untracked-files=all"]), "");

    let view = diff(&wt, None).unwrap();
    let by_path: std::collections::HashMap<_, _> =
        view.changes.files.iter().map(|f| (f.path.as_str(), f)).collect();
    assert_eq!(view.changes.files_changed, 4, "{:?}", view.changes);
    assert_eq!(by_path["a.txt"].status, "modified");
    assert_eq!(by_path["a.txt"].additions, 1);
    assert_eq!(by_path["new.txt"].status, "added");
    assert_eq!(by_path["new.txt"].additions, 2);
    assert_eq!(by_path["b.txt"].status, "deleted");
    assert_eq!(by_path["b.txt"].deletions, 1);
    assert_eq!(by_path["src/lib.rs"].additions, 1);
    assert_eq!(view.changes.additions, 1 + 2 + 1);
    assert_eq!(view.changes.deletions, 1);
    assert!(view.patch.contains("+agent line"), "{}", view.patch);
    assert!(view.patch.contains("+++ b/new.txt"));
    assert!(view.patch.contains("-bravo"));
    assert!(!view.patch_truncated);
    // a single-file patch
    let one = diff(&wt, Some("a.txt")).unwrap();
    assert!(one.patch.contains("a.txt") && !one.patch.contains("new.txt"));
    // the agent's own index was not disturbed (lib.rs edit still unstaged)
    assert_eq!(git_in(&work, &["diff", "--cached", "--name-only"]), "", "nothing was staged in the agent's index");
    assert_eq!(git_in(&work, &["diff", "--name-only"]), "src/lib.rs");
}

#[test]
fn a_large_diff_is_capped() {
    let repo = repo();
    let wt = create(&repo.path(), "big").unwrap();
    let big: String = (0..40_000).map(|i| format!("line number {i}\n")).collect();
    std::fs::write(Path::new(&wt.path).join("big.txt"), &big).unwrap();
    let view = diff(&wt, None).unwrap();
    assert!(view.patch_truncated);
    assert!(view.patch.len() <= MAX_PATCH_BYTES);
    assert_eq!(view.changes.additions, 40_000, "totals are not capped");
}

#[test]
fn merge_brings_the_changes_in_with_a_merge_commit_and_closes_the_worktree() {
    let repo = repo();
    let wt = create(&repo.path(), "run-3").unwrap();
    let work = PathBuf::from(&wt.path);
    std::fs::write(work.join("a.txt"), "alpha\nagent line\n").unwrap();
    std::fs::write(work.join("new.txt"), "fresh\n").unwrap();
    // meanwhile the origin moves on in an unrelated file
    repo.write("b.txt", "bravo\norigin line\n");
    repo.commit_all("origin work");

    let outcome = merge(&wt).unwrap();
    assert!(outcome.merged);
    assert_eq!(outcome.into, "main");
    assert_eq!(repo.read("a.txt"), "alpha\nagent line\n");
    assert_eq!(repo.read("new.txt"), "fresh\n");
    assert_eq!(repo.read("b.txt"), "bravo\norigin line\n");
    let parents = repo.git(&["rev-list", "--parents", "-n", "1", "HEAD"]);
    assert_eq!(parents.split_whitespace().count(), 3, "a --no-ff merge commit has two parents: {parents}");
    assert_eq!(outcome.merge_commit.as_deref(), Some(repo.head().as_str()));
    assert!(!work.exists(), "worktree removed after merging");
    assert!(!repo.branches().lines().any(|b| b == "pm/run-3"));
    assert!(list(&repo.path()).is_empty());
    assert_eq!(repo.git(&["status", "--porcelain"]), "");
}

#[test]
fn merging_nothing_reports_no_changes_and_keeps_the_worktree() {
    let repo = repo();
    let wt = create(&repo.path(), "idle").unwrap();
    let outcome = merge(&wt).unwrap();
    assert!(!outcome.merged);
    assert_eq!(outcome.reason.as_deref(), Some("no_changes"));
    assert!(Path::new(&wt.path).is_dir());
    assert!(discard_if_pristine(&wt));
    assert!(!Path::new(&wt.path).exists());
}

#[test]
fn a_conflict_aborts_cleanly_and_leaves_the_origin_unchanged() {
    let repo = repo();
    let wt = create(&repo.path(), "run-4").unwrap();
    let work = PathBuf::from(&wt.path);
    std::fs::write(work.join("a.txt"), "alpha from the agent\n").unwrap();
    std::fs::write(work.join("clean.txt"), "no conflict here\n").unwrap();
    repo.write("a.txt", "alpha from the origin\n");
    repo.commit_all("origin edits the same line");
    let head = repo.head();

    let error = merge(&wt).unwrap_err();
    assert_eq!(error.code, "MERGE_CONFLICT", "{error:?}");
    assert_eq!(error.context["files"], serde_json::json!(["a.txt"]));
    assert_eq!(repo.head(), head, "origin HEAD did not move");
    assert_eq!(repo.read("a.txt"), "alpha from the origin\n", "no conflict markers");
    assert!(!repo.root.join("clean.txt").exists());
    assert_eq!(repo.git(&["status", "--porcelain"]), "");
    let in_progress = Command::new("git")
        .args(["rev-parse", "-q", "--verify", "MERGE_HEAD"])
        .current_dir(&repo.root)
        .output()
        .unwrap();
    assert!(!in_progress.status.success(), "origin is not mid-merge");
    // nothing is lost: the work is committed on the branch and the worktree is still there
    assert!(Path::new(&wt.path).is_dir());
    assert!(repo.branches().lines().any(|b| b == "pm/run-4"));
    assert_eq!(git_in(&repo.root, &["show", "pm/run-4:a.txt"]), "alpha from the agent");
}

#[test]
fn merge_refuses_when_the_origin_has_uncommitted_changes_to_the_same_files() {
    let repo = repo();
    let wt = create(&repo.path(), "run-5").unwrap();
    std::fs::write(Path::new(&wt.path).join("a.txt"), "alpha agent\n").unwrap();
    repo.write("a.txt", "alpha local edit\n"); // uncommitted
    let head = repo.head();
    let error = merge(&wt).unwrap_err();
    assert_eq!(error.code, "ORIGIN_DIRTY_OVERLAP");
    assert_eq!(error.context["files"], serde_json::json!(["a.txt"]));
    assert_eq!(repo.read("a.txt"), "alpha local edit\n", "the user's edit is untouched");
    assert_eq!(repo.head(), head);
    // an unrelated dirty file does not block the merge
    repo.git(&["checkout", "--", "a.txt"]);
    repo.write("b.txt", "bravo dirty\n");
    let outcome = merge(&wt).unwrap();
    assert!(outcome.merged);
    assert_eq!(repo.read("b.txt"), "bravo dirty\n", "unrelated local edit survives the merge");
    assert_eq!(repo.read("a.txt"), "alpha agent\n");
}

#[test]
fn merge_refuses_a_detached_origin() {
    let repo = repo();
    let wt = create(&repo.path(), "run-6").unwrap();
    std::fs::write(Path::new(&wt.path).join("new.txt"), "x\n").unwrap();
    repo.git(&["checkout", "-q", "--detach"]);
    assert_eq!(merge(&wt).unwrap_err().code, "ORIGIN_DETACHED");
}

#[test]
fn discard_removes_the_worktree_and_the_branch_even_with_unmerged_work() {
    let repo = repo();
    let wt = create(&repo.path(), "run-7").unwrap();
    let work = PathBuf::from(&wt.path);
    std::fs::write(work.join("new.txt"), "x\n").unwrap();
    git_in(&work, &["add", "-A"]);
    git_in(&work, &["commit", "-q", "-m", "unmerged"]);
    std::fs::write(work.join("more.txt"), "y\n").unwrap();

    discard(&wt).unwrap();
    assert!(!work.exists());
    assert!(!repo.branches().lines().any(|b| b == "pm/run-7"));
    assert!(list(&repo.path()).is_empty());
    assert_eq!(repo.git(&["worktree", "list", "--porcelain"]).matches("worktree ").count(), 1);
    assert!(!repo.root.join("new.txt").exists(), "discarded work never reaches the origin");
}

#[test]
fn discard_refuses_a_path_outside_the_worktrees_folder() {
    let repo = repo();
    let mut wt = create(&repo.path(), "run-8").unwrap();
    let outside = temp_dir("outside");
    std::fs::write(outside.join("keep.txt"), "precious").unwrap();
    wt.path = outside.to_string_lossy().into_owned();
    assert_eq!(discard(&wt).unwrap_err().code, "WORKTREE_PATH_REFUSED");
    assert!(outside.join("keep.txt").is_file());
    let _ = std::fs::remove_dir_all(outside);
}

#[test]
fn a_non_git_project_is_refused_with_a_clear_code() {
    let dir = temp_dir("nogit");
    let error = create(&dir.to_string_lossy(), "x").unwrap_err();
    assert_eq!(error.code, "NOT_A_GIT_REPO", "{error:?}");
    assert!(error.message.contains("git init"), "{}", error.message);
    assert!(!dir.join(".puppet-master").exists(), "nothing was created");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_repo_without_commits_is_refused() {
    let dir = temp_dir("nocommit");
    git_in(&dir, &["init", "-q", "-b", "main"]);
    assert_eq!(create(&dir.to_string_lossy(), "x").unwrap_err().code, "NO_COMMITS");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_project_inside_a_repo_gets_its_offset_as_the_working_directory() {
    let repo = repo();
    let sub = repo.root.join("src");
    let wt = create(&sub.to_string_lossy(), "sub").unwrap();
    assert!(Path::new(&wt.cwd).ends_with("src"), "{}", wt.cwd);
    assert!(Path::new(&wt.cwd).join("lib.rs").is_file());
    assert!(Path::new(&wt.path).starts_with(sub.join(".puppet-master")));
    assert!(wt.active_cwd().is_some());
}

#[test]
fn the_startup_sweep_removes_leftovers_but_never_unmerged_work() {
    let repo = repo();
    // 1. a real worktree with unmerged work: must survive
    let keep = create(&repo.path(), "keeper").unwrap();
    std::fs::write(Path::new(&keep.path).join("work.txt"), "precious\n").unwrap();
    // 2. a checkout deleted behind git's back, branch empty: description and branch go
    let gone = create(&repo.path(), "gone").unwrap();
    clear_readonly(Path::new(&gone.path));
    std::fs::remove_dir_all(&gone.path).unwrap();
    // 3. a half-created folder nobody knows
    let stray = repo.root.join(".puppet-master/worktrees/stray-dir");
    std::fs::create_dir_all(&stray).unwrap();
    std::fs::write(stray.join("junk"), "x").unwrap();

    let report = sweep(&repo.path());
    assert_eq!(report.removed_meta, vec!["gone".to_string()], "{report:?}");
    assert_eq!(report.removed_dirs.len(), 1, "{report:?}");
    assert!(report.pruned >= 1, "{report:?}");
    assert!(!stray.exists());
    assert!(Path::new(&keep.path).join("work.txt").is_file(), "unmerged work survives the sweep");
    assert!(repo.branches().lines().any(|b| b == "pm/keeper"));
    assert!(!repo.branches().lines().any(|b| b == "pm/gone"));
    let left = list(&repo.path());
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].worktree.handle, "keeper");
    assert!(left[0].exists);
    // idempotent
    assert_eq!(sweep(&repo.path()), SweepReport::default());
}

#[test]
fn a_vanished_checkout_whose_branch_has_commits_keeps_the_branch() {
    let repo = repo();
    let wt = create(&repo.path(), "committed").unwrap();
    let work = PathBuf::from(&wt.path);
    std::fs::write(work.join("n.txt"), "n\n").unwrap();
    git_in(&work, &["add", "-A"]);
    git_in(&work, &["commit", "-q", "-m", "work"]);
    clear_readonly(&work);
    std::fs::remove_dir_all(&work).unwrap();
    sweep(&repo.path());
    assert!(repo.branches().lines().any(|b| b == "pm/committed"), "committed work stays reachable");
}

#[test]
fn slugs_are_short_and_safe() {
    assert_eq!(slug("My Run #1!", 40), "my-run-1");
    assert_eq!(slug("", 10), "run");
    assert_eq!(slug("3f2a9c1e-77aa-4b0e-9d11-0123456789ab", 10), "3f2a9c1e-7");
    assert!(slug("../../etc", 40).chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
}

// ---- the `/worktrees/*` route bodies, driven like the bridge does (request -> response) ----

fn route(
    method: &str,
    path: &str,
    project: &str,
    body: serde_json::Value,
) -> Result<serde_json::Value, OperationError> {
    let registry = std::sync::Arc::new(parking_lot::Mutex::new(crate::pty::PaneRegistry::new()));
    let segments: Vec<&str> = path.trim_matches('/').split('/').collect();
    super::routes::handle(method, &segments, Some(project), None, &body, &registry)
        .expect("route is owned by worktrees")
}

#[test]
fn route_transcript_list_diff_conflict_merge_discard() {
    let repo = repo();
    let project = repo.path();
    let a = create(&project, "route-a").unwrap();
    let b = create(&project, "route-b").unwrap();
    std::fs::write(Path::new(&a.path).join("a.txt"), "alpha from a\n").unwrap();
    std::fs::write(Path::new(&b.path).join("a.txt"), "alpha from b\n").unwrap();
    std::fs::write(Path::new(&b.path).join("only-b.txt"), "b\n").unwrap();

    // GET /worktrees
    let listed = route("GET", "/worktrees", &project, serde_json::json!({})).unwrap();
    let items = listed["worktrees"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert!(items.iter().all(|i| i["state"] == "unreviewed" && i["exists"] == true), "{items:?}");
    let item_a = items.iter().find(|i| i["handle"] == "route-a").unwrap();
    assert_eq!(item_a["branch"], "pm/route-a");
    assert_eq!(item_a["summary"]["files_changed"], 1);

    // GET /worktrees/route-b/diff
    let diff_b = route("GET", "/worktrees/route-b/diff", &project, serde_json::json!({})).unwrap();
    assert_eq!(diff_b["changes"]["files_changed"], 2);
    assert!(diff_b["patch"].as_str().unwrap().contains("+alpha from b"));

    // POST /worktrees/route-a/merge -> merged
    let merged = route("POST", "/worktrees/route-a/merge", &project, serde_json::json!({})).unwrap();
    assert_eq!(merged["merged"], true);
    assert_eq!(merged["into"], "main");
    assert_eq!(repo.read("a.txt"), "alpha from a\n");

    // POST /worktrees/route-b/merge -> MERGE_CONFLICT on a.txt, origin unchanged
    let head = repo.head();
    let error = route("POST", "/worktrees/route-b/merge", &project, serde_json::json!({})).unwrap_err();
    assert_eq!(error.code, "MERGE_CONFLICT");
    assert_eq!(error.context["files"], serde_json::json!(["a.txt"]));
    assert_eq!(repo.head(), head);
    assert_eq!(repo.git(&["status", "--porcelain"]), "");

    // review (MCP) route: diff by default, discard on request
    let review = route("POST", "/worktrees/review", &project, serde_json::json!({"handle":"route-b"})).unwrap();
    assert_eq!(review["changes"]["files_changed"], 2);
    let gone = route("POST", "/worktrees/review", &project, serde_json::json!({"handle":"route-b","action":"discard"})).unwrap();
    assert_eq!(gone["discarded"], true);
    let missing = route("POST", "/worktrees/route-b/discard", &project, serde_json::json!({})).unwrap_err();
    assert_eq!(missing.code, "WORKTREE_NOT_FOUND");
    assert!(route("GET", "/worktrees", &project, serde_json::json!({})).unwrap()["worktrees"].as_array().unwrap().is_empty());
    let bad = route("POST", "/worktrees/review", &project, serde_json::json!({"handle":"x","action":"nuke"})).unwrap_err();
    assert_eq!(bad.code, "INVALID_ARGUMENT");
}

#[test]
fn origin_of_cwd_finds_the_project_above_a_worktree() {
    assert_eq!(
        origin_of_cwd(r"C:\work\proj\.puppet-master\worktrees\a-1\src").as_deref(),
        Some(r"C:\work\proj")
    );
    assert_eq!(origin_of_cwd("/home/u/p/.puppet-master/worktrees/x").as_deref(), None.or(Some("/home/u/p")));
    assert_eq!(origin_of_cwd("/home/u/p"), None);
}

fn registry_with_pane(id: &str, cwd: &str) -> std::sync::Arc<parking_lot::Mutex<crate::pty::PaneRegistry>> {
    let registry = std::sync::Arc::new(parking_lot::Mutex::new(crate::pty::PaneRegistry::new()));
    let mut pane = crate::pty::PaneRegistry::test_pane_stub(id);
    pane.info.cwd = cwd.to_string();
    *pane.status.lock() = crate::pty::status::PaneStatus::Idle;
    registry.lock().panes.insert(id.to_string(), pane);
    registry
}

#[test]
fn a_pane_working_in_a_worktree_finds_its_changes() {
    let repo = repo();
    let wt = create(&repo.path(), "run-pane").unwrap();
    std::fs::write(Path::new(&wt.path).join("a.txt"), "alpha\nchanged\n").unwrap();
    let registry = registry_with_pane("pane-wt", &wt.cwd);
    let found = service::for_pane(&registry, "pane-wt").expect("the pane's worktree");
    assert_eq!(found.entry.worktree.handle, "run-pane");
    assert_eq!(found.state, "unreviewed");
    assert_eq!(found.summary.map(|s| s.files_changed), Some(1));
}
