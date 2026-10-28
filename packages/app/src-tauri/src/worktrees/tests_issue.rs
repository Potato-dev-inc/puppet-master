//! Tests for the hardened git of issue runs (spec n32 §3.5, threat S7): a copy's hooks, husky
//! files, fsmonitor, filter drivers and remote URL are planted the way an agent could, and PM's own
//! git calls must not run them. Each attack has a control run without the hardened env, so a pass
//! means the guard works and not that the attack never fired. Real temp git repositories only.

use super::harden::{self, git_env};
use super::pr;
use super::proc::ToolEnv;
use super::*;
use std::process::Command;

// ------------------------------------------------------------------------------------ helpers

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pmharden-{tag}-{}", &uuid::Uuid::new_v4().to_string()[..8]));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn clear_readonly(dir: &Path) {
    if let Ok(read) = std::fs::read_dir(dir) {
        for entry in read.flatten() {
            let path = entry.path();
            if let Ok(meta) = std::fs::symlink_metadata(&path) {
                let mut perms = meta.permissions();
                #[allow(clippy::permissions_set_readonly_false)]
                perms.set_readonly(false);
                let _ = std::fs::set_permissions(&path, perms);
                if meta.is_dir() {
                    clear_readonly(&path);
                }
            }
        }
    }
}

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        clear_readonly(&self.0);
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("git runs");
    assert!(out.status.success(), "git {args:?} failed: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// A repo on `main` with `a.txt` and `src/lib.rs`, a bare `origin` it can push to, and `main`
/// pushed. Line endings and signing are pinned so the tests do not depend on the machine.
fn repo_with_origin() -> (Dir, PathBuf, Dir, PathBuf) {
    let root = temp_dir("repo");
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["config", "user.name", "Test"]);
    git(&root, &["config", "user.email", "test@example.com"]);
    git(&root, &["config", "commit.gpgsign", "false"]);
    git(&root, &["config", "core.autocrlf", "false"]);
    write(&root, "a.txt", "alpha\n");
    write(&root, "src/lib.rs", "pub fn one() {}\n");
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "initial"]);
    let bare = temp_dir("origin");
    git(&bare, &["init", "-q", "--bare", "-b", "main"]);
    git(&root, &["remote", "add", "origin", &bare.to_string_lossy()]);
    git(&root, &["push", "-q", "origin", "main"]);
    (Dir(root.clone()), root, Dir(bare.clone()), bare)
}

fn project_str(root: &Path) -> String {
    root.to_string_lossy().into_owned()
}

/// A marker path with forward slashes, safe inside a `sh` hook or `sh -c` command.
fn marker_in(dir: &Path, name: &str) -> String {
    dir.join(name).to_string_lossy().replace('\\', "/")
}

/// An empty private hooks folder, the way the issue flow makes one.
fn empty_hooks() -> Dir {
    Dir(temp_dir("empty-hooks"))
}

/// A copy of `root` on a new branch, the way an issue run gets one.
fn copy_of(root: &Path, handle: &str) -> WorktreeRef {
    create_with(&project_str(root), handle, &CreateOptions::default()).unwrap()
}

/// A hook file body that leaves a marker and exits 0.
fn marker_hook(marker: &str) -> String {
    format!("#!/bin/sh\necho ran > \"{marker}\"\nexit 0\n")
}

// ------------------------------------------------------------------------------ pure parts

#[test]
fn the_hardened_env_turns_off_hooks_and_fsmonitor_through_git_config_count() {
    let env = git_env(Path::new("C:/pm/empty-hooks"));
    let get = |key: &str| env.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
    assert_eq!(get("GIT_CONFIG_COUNT").as_deref(), Some("2"));
    assert_eq!(get("GIT_CONFIG_KEY_0").as_deref(), Some("core.hooksPath"));
    assert_eq!(get("GIT_CONFIG_VALUE_0").as_deref(), Some("C:/pm/empty-hooks"));
    assert_eq!(get("GIT_CONFIG_KEY_1").as_deref(), Some("core.fsmonitor"));
    assert_eq!(get("GIT_CONFIG_VALUE_1").as_deref(), Some("false"));
    assert_eq!(get("GIT_TERMINAL_PROMPT").as_deref(), Some("0"));
}

#[test]
fn only_the_repos_own_filter_drivers_are_blanked() {
    let listing = "system\tfilter.lfs.clean\nglobal\tfilter.lfs.required\nlocal\tfilter.evil.clean\n\
                   local\tfilter.evil.required\nworktree\tfilter.two.smudge\nlocal\tcore.editor\nnoise\n";
    let overrides = harden::filter_overrides_from(listing);
    assert_eq!(
        overrides,
        vec![
            ("filter.evil.clean".to_string(), String::new()),
            ("filter.evil.required".to_string(), "false".to_string()),
            ("filter.two.smudge".to_string(), String::new()),
        ]
    );
}

