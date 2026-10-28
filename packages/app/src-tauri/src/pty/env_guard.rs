//! The `no_push` environment guard for an agent's pane: the pane cannot push to GitHub.
//!
//! What the guard does to a pane's environment:
//! - `gh` reads no login (`GH_CONFIG_DIR` points at an empty private folder) and never prompts.
//! - A push to ANY destination fails, whatever the URL looks like (a web URL, a folder, a UNC share
//!   path, `file://`). Two layers: (1) every push URL is rewritten (`pushInsteadOf` with an empty
//!   prefix matches every URL) to a remote helper that does not exist, so the push fails before
//!   anything is sent; (2) `core.hooksPath` points at a PM folder whose `pre-push` hook always
//!   refuses, which also stops a push to a `pushurl` (that setting skips the rewrite).
//!   Residual, by design of git: `git push --no-verify` skips the hook, and an agent that edits
//!   the git settings itself is caught after the run (the push check holds the run).
//! - No stored git credential is handed out (`credential.helper` is reset) and git never prompts.
//! - GitHub variables (`GITHUB_*`, `GH_*`, except the two the guard sets) and token, secret,
//!   password and `_PAT` variables the app inherited are removed from the pane.
//!
//! Residual, documented in Settings: the agent still runs as the person and can read files on disk.
//! `*_API_KEY` variables are kept, because agents use them to log in. This module is generic: it
//! never names an agent, and the caller decides which runs ask for the guard.

use std::path::Path;

use crate::tui::LaunchEnv;

/// The guard name a run asks for. An unknown guard name is refused by the spawn (fail closed).
pub const NO_PUSH: &str = "no_push";

/// Variables removed from every guarded pane by name.
pub const REMOVE: &[&str] = &[
    "GH_TOKEN",
    "GITHUB_TOKEN",
    "GH_ENTERPRISE_TOKEN",
    "GITHUB_ENTERPRISE_TOKEN",
    "GIT_ASKPASS",
    "SSH_ASKPASS",
    "SSH_AUTH_SOCK",
    "GCM_CREDENTIAL_STORE",
    "NPM_TOKEN",
    "NODE_AUTH_TOKEN",
    "CARGO_REGISTRY_TOKEN",
    "PYPI_TOKEN",
    "TWINE_PASSWORD",
    "TWINE_USERNAME",
    "AWS_ACCESS_KEY_ID",
    "AWS_SECRET_ACCESS_KEY",
    "AWS_SESSION_TOKEN",
    "AZURE_CLIENT_SECRET",
    "GOOGLE_APPLICATION_CREDENTIALS",
    "DOCKER_AUTH_CONFIG",
    "HEROKU_API_KEY",
    "VERCEL_TOKEN",
    "NETLIFY_AUTH_TOKEN",
    "CLOUDFLARE_API_TOKEN",
];

/// Any variable whose name starts with one of these is removed too (case-insensitive): the GitHub
/// CLI and GitHub variables, so no GitHub login or token reaches the pane under another name.
const REMOVE_PREFIXES: &[&str] = &["GITHUB_", "GH_"];

/// The variables the guard sets itself. They start with `GH_`, so they are exempt from the prefix
/// rule (the set is applied before the removes, so a removal would undo them).
const GUARD_SETS: &[&str] = &["GH_CONFIG_DIR", "GH_PROMPT_DISABLED"];

/// Any variable whose name ends with one of these is removed too (case-insensitive).
const REMOVE_SUFFIXES: &[&str] = &["_TOKEN", "_SECRET", "_PASSWORD", "_PAT"];

/// Kept even when a suffix above would match: LLM keys are how agents log in.
const KEEP_SUFFIX: &str = "_API_KEY";

/// The push rewrite key: its target is a remote helper named `pm-push-blocked`, which does not exist.
const PUSH_BLOCK_KEY: &str = "url.pm-push-blocked:///.pushInsteadOf";

