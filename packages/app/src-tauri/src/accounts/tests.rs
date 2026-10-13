use super::*;

fn temp_store(name: &str) -> (Store, PathBuf) {
    let root = std::env::temp_dir().join(format!("pm-accounts-{name}-{}", uuid::Uuid::new_v4()));
    (Store::at(&root), root)
}

#[test]
fn crud_round_trips_through_accounts_json() {
    let (store, root) = temp_store("crud");
    assert!(store.list().is_empty());
    let work = store.add("claude", "Work", None).unwrap();
    let personal = store.add("claude", "Personal", None).unwrap();
    assert!(work.id.starts_with("acc_") && work.id != personal.id);
    assert_eq!(work.agent_id, "claude");
    assert!(Path::new(&work.home).is_dir(), "default home is created");
    assert!(Path::new(&work.home).starts_with(root.join("accounts")));
    assert_eq!(store.list().len(), 2);

    // persisted: a fresh Store on the same folder sees it, as one JSON file
    let again = Store::at(&root);
    assert_eq!(again.get(&work.id), Some(work.clone()));
    let text = std::fs::read_to_string(root.join("accounts.json")).unwrap();
    assert!(text.contains("\"agent_id\": \"claude\"") && text.contains("\"label\": \"Work\""));

    let renamed = store.rename(&work.id, "  Work (Max)  ").unwrap();
    assert_eq!(renamed.label, "Work (Max)");
    assert_eq!(store.get(&work.id).unwrap().label, "Work (Max)");
    assert!(store.rename("acc_missing", "x").is_err());

    assert_eq!(store.remove(&personal.id, false), Ok(false));
    assert_eq!(store.list().len(), 1);
    assert!(store.remove(&personal.id, false).is_err(), "already gone");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn labels_are_unique_per_agent_and_validated() {
    let (store, root) = temp_store("labels");
    store.add("claude", "Work", None).unwrap();
    assert!(store.add("claude", "work", None).unwrap_err().contains("already has"));
    store.add("codex", "Work", None).expect("same label under another agent is fine");
    assert!(store.add("claude", "   ", None).is_err());
    assert!(store.add("claude", &"x".repeat(41), None).is_err());
    let b = store.add("claude", "B", None).unwrap();
    assert!(store.rename(&b.id, "WORK").is_err());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn only_agents_whose_profile_declares_a_scheme_get_accounts() {
    let (store, root) = temp_store("support");
    assert!(store.add("claude", "a", None).is_ok());
    assert!(store.add("codex", "a", None).is_ok());
    // no verified env var for these: left out on purpose (docs/features/account-profiles.md)
    for agent in ["opencode", "opencode_native", "cursor_agent", "cmd"] {
        let err = store.add(agent, "a", None).unwrap_err();
        assert!(err.contains("does not support accounts") || err.contains("unknown agent"), "{agent}: {err}");
    }
    assert!(store.add("no_such_agent", "a", None).unwrap_err().contains("unknown agent"));
    let ids: Vec<String> = supporting_agents().into_iter().map(|a| a.agent_id).collect();
    assert!(ids.contains(&"claude".to_string()) && ids.contains(&"codex".to_string()));
    assert!(!ids.contains(&"opencode".to_string()));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn account_maps_to_the_profiles_home_env_var() {
    let (store, root) = temp_store("env");
    let claude = store.add("claude", "Work", None).unwrap();
    let codex = store.add("codex", "Work", None).unwrap();
    let env = store.launch_env("claude", &claude.id).unwrap();
    assert_eq!(env.set, vec![("CLAUDE_CONFIG_DIR".to_string(), claude.home.clone())]);
    assert!(env.remove.is_empty());
    let env = store.launch_env("codex", &codex.id).unwrap();
    assert_eq!(env.set, vec![("CODEX_HOME".to_string(), codex.home.clone())]);
    // an account belongs to one agent
    assert!(store.launch_env("codex", &claude.id).unwrap_err().contains("belongs to claude"));
    assert!(store.launch_env("claude", "acc_nope").unwrap_err().contains("unknown account"));
    // extra variables from the scheme ride along
    let scheme = AccountScheme {
        home_env: "X_HOME".into(),
        extra_env: vec![("X_FLAG".into(), "1".into())],
        login_hint: String::new(),
    };
    assert_eq!(env_for(&scheme, &claude).set, vec![("X_HOME".into(), claude.home.clone()), ("X_FLAG".into(), "1".into())]);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn removing_never_deletes_the_home_unless_asked_and_only_app_owned_ones() {
    let (store, root) = temp_store("remove");
    let kept = store.add("claude", "kept", None).unwrap();
    assert_eq!(store.remove(&kept.id, false), Ok(false));
    assert!(Path::new(&kept.home).is_dir(), "home survives a plain remove");

    let gone = store.add("claude", "gone", None).unwrap();
    assert_eq!(store.remove(&gone.id, true), Ok(true));
    assert!(!Path::new(&gone.home).exists(), "the explicit flag deletes the app-owned home");

    // a folder the user pointed at is never deleted, flag or not
    let user_dir = std::env::temp_dir().join(format!("pm-user-home-{}", uuid::Uuid::new_v4()));
    let custom = store.add("claude", "custom", Some(user_dir.to_str().unwrap())).unwrap();
    assert_eq!(custom.home, user_dir.to_string_lossy());
    assert_eq!(store.remove(&custom.id, true), Ok(false));
    assert!(user_dir.is_dir());
    assert!(store.add("claude", "rel", Some("relative/dir")).is_err());
    let _ = std::fs::remove_dir_all(user_dir);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_run_naming_an_unsupported_agent_or_unknown_account_is_refused_before_spawning() {
    assert!(validate_for_run("opencode", "acc_x").unwrap_err().contains("does not support accounts"));
    assert!(validate_for_run("claude", "acc_does_not_exist").unwrap_err().contains("unknown account"));
    assert!(validate_for_run("no_such_agent", "acc_x").unwrap_err().contains("unknown agent"));
}
