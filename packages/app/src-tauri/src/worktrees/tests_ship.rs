//! Tests for worktrees that ship: presets (setup, files copied in), Create PR and the checks
//! that come back, and the folder step. Real temp git repositories and a fake `gh` that is first
//! on the `PATH` handed to the code under test (no real GitHub is ever contacted).

use super::folder;
use super::pr::{self, ChecksReport, PrFacts};
use super::preset::{self, WorktreePreset};
use super::proc::ToolEnv;
use super::*;
use std::process::Command;

// ------------------------------------------------------------------------------------ helpers

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pmship-{tag}-{}", &uuid::Uuid::new_v4().to_string()[..8]));
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

/// A repo on `main` with `a.txt` and `src/lib.rs`.
fn repo() -> (Dir, PathBuf) {
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
    (Dir(root.clone()), root)
}

/// `repo()` plus a bare `origin` it can push to.
fn repo_with_origin() -> (Dir, PathBuf, Dir, PathBuf) {
    let (guard, root) = repo();
    let bare = temp_dir("origin");
    git(&bare, &["init", "-q", "--bare", "-b", "main"]);
    git(&root, &["remote", "add", "origin", &bare.to_string_lossy()]);
    git(&root, &["push", "-q", "origin", "main"]);
    (guard, root, Dir(bare.clone()), bare)
}

fn project_str(root: &Path) -> String {
    root.to_string_lossy().into_owned()
}

fn write_project_toml(root: &Path, text: &str) {
    write(root, ".puppet-master/project.toml", text);
}

// ---------------------------------------------------------------------------------- fake gh

/// A folder holding a fake `gh`. It logs every call to `calls.log`, and answers from files the
/// test drops next to it: `repo.json`, `view.json`, `checks.json`, `runlog.txt`; the markers
/// `signed_out`, `not_github` and `pr_exists` switch behaviour.
struct FakeGh {
    dir: Dir,
}

impl FakeGh {
    fn new() -> Self {
        let dir = temp_dir("gh");
        let fake = FakeGh { dir: Dir(dir.clone()) };
        fake.put("repo.json", r#"{"url":"https://github.com/o/r","defaultBranchRef":{"name":"main"}}"#);
        fake.put("view.json", r#"{"number":42,"url":"https://github.com/o/r/pull/42","isDraft":true,"state":"OPEN","baseRefName":"main"}"#);
        fake.put("checks.json", "[]");
        fake.put("runlog.txt", "");
        #[cfg(windows)]
        {
            let script = [
                "@echo off",
                "echo %* >> \"%~dp0calls.log\"",
                "if \"%1\"==\"auth\" goto auth",
                "if \"%1\"==\"repo\" goto repo",
                "if \"%1\"==\"run\" goto run",
                "if \"%1\"==\"pr\" goto pr",
                "exit /b 2",
                ":auth",
                "if exist \"%~dp0signed_out\" exit /b 1",
                "exit /b 0",
                ":repo",
                "if exist \"%~dp0not_github\" exit /b 1",
                "type \"%~dp0repo.json\"",
                "exit /b 0",
                ":run",
                "type \"%~dp0runlog.txt\"",
                "exit /b 0",
                ":pr",
                "if \"%2\"==\"view\" goto view",
                "if \"%2\"==\"create\" goto create",
                "if \"%2\"==\"checks\" goto checks",
                "exit /b 2",
                ":view",
                "if exist \"%~dp0pr_exists\" goto viewok",
                "echo no pull requests found 1>&2",
                "exit /b 1",
                ":viewok",
                "type \"%~dp0view.json\"",
                "exit /b 0",
                ":create",
                "set \"D=%~dp0\"",
                ":scan",
                "if \"%~1\"==\"\" goto created",
                "if \"%~1\"==\"--body-file\" copy /Y \"%~2\" \"%D%body.txt\" >nul",
                "shift",
                "goto scan",
                ":created",
                "echo https://github.com/o/r/pull/42",
                "exit /b 0",
                ":checks",
                "type \"%~dp0checks.json\"",
                "exit /b 1",
                "",
            ]
            .join("\r\n");
            std::fs::write(dir.join("gh.cmd"), script).unwrap();
        }
        #[cfg(not(windows))]
        {
            use std::os::unix::fs::PermissionsExt;
            let script = r#"#!/bin/sh
d="$(dirname "$0")"
echo "$@" >> "$d/calls.log"
case "$1" in
  auth) [ -e "$d/signed_out" ] && exit 1; exit 0;;
  repo) [ -e "$d/not_github" ] && exit 1; cat "$d/repo.json"; exit 0;;
  run) cat "$d/runlog.txt"; exit 0;;
  pr) case "$2" in
    view) if [ -e "$d/pr_exists" ]; then cat "$d/view.json"; exit 0; fi; echo "no pull requests found" >&2; exit 1;;
    create) while [ $# -gt 0 ]; do [ "$1" = "--body-file" ] && cp "$2" "$d/body.txt"; shift; done; echo https://github.com/o/r/pull/42; exit 0;;
    checks) cat "$d/checks.json"; exit 1;;
  esac;;
esac
exit 2
"#;
            let path = dir.join("gh");
            std::fs::write(&path, script).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        fake
    }

    fn put(&self, name: &str, text: &str) {
        std::fs::write(self.dir.0.join(name), text).unwrap();
    }

    fn mark(&self, name: &str) {
        self.put(name, "1");
    }

    fn env(&self) -> ToolEnv {
        ToolEnv::with_dir_first(&self.dir.0)
    }

    fn calls(&self) -> String {
        std::fs::read_to_string(self.dir.0.join("calls.log")).unwrap_or_default()
    }

    fn body(&self) -> String {
        std::fs::read_to_string(self.dir.0.join("body.txt")).unwrap_or_default()
    }
}