/// The file name of the hook in the hooks folder (git runs `pre-push` before it sends anything).
const PRE_PUSH: &str = "pre-push";

/// The hook: refuses every push and says why. Git for Windows runs it with its own `sh`.
const PRE_PUSH_HOOK: &str = "#!/bin/sh
echo 'Puppet Master blocks pushes while an agent works here. It pushes after its own checks.' >&2
exit 1
";

/// Git config set for the pane through `GIT_CONFIG_COUNT`: the push rewrites (the three web kinds,
/// then the empty prefix that matches every other URL shape), the blocking hooks folder, and no
/// stored credential.
fn git_overrides(hooks_dir: &Path) -> Vec<(&'static str, String)> {
    vec![
        (PUSH_BLOCK_KEY, "https://".into()),
        (PUSH_BLOCK_KEY, "git@".into()),
        (PUSH_BLOCK_KEY, "ssh://".into()),
        // An empty prefix is a prefix of every URL: a folder, a UNC path, `file://`, anything.
        (PUSH_BLOCK_KEY, String::new()),
        ("core.hooksPath", hooks_dir.display().to_string()),
        // An empty helper resets the helper list, so no stored credential is handed out.
        ("credential.helper", String::new()),
    ]
}

/// One writer at a time: two panes can start together, and an atomic replace of a file that
/// another thread is replacing fails on Windows.
static HOOK_WRITE: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Writes the refusing `pre-push` hook into `dir` (an existing private folder), over whatever is
/// there. Called at every guarded launch, so an edited hook is put right again. A hook that is
/// already right is left alone.
pub fn write_push_block_hook(dir: &Path) -> Result<(), String> {
    let path = dir.join(PRE_PUSH);
    let _one_at_a_time = HOOK_WRITE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let is_right = || std::fs::read_to_string(&path).is_ok_and(|text| text == PRE_PUSH_HOOK);
    if is_right() {
        return Ok(());
    }
    if let Err(error) = crate::secure_file::write_private(&path, PRE_PUSH_HOOK) {
        // Another process may have put the same text there in the meantime.
        return if is_right() { Ok(()) } else { Err(error) };
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
            .map_err(|err| format!("make {} runnable: {err}", path.display()))?;
    }
    Ok(())
}

/// True when a variable with this name is removed from a guarded pane.
pub fn removable(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    if REMOVE.iter().any(|r| r.eq_ignore_ascii_case(name)) {
        return true;
    }
    if GUARD_SETS.iter().any(|g| g.eq_ignore_ascii_case(name)) {
        return false;
    }
    if REMOVE_PREFIXES.iter().any(|p| upper.starts_with(p)) {
        return true;
    }
    if upper.ends_with(KEEP_SUFFIX) {
        return false;
    }
    REMOVE_SUFFIXES.iter().any(|s| upper.ends_with(s))
}

/// True when `guard` names a guard this build knows. Callers refuse anything else (fail closed).
pub fn known(guard: &str) -> bool {
    guard == NO_PUSH
}

/// `base` (the account's env, then the pane's hook env) with the guard on top. The guard comes
/// last, so it wins: its sets replace an earlier set of the same name (names compare ignoring
/// case, as on Windows), and its removes apply even to a variable `base` set. Names in `keep` are
/// the app's own pane variables (hook token, pane id); a guard remove never strips those.
pub fn layer(mut base: LaunchEnv, guard: LaunchEnv, keep: &[String]) -> LaunchEnv {
    let same = |a: &str, b: &str| a.eq_ignore_ascii_case(b);
    let kept = |name: &str| keep.iter().any(|k| same(k, name));
    let stripped: Vec<String> = guard.remove.into_iter().filter(|name| !kept(name)).collect();
    // Earlier sets the guard replaces or strips are dropped, so the vector shows the guard last.
    base.set.retain(|(name, _)| {
        !guard.set.iter().any(|(g, _)| same(name, g)) && !stripped.iter().any(|r| same(name, r))
    });
    base.set.extend(guard.set);
    for name in stripped {
        if !base.remove.iter().any(|r| same(r, &name)) {
            base.remove.push(name);
        }
    }
    base
}