#[test]
fn names_that_could_be_options_or_escape_are_refused() {
    assert!(harden::check_name("origin").is_ok());
    assert!(harden::check_name("feature/x-1.2_y").is_ok());
    assert!(harden::check_name("-o").is_err());
    assert!(harden::check_name("a..b").is_err());
    assert!(harden::check_name("a b").is_err());
    assert!(harden::check_name("").is_err());
}

// ------------------------------------------------------------------ start point and env files

#[test]
fn a_copy_starts_from_the_named_start_point_not_from_the_checkout() {
    let (_guard, root, _bare, _bare_path) = repo_with_origin();
    // The checkout moves on to a feature branch with a commit that origin/main does not have.
    git(&root, &["checkout", "-q", "-b", "feature"]);
    write(&root, "feature.txt", "feature\n");
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "feature"]);
    let empty = empty_hooks();
    let env = git_env(&empty.0);
    let main_sha = harden::fetch_branch(&project_str(&root), "origin", "main", &env).unwrap();
    let opts = CreateOptions { start_point: Some("origin/main".into()), copy_files: true, git_env: env };
    let worktree = create_with(&project_str(&root), "start-1", &opts).unwrap();
    let cwd = Path::new(&worktree.cwd);
    assert!(!cwd.join("feature.txt").exists(), "the copy must not carry the checkout's commit");
    assert!(cwd.join("a.txt").exists());
    assert_eq!(worktree.base_commit, main_sha);
    assert_eq!(git(cwd, &["rev-parse", "HEAD"]), main_sha);
    // The default (no start point) still starts from the checkout's HEAD.
    let plain = create(&project_str(&root), "start-2").unwrap();
    assert!(Path::new(&plain.cwd).join("feature.txt").exists());
}

#[test]
fn a_copy_without_its_preset_files_still_runs_the_setup() {
    let (_guard, root, _bare, _bare_path) = repo_with_origin();
    write(&root, ".env", "SECRET=1\n");
    write(&root, ".puppet-master/project.toml", "[worktree]\ncopy = [\".env\"]\nsetup = [[\"git\", \"config\", \"pm.setup-ran\", \"yes\"]]\n");
    let opts = CreateOptions { start_point: None, copy_files: false, git_env: Vec::new() };
    let worktree = create_with(&project_str(&root), "files-1", &opts).unwrap();
    let cwd = Path::new(&worktree.cwd);
    assert!(!cwd.join(".env").exists(), "the preset's copy list is skipped");
    let setup = worktree.setup.as_ref().expect("the setup is recorded");
    assert_eq!(setup.status, "ok", "{setup:?}");
    assert_eq!(git(cwd, &["config", "--get", "pm.setup-ran"]), "yes");
    // Control: the same preset copies the file when copy_files is on.
    let normal = create(&project_str(&root), "files-2").unwrap();
    assert!(Path::new(&normal.cwd).join(".env").is_file());
}

// --------------------------------------------------------------------------- planted hooks

#[test]
fn a_planted_post_commit_hook_does_not_run_under_the_hardened_env_but_does_without_it() {
    let (_guard, root, _bare, _bare_path) = repo_with_origin();
    let worktree = copy_of(&root, "hook-commit-1");
    let marker_dir = Dir(temp_dir("marker"));
    let marker = marker_in(&marker_dir.0, "post-commit.txt");
    // The agent writes into the shared hooks folder (a copy shares it with the origin repo).
    let hooks = PathBuf::from(git(Path::new(&worktree.cwd), &["rev-parse", "--git-path", "hooks"]));
    let hooks = if hooks.is_absolute() { hooks } else { Path::new(&worktree.cwd).join(hooks) };
    write(&hooks, "post-commit", &marker_hook(&marker));
    write(Path::new(&worktree.cwd), "agent.txt", "work\n");
    let empty = empty_hooks();
    assert!(commit_pending_with(&worktree, &git_env(&empty.0)).unwrap());
    assert!(!Path::new(&marker).exists(), "the hardened commit must not run post-commit");
    // Control: the plain commit path runs it.
    write(Path::new(&worktree.cwd), "agent2.txt", "more\n");
    assert!(commit_pending(&worktree).unwrap());
    assert!(Path::new(&marker).exists(), "the control run must show the hook fires without the env");
}