/// A worktree for `handle` with one file changed and not committed.
fn worktree_with_change(root: &Path, handle: &str) -> WorktreeRef {
    let worktree = create(&project_str(root), handle).expect("worktree");
    write(Path::new(&worktree.cwd), "src/new.rs", "pub fn two() {}\n");
    worktree
}

// ------------------------------------------------------------------------- setup and copy

#[test]
fn a_preset_is_read_from_project_toml_in_both_spellings() {
    let preset = preset::parse(
        r#"
[other]
keep = 1
[worktree]
setup = [["npm", "ci"], "npm run build"]
copy = [".env", ".env.local"]
run = ["npm", "run", "dev"]
"#,
    )
    .unwrap();
    assert_eq!(preset.setup, vec![vec!["npm".to_string(), "ci".into()], vec!["npm".into(), "run".into(), "build".into()]]);
    assert_eq!(preset.copy, vec![".env".to_string(), ".env.local".into()]);
    assert_eq!(preset.run, vec!["npm".to_string(), "run".into(), "dev".into()]);
    assert!(preset::parse("").unwrap().is_empty());
}

#[test]
fn typed_commands_split_into_words_and_shell_features_are_refused() {
    assert_eq!(preset::parse_command_line("npm ci").unwrap(), vec!["npm", "ci"]);
    assert_eq!(preset::parse_command_line("pnpm run \"build all\"").unwrap(), vec!["pnpm", "run", "build all"]);
    for bad in ["npm ci && npm run build", "cat a | sort", "echo hi > out.txt", "echo $(whoami)", "rm x; ls"] {
        assert!(preset::parse_command_line(bad).is_err(), "{bad} must be refused");
    }
    assert!(preset::parse_command_line("echo \"open").is_err());
}

#[test]
fn saving_the_preset_keeps_the_other_tables_of_the_file() {
    let (_guard, root) = repo();
    write_project_toml(&root, "[team]\nname = \"core\"\n");
    let view = preset::set_from_text(&root, &["npm ci".into(), "".into()], &[".env".into()], "npm run dev").unwrap();
    assert_eq!(view.setup, vec!["npm ci"]);
    assert_eq!(view.copy, vec![".env"]);
    assert_eq!(view.run, "npm run dev");
    let text = std::fs::read_to_string(root.join(".puppet-master/project.toml")).unwrap();
    assert!(text.contains("[team]") && text.contains("name = \"core\""), "{text}");
    assert_eq!(preset::load(&root).unwrap().setup, vec![vec!["npm".to_string(), "ci".into()]]);
    // A command that needs a shell is refused and nothing is written.
    assert!(preset::set_from_text(&root, &["npm ci && npm test".into()], &[], "").is_err());
    assert_eq!(preset::load(&root).unwrap().setup.len(), 1);
    // A path that escapes the project cannot be saved.
    assert!(preset::set_from_text(&root, &[], &["../secret".into()], "").is_err());
}

