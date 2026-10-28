//! A guarded pane's launch env: account < hooks < pane id < guard (the guard is last and wins).

use super::*;
use crate::pty::env_guard;
use crate::tui::{LaunchEnv, LaunchSpec};

fn pairs(items: &[(&str, &str)]) -> Vec<(String, String)> {
    items.iter().map(|(k, v)| ((*k).to_string(), (*v).to_string())).collect()
}

fn account() -> LaunchEnv {
    LaunchEnv {
        set: pairs(&[("GH_CONFIG_DIR", "C:/account/gh"), ("ACCOUNT_HOME", "C:/account/home"), ("ORDER", "account")]),
        remove: vec!["ACCOUNT_DROP".into()],
    }
}

fn hooks() -> Vec<(String, String)> {
    pairs(&[("PM_HOOK_TOKEN", "hook-token"), ("ORDER", "hooks")])
}

fn guard() -> LaunchEnv {
    env_guard::launch_env(env_guard::NO_PUSH, std::path::Path::new("C:/app/gh-empty"), std::path::Path::new("C:/app/push-blocked-hooks"))
        .expect("guard")
}

fn value_of(env: &LaunchEnv, key: &str) -> Option<String> {
    // The last set wins, like the child process sees it.
    env.set.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v.clone())
}

#[test]
fn without_a_guard_the_env_is_the_account_then_the_hooks_then_the_pane_id() {
    let env = spawn::compose_pane_env(account(), hooks(), "pane-1", None);
    assert_eq!(value_of(&env, "ORDER").as_deref(), Some("hooks"), "hooks win over the account");
    assert_eq!(value_of(&env, "GH_CONFIG_DIR").as_deref(), Some("C:/account/gh"));
    assert_eq!(value_of(&env, crate::pty::PANE_ID_ENV).as_deref(), Some("pane-1"));
    assert_eq!(env.remove, vec!["ACCOUNT_DROP".to_string()]);
}

#[test]
fn a_guarded_env_is_account_then_hooks_then_guard() {
    let env = spawn::compose_pane_env(account(), hooks(), "pane-1", Some(guard()));
    // Account and hook values the guard does not touch stay, hooks still win over the account.
    assert_eq!(value_of(&env, "ACCOUNT_HOME").as_deref(), Some("C:/account/home"));
    assert_eq!(value_of(&env, "ORDER").as_deref(), Some("hooks"));
    assert_eq!(value_of(&env, "PM_HOOK_TOKEN").as_deref(), Some("hook-token"));
    assert_eq!(value_of(&env, crate::pty::PANE_ID_ENV).as_deref(), Some("pane-1"));
    // The guard wins over the account: GH_CONFIG_DIR is the guard's empty folder.
    assert_eq!(value_of(&env, "GH_CONFIG_DIR").as_deref(), Some("C:/app/gh-empty"));
    assert_eq!(value_of(&env, "GIT_CONFIG_COUNT").as_deref(), Some("6"));
    // The guard's sets are after the account's and the hooks' in the list.
    let at = |name: &str| env.set.iter().position(|(k, _)| k == name).unwrap();
    assert!(at("GH_CONFIG_DIR") > at("ACCOUNT_HOME"));
    assert!(at("GH_CONFIG_DIR") > at("PM_HOOK_TOKEN"));
    assert!(at("GH_CONFIG_DIR") > at(crate::pty::PANE_ID_ENV));
    // The account's own removes stay, and the guard's removes are added after them.
    assert_eq!(env.remove.first().map(String::as_str), Some("ACCOUNT_DROP"));
    assert!(env.remove.iter().any(|r| r == "GH_TOKEN"));
}

#[test]
fn a_guard_never_strips_the_hook_token_or_pane_id_even_when_the_app_inherited_them() {
    let mut guard = guard();
    guard.remove.push("PM_HOOK_TOKEN".into());
    guard.remove.push(crate::pty::PANE_ID_ENV.into());
    let env = spawn::compose_pane_env(account(), hooks(), "pane-1", Some(guard));
    assert!(!env.remove.iter().any(|r| r == "PM_HOOK_TOKEN" || r == crate::pty::PANE_ID_ENV));
    assert_eq!(value_of(&env, "PM_HOOK_TOKEN").as_deref(), Some("hook-token"));
}

#[test]
fn the_launch_command_gets_the_guard_last_and_drops_the_tokens() {
    // A profile launch (`spawn_command`): the definition's own env first, then the pane env.
    let mut spec = LaunchSpec::new("definitely-not-run", vec![], std::env::temp_dir(), false);
    spec.env = pairs(&[("GH_CONFIG_DIR", "C:/definition/gh"), ("SPEC_ONLY", "kept"), ("PM_TEST_SPEC_TOKEN", "leak")]);
    let mut guard = guard();
    guard.remove.push("PM_TEST_SPEC_TOKEN".into());
    let env = spawn::compose_pane_env(account(), hooks(), "pane-1", Some(guard));
    let cmd = command::build_launch_spec_command(&spec, &env);
    let get = |key: &str| cmd.get_env(key).map(|v| v.to_string_lossy().into_owned());
    assert_eq!(get("GH_CONFIG_DIR").as_deref(), Some("C:/app/gh-empty"), "guard beats definition and account");
    assert_eq!(get("SPEC_ONLY").as_deref(), Some("kept"));
    assert_eq!(get("GIT_TERMINAL_PROMPT").as_deref(), Some("0"));
    assert_eq!(get("PM_HOOK_TOKEN").as_deref(), Some("hook-token"));
    assert_eq!(get("PM_TEST_SPEC_TOKEN"), None, "removed after every set");
}