#[test]
fn a_planted_post_checkout_hook_does_not_run_when_the_copy_is_made_hardened() {
    let (_guard, root, _bare, _bare_path) = repo_with_origin();
    let marker_dir = Dir(temp_dir("marker"));
    let marker = marker_in(&marker_dir.0, "post-checkout.txt");
    write(&root.join(".git").join("hooks"), "post-checkout", &marker_hook(&marker));
    let empty = empty_hooks();
    let opts = CreateOptions { start_point: None, copy_files: true, git_env: git_env(&empty.0) };
    create_with(&project_str(&root), "hook-checkout-1", &opts).unwrap();
    assert!(!Path::new(&marker).exists(), "the hardened worktree add must not run post-checkout");
    // Control: a plain copy runs the same hook.
    create(&project_str(&root), "hook-checkout-2").unwrap();
    assert!(Path::new(&marker).exists(), "the control copy must show the hook fires without the env");
}

#[test]
fn a_planted_husky_pre_push_does_not_block_or_run_during_puppet_masters_push() {
    let (_guard, root, _bare, bare_path) = repo_with_origin();
    let worktree = copy_of(&root, "husky-push-1");
    let cwd = PathBuf::from(&worktree.cwd);
    let marker_dir = Dir(temp_dir("marker"));
    let marker = marker_in(&marker_dir.0, "pre-push.txt");
    // Husky-style: core.hooksPath points at .husky, and the agent writes the pre-push hook there.
    write(&cwd, ".husky/pre-push", &marker_hook(&marker));
    git(&cwd, &["config", "core.hooksPath", ".husky"]);
    let empty = empty_hooks();
    let env = git_env(&empty.0);
    // The first push is PM's, hardened: it must succeed and must not run the hook.
    pr::push_branch(&worktree, &ToolEnv::default(), &env).unwrap();
    assert!(!Path::new(&marker).exists(), "the hardened push must not run pre-push");
    assert!(git(&bare_path, &["branch", "--list", &worktree.branch]).contains(&worktree.branch));
    // Control: a new commit pushed plainly runs the hook (and the hook's marker shows it).
    write(&cwd, "more.txt", "more\n");
    git(&cwd, &["add", "-A"]);
    git(&cwd, &["commit", "-q", "--no-verify", "-m", "more"]);
    pr::push_branch(&worktree, &ToolEnv::default(), &[]).unwrap_or_else(|e| panic!("{e:?}"));
    assert!(Path::new(&marker).exists(), "the control push must show the hook fires without the env");
}

// ------------------------------------------------------------------------- fsmonitor, filters

#[test]
fn a_planted_fsmonitor_command_does_not_run_during_the_hardened_commit() {
    let (_guard, root, _bare, _bare_path) = repo_with_origin();
    let worktree = copy_of(&root, "fsmon-1");
    let cwd = Path::new(&worktree.cwd);
    let marker_dir = Dir(temp_dir("marker"));
    let marker = marker_in(&marker_dir.0, "fsmonitor.txt");
    git(cwd, &["config", "core.fsmonitor", &format!("sh -c 'echo fsm > \"{marker}\"'")]);
    write(cwd, "agent.txt", "work\n");
    let empty = empty_hooks();
    assert!(commit_pending_with(&worktree, &git_env(&empty.0)).unwrap());
    assert!(!Path::new(&marker).exists(), "the hardened commit must not run core.fsmonitor");
    // Control: the plain path runs it on the same index refresh.
    write(cwd, "agent2.txt", "more\n");
    assert!(commit_pending(&worktree).unwrap());
    assert!(Path::new(&marker).exists(), "the control must show core.fsmonitor fires without the env");
}

#[test]
fn a_planted_clean_filter_does_not_run_during_the_hardened_commit_or_changes() {
    let (_guard, root, _bare, _bare_path) = repo_with_origin();
    let worktree = copy_of(&root, "filter-1");
    let cwd = PathBuf::from(&worktree.cwd);
    let marker_dir = Dir(temp_dir("marker"));
    let marker = marker_in(&marker_dir.0, "clean.txt");
    // The agent plants a clean filter in the repo's local config and maps *.txt to it.
    git(&cwd, &["config", "filter.evil.clean", &format!("sh -c 'echo filt > \"{marker}\"; cat'")]);
    git(&cwd, &["config", "filter.evil.required", "true"]);
    write(&cwd, ".gitattributes", "*.txt filter=evil\n");
    write(&cwd, "agent.txt", "work\n");
    let empty = empty_hooks();
    let env = git_env(&empty.0);
    assert!(commit_pending_with(&worktree, &env).unwrap(), "the hardened commit still succeeds");
    assert!(!Path::new(&marker).exists(), "the hardened commit must not run the clean filter");
    let listed = changes_with(&worktree, &env).unwrap();
    assert!(listed.files.iter().any(|f| f.path == "agent.txt"), "{listed:?}");
    assert!(!Path::new(&marker).exists(), "the hardened changes check must not run the clean filter");
    // Control: a plain commit runs the filter.
    write(&cwd, "agent2.txt", "more\n");
    assert!(commit_pending(&worktree).unwrap());
    assert!(Path::new(&marker).exists(), "the control must show the clean filter fires without the env");
}