#[test]
fn conductor_json_can_be_imported_and_shell_scripts_are_explained() {
    let (_guard, root) = repo();
    write(&root, "conductor.json", r#"{"scripts":{"setup":"npm ci && npm run build","run":"npm run dev"}}"#);
    let view = preset::view(&root);
    let found = view.conductor.expect("found conductor.json");
    assert_eq!(found.setup, vec!["npm ci", "npm run build"]);
    assert_eq!(found.run, "npm run dev");
    let imported = preset::import_conductor(&root).unwrap();
    assert_eq!(imported.setup, vec!["npm ci", "npm run build"]);
    write(&root, "conductor.json", r#"{"scripts":{"setup":"cat a | sort"}}"#);
    let view = preset::view(&root);
    assert!(view.conductor.unwrap().unsupported.is_some());
}

#[test]
fn a_new_copy_runs_the_setup_in_the_copy_and_logs_it() {
    let (_guard, root) = repo();
    write_project_toml(&root, "[worktree]\nsetup = [[\"git\", \"config\", \"pm.setup-ran\", \"yes\"]]\n");
    let worktree = create(&project_str(&root), "ready-1").unwrap();
    let setup = worktree.setup.as_ref().expect("setup was recorded");
    assert_eq!(setup.status, "ok", "{setup:?}");
    assert_eq!(setup.steps.len(), 1);
    assert!(setup.steps[0].ok && setup.steps[0].command.starts_with("git config"));
    assert_eq!(git(Path::new(&worktree.cwd), &["config", "--get", "pm.setup-ran"]), "yes");
    // Not silent: the full output is in a log next to the copy, and the receipt sentence says so.
    let log = Path::new(&worktree.path).parent().unwrap().join(setup.log.as_ref().unwrap());
    assert!(std::fs::read_to_string(&log).unwrap().contains("$ git config pm.setup-ran yes"));
    assert!(setup.sentence().contains("Ran git config"), "{}", setup.sentence());
    // The record survives a restart (it is in the worktree's description file).
    let listed = list(&project_str(&root));
    assert_eq!(listed[0].worktree.setup.as_ref().unwrap().status, "ok");
    // The log goes with the copy when it is discarded.
    assert!(log.is_file());
    discard(&worktree).unwrap();
    assert!(!log.exists(), "the setup log is removed with the copy");
}

#[test]
fn a_failing_setup_is_recorded_with_its_output_and_the_copy_is_kept() {
    let (_guard, root) = repo();
    write_project_toml(
        &root,
        "[worktree]\nsetup = [[\"git\", \"definitely-not-a-command\"], [\"git\", \"config\", \"pm.second\", \"ran\"]]\n",
    );
    let worktree = create(&project_str(&root), "broken-1").unwrap();
    let setup = worktree.setup.as_ref().unwrap();
    assert!(setup.failed());
    assert_eq!(setup.steps.len(), 1, "steps after a failure do not run");
    assert!(!setup.steps[0].ok && setup.steps[0].exit_code.is_some());
    assert!(!setup.steps[0].tail.is_empty(), "the output is kept for the card");
    assert!(setup.sentence().starts_with("Setting up the copy failed:"), "{}", setup.sentence());
    assert!(Path::new(&worktree.cwd).is_dir(), "the copy is never deleted for a failed setup");
    let second = Command::new("git").args(["config", "--get", "pm.second"]).current_dir(&root).output().unwrap();
    assert!(!second.status.success());
}

#[test]
fn a_missing_tool_in_setup_is_a_plain_failure() {
    let (_guard, root) = repo();
    write_project_toml(&root, "[worktree]\nsetup = [[\"pm-no-such-tool-xyz\", \"--flag\"]]\n");
    let worktree = create(&project_str(&root), "missing-tool").unwrap();
    let step = &worktree.setup.unwrap().steps[0];
    assert!(!step.ok);
    assert!(step.tail.contains("was not found"), "{}", step.tail);
}

#[test]
fn a_setup_step_that_runs_too_long_is_stopped() {
    let (_guard, root) = repo();
    #[cfg(windows)]
    let sleeper = r#"["ping", "-n", "30", "127.0.0.1"]"#;
    #[cfg(not(windows))]
    let sleeper = r#"["sleep", "30"]"#;
    write_project_toml(&root, &format!("[worktree]\nsetup = [{sleeper}]\nsetup_timeout_secs = 1\n"));
    let started = std::time::Instant::now();
    let worktree = create(&project_str(&root), "slow-1").unwrap();
    assert!(started.elapsed() < Duration::from_secs(20), "the step was stopped, not waited for");
    let step = &worktree.setup.unwrap().steps[0];
    assert!(step.timed_out && !step.ok);
}

use std::time::Duration;

#[test]
fn env_files_are_copied_in_and_never_committed() {
    let (_guard, root) = repo();
    write(&root, ".env", "SECRET=1\n");
    write(&root, ".env.local", "LOCAL=1\n");
    write_project_toml(&root, "[worktree]\ncopy = [\".env\", \".env.local\"]\n");
    let worktree = worktree_with_change(&root, "env-1");
    let setup = worktree.setup.as_ref().unwrap();
    assert_eq!(setup.copied, vec![".env", ".env.local"]);
    let cwd = Path::new(&worktree.cwd);
    assert_eq!(std::fs::read_to_string(cwd.join(".env")).unwrap(), "SECRET=1\n");
    // Neither `git status` nor a plain `git add -A` by the agent sees them...
    let status = git(cwd, &["status", "--porcelain", "--untracked-files=all"]);
    assert!(!status.contains(".env"), "{status}");
    git(cwd, &["add", "-A"]);
    assert!(!git(cwd, &["diff", "--cached", "--name-only"]).contains(".env"));
    git(cwd, &["reset", "-q"]);
    // ...nor does Puppet Master's own commit, the changes list, or the pushed branch.
    assert!(commit_pending(&worktree).unwrap());
    let tree = git(cwd, &["ls-tree", "-r", "--name-only", &worktree.branch]);
    assert!(tree.contains("src/new.rs") && !tree.contains(".env"), "{tree}");
    let changed = changes(&worktree).unwrap();
    assert!(changed.files.iter().all(|f| !f.path.contains(".env")), "{changed:?}");
    // The project's own folder keeps its files.
    assert_eq!(std::fs::read_to_string(root.join(".env")).unwrap(), "SECRET=1\n");
}

#[test]
fn a_file_already_ignored_is_copied_without_touching_the_exclude_file() {
    let (_guard, root) = repo();
    write(&root, ".gitignore", ".env\n");
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "ignore env"]);
    write(&root, ".env", "A=1\n");
    write_project_toml(&root, "[worktree]\ncopy = [\".env\"]\n");
    let worktree = create(&project_str(&root), "env-2").unwrap();
    assert_eq!(worktree.setup.unwrap().copied, vec![".env"]);
    let exclude = std::fs::read_to_string(root.join(".git/info/exclude")).unwrap_or_default();
    assert!(!exclude.lines().any(|l| l.trim() == "/.env"), "{exclude}");
}

#[test]
fn files_that_cannot_or_should_not_be_copied_say_why() {
    let (_guard, root) = repo();
    write_project_toml(
        &root,
        "[worktree]\ncopy = [\"a.txt\", \"nope.env\", \"../outside\", \".git/config\", \".puppet-master/x\"]\n",
    );
    let worktree = create(&project_str(&root), "env-3").unwrap();
    let setup = worktree.setup.unwrap();
    assert!(setup.copied.is_empty());
    let reason = |path: &str| setup.not_copied.iter().find(|n| n.path == path).map(|n| n.reason.clone()).unwrap_or_default();
    assert!(reason("a.txt").contains("already in the project's history"), "{setup:?}");
    assert!(reason("nope.env").contains("not in the project folder"));
    for bad in ["../outside", ".git/config", ".puppet-master/x"] {
        assert!(reason(bad).contains("not a path inside the project"), "{bad}: {setup:?}");
    }
}

#[test]
fn a_project_without_a_preset_gets_a_plain_copy() {
    let (_guard, root) = repo();
    let worktree = create(&project_str(&root), "plain-1").unwrap();
    assert!(worktree.setup.is_none());
}

