//! The `no_push` environment guard for an agent's pane: the pane cannot push to GitHub.
//!
//! What the guard does to a pane's environment:
//! - `gh` reads no login (`GH_CONFIG_DIR` points at an empty private folder) and never prompts.
//! - Every `https://`, `git@` and `ssh://` push URL is rewritten to a remote helper that does not
//!   exist, so the push fails before anything is sent.
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

/// Git config set for the pane through `GIT_CONFIG_COUNT`. The push rewrites send every push URL
/// of the three kinds to a helper named `pm-push-blocked`, which does not exist.
const GIT_OVERRIDES: &[(&str, &str)] = &[
    ("url.pm-push-blocked:///.pushInsteadOf", "https://"),
    ("url.pm-push-blocked:///.pushInsteadOf", "git@"),
    ("url.pm-push-blocked:///.pushInsteadOf", "ssh://"),
    // An empty helper resets the helper list, so no stored credential is handed out.
    ("credential.helper", ""),
];

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

/// The environment for a pane under `guard`, or `None` for an unknown guard name. `gh_empty_dir`
/// must be an existing, empty, private folder (`issue_intake::store::gh_empty_dir`).
pub fn launch_env(guard: &str, gh_empty_dir: &Path) -> Option<LaunchEnv> {
    let current: Vec<(String, String)> = std::env::vars_os()
        .map(|(k, v)| (k.to_string_lossy().into_owned(), v.to_string_lossy().into_owned()))
        .collect();
    launch_env_from(guard, gh_empty_dir, &current)
}

/// `launch_env` with the current environment passed in, so tests do not depend on the machine.
fn launch_env_from(guard: &str, gh_empty_dir: &Path, env: &[(String, String)]) -> Option<LaunchEnv> {
    if guard != NO_PUSH {
        return None;
    }
    let mut set: Vec<(String, String)> = vec![
        ("GH_CONFIG_DIR".into(), gh_empty_dir.display().to_string()),
        ("GH_PROMPT_DISABLED".into(), "1".into()),
        ("GIT_TERMINAL_PROMPT".into(), "0".into()),
        ("GCM_INTERACTIVE".into(), "never".into()),
        ("GIT_CONFIG_COUNT".into(), GIT_OVERRIDES.len().to_string()),
    ];
    for (i, (key, value)) in GIT_OVERRIDES.iter().enumerate() {
        set.push((format!("GIT_CONFIG_KEY_{i}"), (*key).into()));
        set.push((format!("GIT_CONFIG_VALUE_{i}"), (*value).into()));
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

    #[test]
    fn unknown_guard_name_is_refused() {
        assert!(launch_env_from("", &dir(), &[]).is_none());
        assert!(launch_env_from("no-push", &dir(), &[]).is_none());
        assert!(launch_env_from("NO_PUSH", &dir(), &[]).is_none());
        assert!(launch_env_from("push", &dir(), &[]).is_none());
        assert!(launch_env("nope", &dir()).is_none());
        assert!(launch_env(NO_PUSH, &dir()).is_some());
    }

    #[test]
    fn sets_the_gh_and_git_prompt_variables() {
        let env = launch_env_from(NO_PUSH, &dir(), &[]).expect("guard");
        assert_eq!(set_value(&env, "GH_CONFIG_DIR"), Some(dir().display().to_string().as_str()));
        assert_eq!(set_value(&env, "GH_PROMPT_DISABLED"), Some("1"));
        assert_eq!(set_value(&env, "GIT_TERMINAL_PROMPT"), Some("0"));
        assert_eq!(set_value(&env, "GCM_INTERACTIVE"), Some("never"));
    }

    #[test]
    fn sets_exactly_four_git_config_pairs() {
        let env = launch_env_from(NO_PUSH, &dir(), &[]).expect("guard");
        assert_eq!(set_value(&env, "GIT_CONFIG_COUNT"), Some("4"));
        let pairs = [
            ("GIT_CONFIG_KEY_0", "url.pm-push-blocked:///.pushInsteadOf", "GIT_CONFIG_VALUE_0", "https://"),
            ("GIT_CONFIG_KEY_1", "url.pm-push-blocked:///.pushInsteadOf", "GIT_CONFIG_VALUE_1", "git@"),
            ("GIT_CONFIG_KEY_2", "url.pm-push-blocked:///.pushInsteadOf", "GIT_CONFIG_VALUE_2", "ssh://"),
            ("GIT_CONFIG_KEY_3", "credential.helper", "GIT_CONFIG_VALUE_3", ""),
        ];
        for (kk, kv, vk, vv) in pairs {
            assert_eq!(set_value(&env, kk), Some(kv), "{kk}");
            assert_eq!(set_value(&env, vk), Some(vv), "{vk}");
        }
        // Exactly the variables the spec lists: 4 fixed, COUNT, and 4 key/value pairs.
        let git_keys = env.set.iter().filter(|(k, _)| k.starts_with("GIT_CONFIG_")).count();
        assert_eq!(git_keys, 9);
        assert_eq!(env.set.len(), 4 + 1 + 8);
    }

    #[test]
    fn removes_every_listed_variable() {
        let env = launch_env_from(NO_PUSH, &dir(), &[]).expect("guard");
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
        let env = launch_env_from(NO_PUSH, &dir(), &inherited).expect("guard");
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
        let env = launch_env_from(NO_PUSH, &dir(), &inherited).expect("guard");
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
        std::fs::create_dir_all(&work).expect("work dir");
        std::fs::create_dir_all(&gh_empty).expect("gh dir");
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

        let guard = launch_env_from(NO_PUSH, &gh_empty, &[]).expect("guard");

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

    #[test]
    fn agent_names_stay_out_of_this_file() {
        let src = include_str!("env_guard.rs");
        let generic = src.split("#[cfg(test)]").next().unwrap_or(src).to_ascii_lowercase();
        for name in ["claude", "codex", "cursor", "opencode", "agy"] {
            assert!(!generic.contains(name), "agent name `{name}` in generic env_guard code");
        }
    }
}