/// The environment for a pane under `guard`, or `None` for an unknown guard name. `gh_empty_dir`
/// must be an existing, empty, private folder (`issue_intake::store::gh_empty_dir`); `hooks_dir`
/// an existing private folder that holds the hook from `write_push_block_hook`.
pub fn launch_env(guard: &str, gh_empty_dir: &Path, hooks_dir: &Path) -> Option<LaunchEnv> {
    let current: Vec<(String, String)> = std::env::vars_os()
        .map(|(k, v)| (k.to_string_lossy().into_owned(), v.to_string_lossy().into_owned()))
        .collect();
    launch_env_from(guard, gh_empty_dir, hooks_dir, &current)
}

/// `launch_env` with the current environment passed in, so tests do not depend on the machine.
fn launch_env_from(guard: &str, gh_empty_dir: &Path, hooks_dir: &Path, env: &[(String, String)]) -> Option<LaunchEnv> {
    if guard != NO_PUSH {
        return None;
    }
    let mut set: Vec<(String, String)> = vec![
        ("GH_CONFIG_DIR".into(), gh_empty_dir.display().to_string()),
        ("GH_PROMPT_DISABLED".into(), "1".into()),
        ("GIT_TERMINAL_PROMPT".into(), "0".into()),
        ("GCM_INTERACTIVE".into(), "never".into()),
    ];
    let overrides = git_overrides(hooks_dir);
    set.push(("GIT_CONFIG_COUNT".into(), overrides.len().to_string()));
    for (i, (key, value)) in overrides.into_iter().enumerate() {
        set.push((format!("GIT_CONFIG_KEY_{i}"), key.into()));
        set.push((format!("GIT_CONFIG_VALUE_{i}"), value));
    }

    let mut remove: Vec<String> = REMOVE.iter().map(|r| (*r).to_string()).collect();
    for (name, _) in env {
        if removable(name) && !remove.iter().any(|r| r == name) {
            remove.push(name.clone());
        }
    }
    Some(LaunchEnv { set, remove })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn env_of(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs.iter().map(|(k, v)| ((*k).to_string(), (*v).to_string())).collect()
    }

    fn set_value<'a>(env: &'a LaunchEnv, key: &str) -> Option<&'a str> {
        env.set.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    fn dir() -> PathBuf {
        PathBuf::from("C:/app/issue-intake/gh-empty")
    }

    fn hooks() -> PathBuf {
        PathBuf::from("C:/app/issue-intake/push-blocked-hooks")
    }

    #[test]
    fn unknown_guard_name_is_refused() {
        assert!(launch_env_from("", &dir(), &hooks(), &[]).is_none());
        assert!(launch_env_from("no-push", &dir(), &hooks(), &[]).is_none());
        assert!(launch_env_from("NO_PUSH", &dir(), &hooks(), &[]).is_none());
        assert!(launch_env_from("push", &dir(), &hooks(), &[]).is_none());
        assert!(launch_env("nope", &dir(), &hooks()).is_none());
        assert!(launch_env(NO_PUSH, &dir(), &hooks()).is_some());
    }

    #[test]
    fn sets_the_gh_and_git_prompt_variables() {
        let env = launch_env_from(NO_PUSH, &dir(), &hooks(), &[]).expect("guard");
        assert_eq!(set_value(&env, "GH_CONFIG_DIR"), Some(dir().display().to_string().as_str()));
        assert_eq!(set_value(&env, "GH_PROMPT_DISABLED"), Some("1"));
        assert_eq!(set_value(&env, "GIT_TERMINAL_PROMPT"), Some("0"));
        assert_eq!(set_value(&env, "GCM_INTERACTIVE"), Some("never"));
    }

    #[test]
    fn sets_exactly_six_git_config_pairs() {
        let env = launch_env_from(NO_PUSH, &dir(), &hooks(), &[]).expect("guard");
        assert_eq!(set_value(&env, "GIT_CONFIG_COUNT"), Some("6"));
        let hooks_path = hooks().display().to_string();
        let pairs = [
            ("GIT_CONFIG_KEY_0", "url.pm-push-blocked:///.pushInsteadOf", "GIT_CONFIG_VALUE_0", "https://"),
            ("GIT_CONFIG_KEY_1", "url.pm-push-blocked:///.pushInsteadOf", "GIT_CONFIG_VALUE_1", "git@"),
            ("GIT_CONFIG_KEY_2", "url.pm-push-blocked:///.pushInsteadOf", "GIT_CONFIG_VALUE_2", "ssh://"),
            // The empty prefix matches every URL shape: a folder, a UNC path, file://.
            ("GIT_CONFIG_KEY_3", "url.pm-push-blocked:///.pushInsteadOf", "GIT_CONFIG_VALUE_3", ""),
            ("GIT_CONFIG_KEY_4", "core.hooksPath", "GIT_CONFIG_VALUE_4", hooks_path.as_str()),
            ("GIT_CONFIG_KEY_5", "credential.helper", "GIT_CONFIG_VALUE_5", ""),
        ];
        for (kk, kv, vk, vv) in pairs {
            assert_eq!(set_value(&env, kk), Some(kv), "{kk}");
            assert_eq!(set_value(&env, vk), Some(vv), "{vk}");
        }
        // 4 fixed variables, COUNT, and 6 key/value pairs.
        let git_keys = env.set.iter().filter(|(k, _)| k.starts_with("GIT_CONFIG_")).count();
        assert_eq!(git_keys, 13);
        assert_eq!(env.set.len(), 4 + 1 + 12);
    }

    #[test]
    fn removes_every_listed_variable() {
        let env = launch_env_from(NO_PUSH, &dir(), &hooks(), &[]).expect("guard");
        for name in REMOVE {
            assert!(env.remove.iter().any(|r| r == name), "{name} not removed");
            assert!(removable(name), "{name} not removable");
        }
        assert_eq!(REMOVE.len(), 24);
    }

    #[test]
    fn removes_github_gh_and_pat_names_but_not_the_guards_own_variables() {
        for name in ["GITHUB_PAT", "GH_PAT", "GITHUB_ENTERPRISE_TOKEN", "gh_custom", "github_custom", "Gh_Other"] {
            assert!(removable(name), "{name} must be removed");
        }
        let inherited = env_of(&[("GITHUB_PAT", "x"), ("GH_PAT", "x"), ("gh_custom", "x")]);
        let env = launch_env_from(NO_PUSH, &dir(), &hooks(), &inherited).expect("guard");
        for name in ["GITHUB_PAT", "GH_PAT", "gh_custom"] {
            assert!(env.remove.iter().any(|r| r == name), "{name} not in remove list");
        }
        // The guard's own variables survive: set and never removed.
        assert!(!removable("GH_CONFIG_DIR"));
        assert!(!removable("gh_config_dir"), "case-insensitive match keeps it too");
        assert!(!removable("GH_PROMPT_DISABLED"));
        assert!(!env.remove.iter().any(|r| r == "GH_CONFIG_DIR"));
        assert!(!env.remove.iter().any(|r| r == "GH_PROMPT_DISABLED"));
        assert_eq!(set_value(&env, "GH_CONFIG_DIR"), Some(dir().display().to_string().as_str()));
        assert_eq!(set_value(&env, "GH_PROMPT_DISABLED"), Some("1"));
        // Other names are not touched by the new rules.
        assert!(!removable("PATH_EXTRA"));
        assert!(!removable("OPENAI_API_KEY"));
    }

    #[test]
    fn removes_suffix_matches_and_keeps_api_keys_and_path() {
        assert!(removable("MY_SERVICE_TOKEN"));
        assert!(removable("MY_SERVICE_SECRET"));
        assert!(removable("MY_SERVICE_PASSWORD"));
        assert!(removable("my_service_token"), "suffix match is case-insensitive");
        assert!(!removable("OPENAI_API_KEY"));
        assert!(!removable("MY_SERVICE_API_KEY"));
        assert!(!removable("PATH"));
        assert!(!removable("HOME"));
        assert!(!removable("GH_CONFIG_DIR"));

        let inherited = env_of(&[
            ("MY_SERVICE_TOKEN", "x"),
            ("OPENAI_API_KEY", "x"),
            ("PATH", "x"),
            ("GH_TOKEN", "x"),
        ]);
        let env = launch_env_from(NO_PUSH, &dir(), &hooks(), &inherited).expect("guard");
        assert!(env.remove.iter().any(|r| r == "MY_SERVICE_TOKEN"));
        assert!(env.remove.iter().any(|r| r == "GH_TOKEN"));
        assert!(!env.remove.iter().any(|r| r == "OPENAI_API_KEY"));
        assert!(!env.remove.iter().any(|r| r == "PATH"));
        // No duplicates when a listed name is also inherited.
        assert_eq!(env.remove.iter().filter(|r| *r == "GH_TOKEN").count(), 1);
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).expect("clock").as_nanos();
        std::env::temp_dir().join(format!("pm-env-guard-{tag}-{}-{nanos}", std::process::id()))
    }

    fn git(dir: &Path, args: &[&str], guard: Option<&LaunchEnv>) -> std::process::Output {
        let mut cmd = Command::new("git");
        cmd.current_dir(dir).args(args);
        if let Some(env) = guard {
            for (k, v) in &env.set {
                cmd.env(k, v);
            }
            for k in &env.remove {
                cmd.env_remove(k);
            }
        }
        cmd.output().expect("run git")
    }

    #[test]
    fn real_git_push_fails_under_the_guard_and_succeeds_without_it() {
        let root = temp_dir("push");
        let bare = root.join("remote.git");
        let work = root.join("work");
        let gh_empty = root.join("gh-empty");
        let hooks_dir = root.join("push-blocked-hooks");
        std::fs::create_dir_all(&work).expect("work dir");
        std::fs::create_dir_all(&gh_empty).expect("gh dir");
        std::fs::create_dir_all(&hooks_dir).expect("hooks dir");
        write_push_block_hook(&hooks_dir).expect("hook");
        assert!(git(&root, &["init", "-q", "--bare", bare.to_str().unwrap()], None).status.success());
        assert!(git(&work, &["init", "-q"], None).status.success());
        assert!(git(&work, &["config", "user.email", "pm@example.invalid"], None).status.success());
        assert!(git(&work, &["config", "user.name", "pm"], None).status.success());
        std::fs::write(work.join("a.txt"), "one\n").expect("file");
        assert!(git(&work, &["add", "a.txt"], None).status.success());
        assert!(git(&work, &["commit", "-q", "-m", "one"], None).status.success());

        // A GitHub-style https remote that is really the local bare repo (insteadOf, set by the
        // repo itself, as a fetch mirror would), so a plain push would succeed.
        let bare_url = bare.display().to_string().replace('\\', "/");
        assert!(git(&work, &["remote", "add", "origin", "https://github.com/acme/billing.git"], None)
            .status
            .success());
        let key = format!("url.{bare_url}.insteadOf");
        assert!(git(&work, &["config", &key, "https://github.com/acme/billing.git"], None).status.success());

        let guard = launch_env_from(NO_PUSH, &gh_empty, &hooks_dir, &[]).expect("guard");

        // The guard's own config is what git sees: all three push rewrites are present.
        let shown = git(&work, &["config", "--get-all", "url.pm-push-blocked:///.pushInsteadOf"], Some(&guard));
        let shown = String::from_utf8_lossy(&shown.stdout).to_string();
        assert!(shown.contains("https://") && shown.contains("git@") && shown.contains("ssh://"), "{shown}");

        // Guarded: the https push is rewritten to a helper that does not exist, so it fails.
        let blocked = git(&work, &["push", "origin", "HEAD:refs/heads/pm/blocked"], Some(&guard));
        assert!(!blocked.status.success(), "guarded push must fail");
        let remote_heads = git(&bare, &["show-ref"], None);
        assert!(!String::from_utf8_lossy(&remote_heads.stdout).contains("pm/blocked"));

        // Not guarded (PM's own push): the same remote accepts the branch.
        let plain = git(&work, &["push", "origin", "HEAD:refs/heads/pm/ok"], None);
        assert!(plain.status.success(), "{}", String::from_utf8_lossy(&plain.stderr));
        let remote_heads = git(&bare, &["show-ref"], None);
        assert!(String::from_utf8_lossy(&remote_heads.stdout).contains("pm/ok"));
    }

    /// A work repo with one commit, a local bare repo `remote.git` that a push would reach, the guard's
    /// folders, and the guard env. Returns (root, work, bare, guard).
    fn rig(tag: &str) -> (PathBuf, PathBuf, PathBuf, LaunchEnv) {
        let root = temp_dir(tag);
        let (work, bare) = (root.join("work"), root.join("remote.git"));
        let (gh_empty, hooks_dir) = (root.join("gh-empty"), root.join("push-blocked-hooks"));
        for d in [&work, &gh_empty, &hooks_dir] {
            std::fs::create_dir_all(d).expect("dir");
        }
        write_push_block_hook(&hooks_dir).expect("hook");
        assert!(git(&root, &["init", "-q", "--bare", bare.to_str().unwrap()], None).status.success());
        assert!(git(&work, &["init", "-q"], None).status.success());
        assert!(git(&work, &["config", "user.email", "pm@example.invalid"], None).status.success());
        assert!(git(&work, &["config", "user.name", "pm"], None).status.success());
        std::fs::write(work.join("a.txt"), "one\n").expect("file");
        assert!(git(&work, &["add", "a.txt"], None).status.success());
        assert!(git(&work, &["commit", "-q", "-m", "one"], None).status.success());
        let guard = launch_env_from(NO_PUSH, &gh_empty, &hooks_dir, &[]).expect("guard");
        (root, work, bare, guard)
    }

    fn remote_refs(bare: &Path) -> String {
        String::from_utf8_lossy(&git(bare, &["show-ref"], None).stdout).to_string()
    }

    /// The push was stopped by the guard (the rewrite or the hook), not by something else.
    fn stopped_by_guard(out: &std::process::Output) -> bool {
        let text = String::from_utf8_lossy(&out.stderr).to_string();
        !out.status.success() && (text.contains("pm-push-blocked") || text.contains("Puppet Master blocks pushes"))
    }

    #[test]
    fn a_push_to_any_url_shape_is_stopped_by_the_guard() {
        let (_root, work, bare, guard) = rig("shapes");
        let plain_path = bare.display().to_string();
        let slashes = plain_path.replace('\\', "/");
        let shapes = [
            ("folder path", plain_path.clone()),
            ("folder path with slashes", slashes.clone()),
            ("relative folder", "../remote.git".to_string()),
            ("file url", format!("file:///{}", slashes.trim_start_matches('/'))),
            ("unc path", r"\\no-such-server.invalid\share\repo.git".to_string()),
            ("unc path with slashes", "//no-such-server.invalid/share/repo.git".to_string()),
            ("https", "https://github.com/acme/billing.git".to_string()),
            ("scp style", "git@github.com:acme/billing.git".to_string()),
            ("ssh", "ssh://git@example.invalid/acme/billing.git".to_string()),
            ("git protocol", "git://example.invalid/acme/billing.git".to_string()),
            ("http", "http://example.invalid/acme/billing.git".to_string()),
        ];
        for (name, url) in shapes {
            // As a URL argument.
            let out = git(&work, &["push", &url, "HEAD:refs/heads/leak"], Some(&guard));
            assert!(stopped_by_guard(&out), "{name} as an argument: {}", String::from_utf8_lossy(&out.stderr));
            // As a configured remote.
            let remote = format!("r-{}", name.replace(' ', "-"));
            assert!(git(&work, &["remote", "add", &remote, &url], None).status.success(), "{name}");
            let out = git(&work, &["push", &remote, "HEAD:refs/heads/leak"], Some(&guard));
            assert!(stopped_by_guard(&out), "{name} as a remote: {}", String::from_utf8_lossy(&out.stderr));
        }
        assert!(remote_refs(&bare).is_empty(), "the folder remote changed: {}", remote_refs(&bare));
    }

    #[test]
    fn a_folder_remote_accepts_a_push_without_the_guard_and_never_with_it() {
        let (_root, work, bare, guard) = rig("folder");
        let path = bare.display().to_string();
        assert!(git(&work, &["remote", "add", "share", &path], None).status.success());
        let blocked = git(&work, &["push", "share", "HEAD:refs/heads/pm/blocked"], Some(&guard));
        assert!(stopped_by_guard(&blocked), "{}", String::from_utf8_lossy(&blocked.stderr));
        assert!(remote_refs(&bare).is_empty());
        // PM's own push (no guard env) still works on the same remote.
        let plain = git(&work, &["push", "share", "HEAD:refs/heads/pm/ok"], None);
        assert!(plain.status.success(), "{}", String::from_utf8_lossy(&plain.stderr));
        assert!(remote_refs(&bare).contains("pm/ok"));
    }

    #[test]
    fn a_pushurl_skips_the_rewrite_but_the_hook_stops_it() {
        let (_root, work, bare, guard) = rig("pushurl");
        let path = bare.display().to_string();
        assert!(git(&work, &["remote", "add", "origin", "https://github.com/acme/billing.git"], None).status.success());
        assert!(git(&work, &["remote", "set-url", "--push", "origin", &path], None).status.success());
        let out = git(&work, &["push", "origin", "HEAD:refs/heads/leak"], Some(&guard));
        assert!(stopped_by_guard(&out), "{}", String::from_utf8_lossy(&out.stderr));
        assert!(String::from_utf8_lossy(&out.stderr).contains("Puppet Master blocks pushes"), "the hook did not run");
        assert!(remote_refs(&bare).is_empty(), "{}", remote_refs(&bare));
    }

    #[test]
    fn fetching_from_a_folder_still_works_under_the_guard() {
        let (_root, work, bare, guard) = rig("fetch");
        let path = bare.display().to_string();
        assert!(git(&work, &["push", &path, "HEAD:refs/heads/main"], None).status.success());
        assert!(git(&work, &["remote", "add", "share", &path], None).status.success());
        let out = git(&work, &["fetch", "share"], Some(&guard));
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let ls = git(&work, &["ls-remote", "share"], Some(&guard));
        assert!(ls.status.success() && String::from_utf8_lossy(&ls.stdout).contains("refs/heads/main"));
    }

    #[test]
    fn the_hook_is_written_and_put_right_again() {
        let dir = temp_dir("hook");
        std::fs::create_dir_all(&dir).expect("dir");
        write_push_block_hook(&dir).expect("write");
        let text = std::fs::read_to_string(dir.join("pre-push")).expect("hook file");
        assert!(text.starts_with("#!/bin/sh\n") && text.contains("exit 1"), "{text}");
        assert!(!text.contains('\r'), "the hook must use LF line ends");
        std::fs::write(dir.join("pre-push"), "#!/bin/sh\nexit 0\n").expect("tamper");
        write_push_block_hook(&dir).expect("write again");
        assert_eq!(std::fs::read_to_string(dir.join("pre-push")).unwrap(), text);
    }

    fn pairs(items: &[(&str, &str)]) -> Vec<(String, String)> {
        env_of(items)
    }

    #[test]
    fn only_the_known_name_is_known() {
        assert!(known(NO_PUSH));
        for name in ["", "NO_PUSH", "no-push", "no_push ", "push"] {
            assert!(!known(name), "{name:?}");
        }
    }

    #[test]
    fn the_guard_layer_wins_over_the_account_and_hook_env() {
        let base = LaunchEnv {
            set: pairs(&[("GH_CONFIG_DIR", "C:/account/gh"), ("PM_HOOK_TOKEN", "t"), ("KEEP_ME", "1")]),
            remove: vec!["FROM_ACCOUNT".into()],
        };
        let guard = launch_env_from(NO_PUSH, &dir(), &hooks(), &env_of(&[("GH_TOKEN", "x"), ("PM_HOOK_TOKEN", "inherited")]))
            .expect("guard");
        let out = layer(base, guard, &["PM_HOOK_TOKEN".to_string()]);
        // The guard's value replaced the account's, and is the only one left, after the others.
        assert_eq!(out.set.iter().filter(|(k, _)| k == "GH_CONFIG_DIR").count(), 1);
        assert_eq!(set_value(&out, "GH_CONFIG_DIR"), Some(dir().display().to_string().as_str()));
        assert_eq!(set_value(&out, "KEEP_ME"), Some("1"), "unrelated env stays");
        let guard_at = out.set.iter().position(|(k, _)| k == "GH_CONFIG_DIR").unwrap();
        let kept_at = out.set.iter().position(|(k, _)| k == "KEEP_ME").unwrap();
        assert!(guard_at > kept_at, "the guard's sets come last");
        // The account's own removes stay, the guard's removes are added.
        assert!(out.remove.iter().any(|r| r == "FROM_ACCOUNT"));
        assert!(out.remove.iter().any(|r| r == "GH_TOKEN"));
        // The app's own pane variable is never stripped by the guard.
        assert_eq!(set_value(&out, "PM_HOOK_TOKEN"), Some("t"));
        assert!(!out.remove.iter().any(|r| r == "PM_HOOK_TOKEN"));
    }

    #[test]
    fn the_guard_strips_a_token_the_account_or_hooks_set() {
        let base = LaunchEnv { set: pairs(&[("GH_TOKEN", "from-account"), ("NPM_TOKEN", "x")]), remove: vec![] };
        let guard = launch_env_from(NO_PUSH, &dir(), &hooks(), &[]).expect("guard");
        let out = layer(base, guard, &[]);
        assert!(set_value(&out, "GH_TOKEN").is_none() && set_value(&out, "NPM_TOKEN").is_none());
        assert!(out.remove.iter().any(|r| r == "GH_TOKEN") && out.remove.iter().any(|r| r == "NPM_TOKEN"));
    }

    #[test]
    fn layer_compares_names_ignoring_case() {
        let base = LaunchEnv { set: pairs(&[("gh_config_dir", "C:/account/gh")]), remove: vec![] };
        let guard = launch_env_from(NO_PUSH, &dir(), &hooks(), &[]).expect("guard");
        let out = layer(base, guard, &[]);
        assert!(set_value(&out, "gh_config_dir").is_none(), "the lower-case twin is replaced");
        assert_eq!(set_value(&out, "GH_CONFIG_DIR"), Some(dir().display().to_string().as_str()));
    }

    #[test]
    fn agent_names_stay_out_of_this_file() {
        let src = include_str!("env_guard.rs");
        let generic = src.split("#[cfg(test)]").next().unwrap_or(src).to_ascii_lowercase();
        for name in ["claude", "codex", "cursor", "opencode", "agy"] {
            assert!(!generic.contains(name), "agent name `{name}` in generic env_guard code");
        }
    }
}