// ----------------------------------------------------------------------------- Create PR

#[test]
fn create_pr_pushes_the_pm_branch_and_makes_a_draft_with_the_receipt_body() {
    let (_guard, root, _bare_guard, bare) = repo_with_origin();
    let gh = FakeGh::new();
    let worktree = worktree_with_change(&root, "ship-1");
    assert!(commit_pending(&worktree).unwrap());
    let facts = PrFacts {
        task: Some("Round the cart totals to cents".into()),
        summary: Some("Claude Code changed 1 file and ran 2 commands.".into()),
        files: vec![("src/new.rs".into(), 1, 0)],
        files_total: 1,
        additions: 1,
        checks: Some("14/14 passed".into()),
        ..Default::default()
    };
    let title = pr::default_title(&facts, &worktree.branch);
    let body = pr::render_body(&facts);
    let made = pr::create(&worktree, &title, &body, "main", &gh.env()).expect("pr");
    assert_eq!((made.number, made.draft, made.base.as_str()), (42, true, "main"));
    assert_eq!(made.url, "https://github.com/o/r/pull/42");
    // The branch is on the remote, `main` was not touched.
    assert!(git(&bare, &["branch", "--list", &worktree.branch]).contains(&worktree.branch));
    assert_eq!(git(&bare, &["rev-parse", "main"]), git(&root, &["rev-parse", "main"]));
    let calls = gh.calls();
    assert!(calls.contains("pr create --draft"), "{calls}");
    assert!(calls.contains("--base main") && calls.contains(&format!("--head {}", worktree.branch)), "{calls}");
    assert!(calls.contains("Round the cart totals to cents"), "{calls}");
    // The body that reached `gh` is the receipt.
    let sent = gh.body();
    assert!(sent.contains("## What changed") && sent.contains("src/new.rs") && sent.contains("14/14 passed"), "{sent}");
}

#[test]
fn a_branch_that_has_a_pr_is_only_pushed_again() {
    let (_guard, root, _bare_guard, bare) = repo_with_origin();
    let gh = FakeGh::new();
    gh.mark("pr_exists");
    let worktree = worktree_with_change(&root, "ship-2");
    commit_pending(&worktree).unwrap();
    let first = pr::create(&worktree, "t", "b", "main", &gh.env()).unwrap();
    assert_eq!(first.number, 42);
    assert!(!gh.calls().contains("pr create"), "{}", gh.calls());
    // A new commit goes up with the second call.
    write(Path::new(&worktree.cwd), "src/more.rs", "// more\n");
    commit_pending(&worktree).unwrap();
    pr::create(&worktree, "t", "b", "main", &gh.env()).unwrap();
    let head = git(Path::new(&worktree.cwd), &["rev-parse", "HEAD"]);
    assert_eq!(git(&bare, &["rev-parse", &worktree.branch]), head);
}

#[test]
fn a_diverged_remote_branch_is_refused_never_overwritten() {
    let (_guard, root, _bare_guard, bare) = repo_with_origin();
    let gh = FakeGh::new();
    let worktree = worktree_with_change(&root, "ship-3");
    commit_pending(&worktree).unwrap();
    // Someone else already pushed a different history to the same branch name.
    let other = temp_dir("other");
    let _other_guard = Dir(other.clone());
    git(&other, &["clone", "-q", &bare.to_string_lossy(), "."]);
    git(&other, &["config", "user.name", "Other"]);
    git(&other, &["config", "user.email", "other@example.com"]);
    git(&other, &["checkout", "-q", "-b", &worktree.branch]);
    write(&other, "theirs.txt", "theirs\n");
    git(&other, &["add", "-A"]);
    git(&other, &["commit", "-q", "-m", "theirs"]);
    git(&other, &["push", "-q", "origin", &worktree.branch]);
    let theirs = git(&bare, &["rev-parse", &worktree.branch]);
    let error = pr::create(&worktree, "t", "b", "main", &gh.env()).unwrap_err();
    assert_eq!(error.code, "PUSH_FAILED", "{error:?}");
    assert!(error.message.contains("never overwrites"), "{}", error.message);
    assert_eq!(git(&bare, &["rev-parse", &worktree.branch]), theirs, "the remote branch is untouched");
    assert!(!gh.calls().contains("pr create"));
}

#[test]
fn only_pm_branches_are_pushed_never_main() {
    let (_guard, root, _bare_guard, bare) = repo_with_origin();
    let gh = FakeGh::new();
    let mut worktree = worktree_with_change(&root, "ship-4");
    let before = git(&bare, &["rev-parse", "main"]);
    for branch in ["main", "master", "feature/x", "pm/../main"] {
        worktree.branch = branch.into();
        let error = pr::create(&worktree, "t", "b", "main", &gh.env()).unwrap_err();
        assert_eq!(error.code, "PR_BRANCH_REFUSED", "{branch}");
    }
    worktree.branch = "pm/ship-4".into();
    let error = pr::create(&worktree, "t", "b", "pm/ship-4", &gh.env()).unwrap_err();
    assert_eq!(error.code, "PR_BRANCH_REFUSED", "a branch is never its own base");
    assert_eq!(git(&bare, &["rev-parse", "main"]), before);
    assert_eq!(gh.calls(), "", "gh was not even called");
}

#[test]
fn the_push_never_carries_a_force_flag() {
    let source = include_str!("pr.rs");
    let code: String = source.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
    assert!(!code.contains("--force") && !code.contains("\"-f\"") && !code.contains("--mirror"));
    assert!(!code.contains("+refs"));
}