#[test]
fn an_empty_env_leaves_every_call_as_it_was() {
    let (_guard, root, _bare, _bare_path) = repo_with_origin();
    let worktree = copy_of(&root, "plain-1");
    let cwd = Path::new(&worktree.cwd);
    write(cwd, "one.txt", "1\n");
    assert!(commit_pending_with(&worktree, &[]).unwrap());
    assert!(!commit_pending_with(&worktree, &[]).unwrap(), "nothing new: no commit");
    assert!(harden::with_filters_off(cwd, &[]).unwrap().is_empty());
}

// ---------------------------------------------------------------- config and remote snapshot

#[test]
fn the_snapshot_is_stable_and_changes_for_a_remote_url_a_hook_or_husky_or_fsmonitor() {
    let (_guard, root, _bare, bare_path) = repo_with_origin();
    let worktree = copy_of(&root, "snap-1");
    let cwd = Path::new(&worktree.cwd);
    let base = harden::config_snapshot(cwd).unwrap();
    assert_eq!(base, harden::config_snapshot(cwd).unwrap(), "two reads of an untouched copy match");

    // A swapped remote URL.
    git(cwd, &["remote", "set-url", "origin", "https://example.invalid/attacker/repo.git"]);
    let url = harden::config_snapshot(cwd).unwrap();
    assert_ne!(url.0, base.0, "a changed remote URL changes the config text");
    assert_eq!(url.1, base.1);
    git(cwd, &["remote", "set-url", "origin", &bare_path.to_string_lossy()]);
    assert_eq!(harden::config_snapshot(cwd).unwrap(), base, "putting it back restores the snapshot");

    // A planted hook in the shared hooks folder.
    let hooks = PathBuf::from(git(cwd, &["rev-parse", "--git-path", "hooks"]));
    let hooks = if hooks.is_absolute() { hooks } else { cwd.join(hooks) };
    write(&hooks, "pre-push", "#!/bin/sh\nexit 0\n");
    let hook = harden::config_snapshot(cwd).unwrap();
    assert_ne!(hook.1, base.1, "a new hook file changes the hook list");
    write(&hooks, "pre-push", "#!/bin/sh\nexit 1\n");
    assert_ne!(harden::config_snapshot(cwd).unwrap().1, hook.1, "an edited hook file changes its hash");
    std::fs::remove_file(hooks.join("pre-push")).unwrap();

    // A husky file in the copy.
    write(cwd, ".husky/pre-push", "npm test\n");
    assert_ne!(harden::config_snapshot(cwd).unwrap().1, base.1, "a husky file changes the hook list");
    std::fs::remove_dir_all(cwd.join(".husky")).unwrap();

    // fsmonitor set in the local config.
    git(cwd, &["config", "core.fsmonitor", "true"]);
    let fsm = harden::config_snapshot(cwd).unwrap();
    assert_ne!(fsm.0, base.0, "core.fsmonitor shows in the config text");
    assert!(fsm.0.contains("core.fsmonitor"));
}

#[test]
fn remote_has_branch_is_true_only_for_the_exact_branch() {
    let (_guard, root, _bare, _bare_path) = repo_with_origin();
    let worktree = copy_of(&root, "remote-1");
    let cwd = PathBuf::from(&worktree.cwd);
    let env = git_env(&empty_hooks().0);
    assert!(!harden::remote_has_branch(&cwd, "origin", &worktree.branch, &env).unwrap());
    pr::push_branch(&worktree, &ToolEnv::default(), &env).unwrap();
    assert!(harden::remote_has_branch(&cwd, "origin", &worktree.branch, &env).unwrap());
    assert!(!harden::remote_has_branch(&cwd, "origin", "main-x", &env).unwrap());
    // Only the full ref name counts: `remote-1` is the tail of `refs/heads/pm/remote-1`, not a branch.
    let tail = worktree.branch.trim_start_matches("pm/").to_string();
    assert!(!harden::remote_has_branch(&cwd, "origin", &tail, &env).unwrap(), "{tail}");
}

#[test]
fn fetch_and_remote_names_refuse_option_like_or_escaping_names() {
    let (_guard, root, _bare, _bare_path) = repo_with_origin();
    let env = git_env(&empty_hooks().0);
    let project = project_str(&root);
    assert!(harden::fetch_branch(&project, "origin", "-h", &env).is_err());
    assert!(harden::fetch_branch(&project, "--upload-pack=x", "main", &env).is_err());
    assert!(harden::fetch_branch(&project, "origin", "../main", &env).is_err());
    assert!(harden::remote_has_branch(&root, "origin", "a b", &env).is_err());
    let sha = harden::fetch_branch(&project, "origin", "main", &env).unwrap();
    assert_eq!(sha, git(&root, &["rev-parse", "main"]));
}