#[test]
fn create_pr_explains_each_way_it_cannot_run() {
    let (_guard, root, _bare_guard, _bare) = repo_with_origin();
    let worktree = worktree_with_change(&root, "ship-5");
    // gh is not installed: an empty PATH has no `gh`.
    let empty = temp_dir("empty");
    let _empty_guard = Dir(empty.clone());
    let none = ToolEnv { path: Some(empty.as_os_str().to_os_string()), extra: Vec::new() };
    let status = pr::status(&worktree, &none);
    assert_eq!(status.gh, "missing");
    let error = pr::create(&worktree, "t", "b", "main", &none).unwrap_err();
    assert_eq!(error.code, "GH_MISSING");
    assert!(error.message.contains("GitHub command line"));
    // Not signed in.
    let gh = FakeGh::new();
    gh.mark("signed_out");
    assert_eq!(pr::status(&worktree, &gh.env()).gh, "signed_out");
    assert_eq!(pr::create(&worktree, "t", "b", "main", &gh.env()).unwrap_err().code, "GH_SIGNED_OUT");
    // The remote is not a GitHub repository.
    let gh = FakeGh::new();
    gh.mark("not_github");
    assert_eq!(pr::create(&worktree, "t", "b", "main", &gh.env()).unwrap_err().code, "NO_GITHUB_REMOTE");
    assert!(!pr::status(&worktree, &gh.env()).on_github);
    // No remote at all.
    let (_g2, root2) = repo();
    let local = worktree_with_change(&root2, "ship-6");
    let gh = FakeGh::new();
    let status = pr::status(&local, &gh.env());
    assert!(!status.has_remote);
    assert_eq!(pr::create(&local, "t", "b", "main", &gh.env()).unwrap_err().code, "NO_GITHUB_REMOTE");
    assert_eq!(pr::create(&local, "  ", "b", "main", &gh.env()).unwrap_err().code, "INVALID_ARGUMENT");
}

#[test]
fn the_sheet_is_prefilled_from_the_receipt() {
    let facts = PrFacts {
        task: Some("Fix the cart rounding\nand add a test".into()),
        summary: Some("Codex changed 2 files.".into()),
        files: vec![("src/money.ts".into(), 40, 6), ("src/cart.ts".into(), 12, 9)],
        files_total: 2,
        additions: 52,
        deletions: 15,
        checks: Some("14/14 passed".into()),
        second_opinion: Some("Claude Code: no concerns".into()),
        setup: Some("Ran npm ci.".into()),
        ..Default::default()
    };
    assert_eq!(pr::default_title(&facts, "pm/x"), "Fix the cart rounding");
    let body = pr::render_body(&facts);
    for wanted in [
        "## What changed",
        "Codex changed 2 files.",
        "`src/money.ts` (+40 \u{2212}6)",
        "## Checks",
        "14/14 passed",
        "## Second opinion",
        "no concerns",
        "## Setup of the copy",
    ] {
        assert!(body.contains(wanted), "missing {wanted:?} in\n{body}");
    }
    assert!(!body.to_lowercase().contains("made with"), "no provenance footer by default");
    let bare = PrFacts::default();
    assert_eq!(pr::default_title(&bare, "pm/abc"), "Changes from pm/abc");
    assert!(pr::render_body(&bare).contains("No checks were configured"));
}

// --------------------------------------------------------------------------------- checks

const FAILING: &str = r#"[
 {"name":"test (ubuntu)","state":"FAILURE","bucket":"fail","link":"https://github.com/o/r/actions/runs/777/job/1","workflow":"ci"},
 {"name":"lint","state":"FAILURE","bucket":"fail","link":"https://github.com/o/r/actions/runs/778/job/2","workflow":"ci"},
 {"name":"build","state":"SUCCESS","bucket":"pass","link":"https://github.com/o/r/actions/runs/777/job/3","workflow":"ci"}
]"#;

#[test]
fn checks_are_parsed_into_passing_pending_failing_or_none() {
    let failing = pr::parse_checks(FAILING).unwrap();
    assert_eq!(failing.state, "failing");
    assert_eq!(failing.sentence(), "2 GitHub checks failed: test (ubuntu), lint");
    let pending = pr::parse_checks(r#"[{"name":"a","state":"PENDING","bucket":"pending"},{"name":"b","state":"SUCCESS","bucket":"pass"}]"#).unwrap();
    assert_eq!(pending.state, "pending");
    let passing = pr::parse_checks(r#"[{"name":"a","state":"SUCCESS","bucket":"pass"},{"name":"b","state":"SKIPPED","bucket":"skipping"}]"#).unwrap();
    assert_eq!((passing.state.as_str(), passing.sentence().as_str()), ("passing", "All 2 GitHub checks passed"));
    assert_eq!(pr::parse_checks("[]").unwrap().state, "none");
    assert!(pr::parse_checks("not json").is_err());
}

#[test]
fn gh_exit_code_one_still_gives_the_checks() {
    let (_guard, root, _g2, _bare) = repo_with_origin();
    let gh = FakeGh::new();
    gh.put("checks.json", FAILING);
    let worktree = worktree_with_change(&root, "chk-1");
    let report = pr::checks_once(&worktree, &gh.env()).unwrap();
    assert_eq!(report.state, "failing");
    assert_eq!(report.failing().len(), 2);
}

#[test]
fn polling_is_bounded_and_stops_when_nothing_is_pending() {
    let (_guard, root, _g2, _bare) = repo_with_origin();
    let gh = FakeGh::new();
    gh.put("checks.json", r#"[{"name":"a","state":"PENDING","bucket":"pending"}]"#);
    let worktree = worktree_with_change(&root, "chk-2");
    let report = pr::poll_checks(&worktree, &gh.env(), Duration::from_millis(0), 3).unwrap();
    assert_eq!(report.state, "pending");
    let looks = gh.calls().lines().filter(|l| l.contains("pr checks")).count();
    assert_eq!(looks, 3, "exactly the bound, then it gives up");
    gh.put("checks.json", r#"[{"name":"a","state":"SUCCESS","bucket":"pass"}]"#);
    let before = gh.calls().lines().count();
    let report = pr::poll_checks(&worktree, &gh.env(), Duration::from_millis(0), 3).unwrap();
    assert_eq!(report.state, "passing");
    assert_eq!(gh.calls().lines().count(), before + 1, "one look is enough when nothing is pending");
}

#[test]
fn send_back_reads_the_failing_logs_raw_and_the_task_has_none_of_it() {
    let (_guard, root, _g2, _bare) = repo_with_origin();
    let gh = FakeGh::new();
    let mut log: String = (1..=300).map(|n| format!("line {n}\n")).collect();
    log.push_str("token ghp_abcdefghijklmnopqrstuvwxyz0123456789 mail dana.private@example.org\n");
    gh.put("runlog.txt", &log);
    let worktree = worktree_with_change(&root, "chk-3");
    let report: ChecksReport = pr::parse_checks(FAILING).unwrap();
    let raw = pr::failure_excerpt(&worktree, &report, &gh.env());
    let lines: Vec<&str> = raw.lines().collect();
    assert_eq!(lines.len(), pr::RAW_EXCERPT_LINES, "the raw tail is only input for the cleaning");
    // Two failing checks in two runs: both runs are read, each at most once.
    let reads = gh.calls().lines().filter(|l| l.contains("run view")).count();
    assert_eq!(reads, 2, "{}", gh.calls());
    assert!(gh.calls().contains("--log-failed"));

    let built = super::sendback::build_with(42, &report, &raw, &|t: &str| {
        crate::issue_intake::text::publish_text(t, &crate::redact::RedactContext::default(), None)
    });
    assert!(!built.task.contains('\n'), "one line, so a terminal never submits half of it");
    assert!(built.task.contains("#42") && built.task.contains(".puppet-master/ci-42-sendback.md"));
    for planted in ["line 300", "line 299", "ghp_", "dana.private", "test (ubuntu)", "lint"] {
        assert!(!built.task.contains(planted), "the task holds {planted:?}");
    }
    assert_eq!(built.excerpt_lines, pr::EXCERPT_LINES, "the last 40 lines go into the file");
    assert!(built.file.contains("line 300") && !built.file.contains("line 150"));
    assert!(!built.file.contains("ghp_abcdefghijklmnopqrstuvwxyz0123456789"), "{}", built.file);
    assert!(!built.file.contains("dana.private@example.org") && built.file.contains("<email>"));
    assert_eq!(built.file.matches("<<<pm-notes").count(), 1);
}

// ------------------------------------------------------------------------------ folder step

#[test]
fn git_state_tells_repo_folder_empty_repo_and_missing_apart() {
    let (_guard, root) = repo();
    assert_eq!(folder::git_state(&root), "repo");
    let plain = temp_dir("plain");
    let _plain_guard = Dir(plain.clone());
    assert_eq!(folder::git_state(&plain), "folder");
    let fresh = temp_dir("fresh");
    let _fresh_guard = Dir(fresh.clone());
    git(&fresh, &["init", "-q"]);
    assert_eq!(folder::git_state(&fresh), "no_commits");
    assert_eq!(folder::git_state(&plain.join("gone")), "missing");
}

#[test]
fn recent_projects_are_most_recent_first_without_duplicates() {
    let store = temp_dir("store");
    let _store_guard = Dir(store.clone());
    let (_g1, a) = repo();
    let (_g2, b) = repo();
    folder::remember(&store, &project_str(&a));
    folder::remember(&store, &project_str(&b));
    folder::remember(&store, &project_str(&a));
    let gone = store.join("deleted-project");
    folder::remember(&store, &gone.to_string_lossy());
    let others = vec![project_str(&b), "".to_string(), format!("{}/.puppet-master/worktrees/h", project_str(&b))];
    let listed = folder::recent(&store, &others, 8);
    let paths: Vec<&str> = listed.iter().map(|p| p.path.as_str()).collect();
    assert_eq!(paths, vec![gone.to_string_lossy().as_ref(), a.to_string_lossy().as_ref(), b.to_string_lossy().as_ref()]);
    assert_eq!(listed[0].state, "missing");
    assert_eq!(listed[1].state, "repo");
    folder::forget(&store, &gone.to_string_lossy());
    assert_eq!(folder::recent(&store, &[], 8).len(), 2);
}

#[test]
fn a_separate_copy_is_mapped_back_to_its_project_in_the_recents() {
    let store = temp_dir("store-copy");
    let _store_guard = Dir(store.clone());
    let (_g, project) = repo();
    let copy = project.join(".puppet-master").join("worktrees").join("pane-1a2b3c4d");
    std::fs::create_dir_all(&copy).unwrap();
    // The copy's own folder reaches `remember` (an old caller, a pane's cwd)…
    folder::remember(&store, &copy.to_string_lossy());
    // …and the run index (`others`) knows the copy too.
    let others = vec![copy.to_string_lossy().into_owned()];
    let listed = folder::recent(&store, &others, 8);
    assert_eq!(listed.len(), 1, "the project once, not the project and its copy: {listed:?}");
    assert_eq!(listed[0].path, project.to_string_lossy());
    assert_eq!(listed[0].name, project.file_name().unwrap().to_string_lossy());
    assert_eq!(listed[0].state, "repo");
    let saved = std::fs::read_to_string(store.join("recent-projects.json")).unwrap();
    assert!(!saved.contains("worktrees"), "the list on disk never holds a copy: {saved}");
    // Forgetting by the copy's path forgets the project.
    folder::forget(&store, &copy.to_string_lossy());
    assert!(folder::recent(&store, &[], 8).is_empty());
}

#[test]
fn a_copy_of_a_project_that_is_already_listed_does_not_move_or_duplicate_it() {
    let store = temp_dir("store-dup");
    let _store_guard = Dir(store.clone());
    let (_g1, a) = repo();
    let (_g2, b) = repo();
    folder::remember(&store, &project_str(&a));
    folder::remember(&store, &project_str(&b));
    let copy_of_a = format!("{}/.puppet-master/worktrees/h-1", project_str(&a));
    let listed = folder::recent(&store, &[copy_of_a], 8);
    let paths: Vec<&str> = listed.iter().map(|p| p.path.as_str()).collect();
    assert_eq!(paths, vec![b.to_string_lossy().as_ref(), a.to_string_lossy().as_ref()]);
}

#[test]
fn other_puppet_master_folders_are_still_not_projects() {
    let store = temp_dir("store-pm");
    let _store_guard = Dir(store.clone());
    let (_g, project) = repo();
    let operations = project.join(".puppet-master").join("operations");
    std::fs::create_dir_all(&operations).unwrap();
    folder::remember(&store, &operations.to_string_lossy());
    assert!(folder::recent(&store, &[operations.to_string_lossy().into_owned()], 8).is_empty());
}

#[test]
fn real_folder_maps_a_copy_and_leaves_a_project_alone() {
    assert_eq!(folder::real_folder(r"C:\code\shop\.puppet-master\worktrees\pane-1a2b\src"), r"C:\code\shop");
    assert_eq!(folder::real_folder("/home/me/shop/.puppet-master/worktrees/h-1"), "/home/me/shop");
    assert_eq!(folder::real_folder("  /home/me/shop  "), "/home/me/shop");
    assert_eq!(folder::real_folder(r"C:\code\shop"), r"C:\code\shop");
}

#[test]
fn set_up_git_here_shows_its_plan_then_makes_a_first_snapshot() {
    let folder_dir = temp_dir("nogit");
    let _guard = Dir(folder_dir.clone());
    write(&folder_dir, "index.html", "<p>hi</p>\n");
    write(&folder_dir, "app/main.js", "console.log(1)\n");
    write(&folder_dir, ".env", "SECRET=1\n");
    write(&folder_dir, "node_modules/dep/index.js", "x\n");
    let plan = folder::init_plan(&folder_dir).unwrap();
    assert_eq!(plan.state, "folder");
    assert_eq!(plan.commands, vec!["git init", "git add -A", "git commit -m \"First snapshot\""]);
    assert_eq!(plan.files, 2, "node_modules and .env are not counted: the .gitignore keeps them out");
    assert!(plan.gitignore.contains(&".env".to_string()));
    // Showing the plan changed nothing.
    assert_eq!(folder::git_state(&folder_dir), "folder");
    assert!(!folder_dir.join(".gitignore").exists());
    let outcome = folder::init(&folder_dir).unwrap();
    assert!(outcome.wrote_gitignore && !outcome.commit.is_empty());
    assert_eq!(folder::git_state(&folder_dir), "repo");
    assert_eq!(git(&folder_dir, &["symbolic-ref", "--short", "HEAD"]), "main");
    let tree = git(&folder_dir, &["ls-tree", "-r", "--name-only", "HEAD"]);
    assert!(tree.contains("index.html") && tree.contains("app/main.js") && tree.contains(".gitignore"), "{tree}");
    assert!(!tree.contains(".env") && !tree.contains("node_modules"), "{tree}");
    // A separate copy can now be made from it.
    assert!(create(&project_str(&folder_dir), "after-init").is_ok());
    assert_eq!(folder::init_plan(&folder_dir).unwrap_err().code, "ALREADY_GIT");
}

#[test]
fn set_up_git_here_keeps_an_existing_gitignore_and_handles_an_empty_repo() {
    let dir = temp_dir("fresh-repo");
    let _guard = Dir(dir.clone());
    git(&dir, &["init", "-q", "-b", "main"]);
    write(&dir, ".gitignore", "secret.txt\n");
    write(&dir, "secret.txt", "s\n");
    write(&dir, "a.txt", "a\n");
    let plan = folder::init_plan(&dir).unwrap();
    assert_eq!(plan.state, "no_commits");
    assert_eq!(plan.commands[0], "git add -A", "no second git init for an existing repository");
    assert!(plan.gitignore.is_empty());
    let outcome = folder::init(&dir).unwrap();
    assert!(!outcome.wrote_gitignore);
    let tree = git(&dir, &["ls-tree", "-r", "--name-only", "HEAD"]);
    assert!(tree.contains("a.txt") && !tree.contains("secret.txt"), "{tree}");
    assert_eq!(std::fs::read_to_string(dir.join(".gitignore")).unwrap(), "secret.txt\n");
}

#[test]
fn a_folder_that_does_not_exist_cannot_be_set_up() {
    let missing = std::env::temp_dir().join("pmship-definitely-missing-folder");
    assert_eq!(folder::init_plan(&missing).unwrap_err().code, "FOLDER_MISSING");
}

#[test]
fn the_preset_type_round_trips() {
    let preset = WorktreePreset {
        setup: vec![vec!["npm".into(), "ci".into()]],
        copy: vec![".env".into()],
        run: vec![],
        setup_timeout_secs: Some(90),
    };
    let (_guard, root) = repo();
    preset::save(&root, &preset).unwrap();
    assert_eq!(preset::load(&root).unwrap(), preset);
}

// ------------------------------------------------------------------------------------ live check

/// The live check: a real project (`PM_LIVE_PROJECT`, run against D:\pm-demo), real `git`, real `npm` and
/// `node`, a preset, a separate copy, a pretend agent edit, and Create PR through the FAKE `gh` into a
/// temporary bare repository. No real GitHub call is made and no agent turn is spent. Everything it
/// touches in the project is put back. Run: `PM_LIVE_PROJECT=D:/pm-demo cargo test --lib live_demo -- --ignored --nocapture`.
#[test]
#[ignore]
fn live_demo_a_preset_a_copy_and_a_draft_pr_through_a_fake_gh() {
    let Ok(project) = std::env::var("PM_LIVE_PROJECT") else { return };
    let project_dir = PathBuf::from(&project);
    let toml_path = project_dir.join(".puppet-master/project.toml");
    let before_toml = std::fs::read_to_string(&toml_path).ok();
    let env_path = project_dir.join(".env");
    assert!(!env_path.exists(), "the live project already has a .env; refusing to overwrite it");
    let bare = temp_dir("live-origin");
    let _bare_guard = Dir(bare.clone());
    git(&bare, &["init", "-q", "--bare", "-b", "main"]);
    let had_origin = Command::new("git").args(["remote", "get-url", "origin"]).current_dir(&project_dir).output().map(|o| o.status.success()).unwrap_or(false);
    assert!(!had_origin, "the live project already has an origin remote");

    write(&project_dir, ".env", "LIVE_SECRET=demo\n");
    write_project_toml(
        &project_dir,
        "[worktree]\nsetup = [[\"node\", \"--version\"], [\"npm\", \"--version\"], [\"node\", \"-p\", \"40+2\"]]\ncopy = [\".env\"]\nrun = [\"npm\", \"test\"]\n",
    );
    git(&project_dir, &["remote", "add", "origin", &bare.to_string_lossy()]);

    let handle = format!("live-{}", &uuid::Uuid::new_v4().to_string()[..6]);
    let result = std::panic::catch_unwind(|| {
        let started = std::time::Instant::now();
        let worktree = create(&project, &handle).expect("worktree");
        let setup = worktree.setup.clone().expect("setup ran");
        println!("LIVE setup status={} took={}ms", setup.status, started.elapsed().as_millis());
        for step in &setup.steps {
            println!("LIVE   step `{}` ok={} exit={:?} {}ms", step.command, step.ok, step.exit_code, step.duration_ms);
        }
        println!("LIVE   copied={:?} not_copied={:?}", setup.copied, setup.not_copied);
        println!("LIVE   sentence: {}", setup.sentence());
        let log = Path::new(&worktree.path).parent().unwrap().join(setup.log.as_ref().unwrap());
        println!("LIVE   log {} ({} bytes)", log.display(), std::fs::metadata(&log).map(|m| m.len()).unwrap_or(0));
        assert_eq!(setup.status, "ok", "{setup:?}");
        assert!(Path::new(&worktree.cwd).join(".env").is_file());

        // A pretend agent turn: one edit in the copy (no real agent was run).
        write(Path::new(&worktree.cwd), "live-note.txt", "written in the copy\n");
        let gh = FakeGh::new();
        let facts = PrFacts {
            task: Some("Live check of Create PR".into()),
            summary: Some("A pretend agent added live-note.txt.".into()),
            files: vec![("live-note.txt".into(), 1, 0)],
            files_total: 1,
            additions: 1,
            checks: Some("1/1 passed".into()),
            setup: Some(setup.sentence()),
            ..Default::default()
        };
        assert!(commit_pending(&worktree).unwrap());
        let made = pr::create(&worktree, &pr::default_title(&facts, &worktree.branch), &pr::render_body(&facts), "master", &gh.env()).expect("pr");
        println!("LIVE PR #{} {} draft={}", made.number, made.url, made.draft);
        println!("LIVE   remote branches: {}", git(&bare, &["branch", "--list"]).replace('\n', " | "));
        println!("LIVE   gh calls: {}", gh.calls().replace("\r\n", " || ").replace('\n', " || "));
        println!("LIVE   body sent to gh:\n{}", gh.body());
        let tree = git(Path::new(&worktree.cwd), &["ls-tree", "-r", "--name-only", &worktree.branch]);
        assert!(!tree.contains(".env"), ".env must never be in the branch: {tree}");
        println!("LIVE   .env in the pushed branch: no (tree has {} files)", tree.lines().count());
        assert!(git(&bare, &["branch", "--list"]).contains(&worktree.branch));
        discard(&worktree).expect("discard");
    });

    // Put the project back exactly as it was.
    let _ = Command::new("git").args(["remote", "remove", "origin"]).current_dir(&project_dir).output();
    let _ = std::fs::remove_file(&env_path);
    match before_toml {
        Some(text) => std::fs::write(&toml_path, text).unwrap(),
        None => {
            let _ = std::fs::remove_file(&toml_path);
        }
    }
    let exclude = project_dir.join(".git/info/exclude");
    if let Ok(text) = std::fs::read_to_string(&exclude) {
        let kept: Vec<&str> = text
            .lines()
            .filter(|l| l.trim() != "/.env" && l.trim() != "# Puppet Master: files copied into separate copies (never committed)")
            .collect();
        let _ = std::fs::write(&exclude, kept.join("\n") + "\n");
    }
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}
