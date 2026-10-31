//! The brain's environment: an allowlist built from nothing. The key appears only as DEEPSEEK_API_KEY
//! and only for the DeepSeek brain; Debug prints names only; no PUPPET_MASTER_* variable, no port file,
//! no local secret.

use std::path::Path;

use super::env::*;

const KEY: &str = "sk-test-deepseek-key-1234567890";
const TOKEN: &str = "tok-0123456789abcdef0123456789abcdef";

/// What a busy desktop session has in its environment, including everything the brain must never see.
fn busy_host() -> HostEnv {
    let mut values: Vec<(String, String)> = [
        ("PATH", "C:\\Windows;C:\\node"),
        ("SystemRoot", "C:\\Windows"),
        ("WINDIR", "C:\\Windows"),
        ("TEMP", "C:\\Temp"),
        ("TMP", "C:\\Temp"),
        ("USERPROFILE", "C:\\Users\\someone"),
        ("HOME", "/home/someone"),
        ("APPDATA", "C:\\Users\\someone\\AppData\\Roaming"),
        ("LOCALAPPDATA", "C:\\Users\\someone\\AppData\\Local"),
        ("XDG_DATA_HOME", "/home/someone/.local/share"),
        ("PUPPET_MASTER_PROFILE", "dev"),
        ("PUPPET_MASTER_PORT_FILE", "C:\\x\\bridge.port"),
        ("PUPPET_MASTER_LOCAL_SECRET", "local-secret-value"),
        ("ANTHROPIC_API_KEY", "sk-ant-should-never-pass"),
        ("OPENAI_API_KEY", "sk-should-never-pass"),
        ("GITHUB_TOKEN", "ghp_should_never_pass"),
        ("DISCORD_TOKEN", "should-never-pass-in-phase-one"),
        ("DEEPSEEK_API_KEY", "an-inherited-key-must-not-pass"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    values.sort();
    HostEnv { values }
}

fn inputs<'a>(brain: Brain, host: &'a HostEnv, key: Option<&'a str>) -> Inputs<'a> {
    Inputs {
        brain,
        pack: "fish",
        model: None,
        vault: Path::new("D:/data/potato/brain/nimbus/vault"),
        persona_dir: Path::new("D:/data/potato/brain/nimbus/persona"),
        state_dir: Path::new("D:/data/potato/brain/nimbus/vault/state"),
        share_dir: Path::new("D:/data/potato/brain/nimbus/share-empty"),
        url: "http://127.0.0.1:17999",
        token: TOKEN,
        bot: "nimbus",
        deepseek_key: key,
        host,
    }
}

fn allowed(name: &str, brain: Brain) -> bool {
    let host = ["PATH", "SystemRoot", "WINDIR", "TEMP", "TMP"];
    let own = [
        "FISHD_PROVIDER", "FISHD_MODEL", "FISHD_VAULT", "FISHD_PERSONA_DIR", "FISHD_STATE_DIR", "FISHD_PACK", "FISHD_VOICE", "FISHD_GATE", "FISHD_SHARE_DIR",
        "PM_POTATO_URL", "PM_POTATO_TOKEN", "PM_POTATO_BOT",
    ];
    let home = ["HOME", "USERPROFILE", "APPDATA", "LOCALAPPDATA", "XDG_DATA_HOME"];
    host.contains(&name) || own.contains(&name) || (name == "DEEPSEEK_API_KEY" && brain == Brain::DeepSeek) || (brain == Brain::Go && home.contains(&name))
}

#[test]
fn the_deepseek_brain_gets_exactly_the_allowlist() {
    let host = busy_host();
    let env = build(&inputs(Brain::DeepSeek, &host, Some(KEY)));
    for name in env.names() {
        assert!(allowed(name, Brain::DeepSeek), "{name} is not on the allowlist");
    }
    for must in ["PATH", "SystemRoot", "WINDIR", "TEMP", "TMP", "FISHD_PROVIDER", "FISHD_VAULT", "FISHD_PERSONA_DIR", "FISHD_STATE_DIR", "FISHD_PACK", "FISHD_VOICE", "FISHD_GATE", "FISHD_SHARE_DIR", "PM_POTATO_URL", "PM_POTATO_TOKEN", "PM_POTATO_BOT", "DEEPSEEK_API_KEY"] {
        assert!(env.names().contains(&must), "{must} is missing");
    }
    assert_eq!(env.get("FISHD_PROVIDER"), Some("deepseek"));
    assert_eq!(env.get("FISHD_VOICE"), Some("0"));
    assert_eq!(env.get("FISHD_GATE"), Some("heuristic"));
    assert_eq!(env.get("FISHD_PACK"), Some("fish"));
    assert_eq!(env.get("PM_POTATO_BOT"), Some("nimbus"));
    assert_eq!(env.get("PM_POTATO_URL"), Some("http://127.0.0.1:17999"));
    assert_eq!(env.get("FISHD_SHARE_DIR"), Some("D:/data/potato/brain/nimbus/share-empty"));
}

#[test]
fn the_deepseek_brain_gets_no_home_variables() {
    let host = busy_host();
    let env = build(&inputs(Brain::DeepSeek, &host, Some(KEY)));
    for name in ["HOME", "USERPROFILE", "APPDATA", "LOCALAPPDATA", "XDG_DATA_HOME"] {
        assert!(!env.names().contains(&name), "{name} must not reach the DeepSeek brain");
    }
}

#[test]
fn the_go_brain_gets_the_home_variables_and_no_key() {
    let host = busy_host();
    // Even if a key is handed in, only the DeepSeek brain gets it.
    let env = build(&inputs(Brain::Go, &host, Some(KEY)));
    for name in ["HOME", "USERPROFILE", "APPDATA", "LOCALAPPDATA", "XDG_DATA_HOME"] {
        assert!(env.names().contains(&name), "{name}");
    }
    assert!(!env.names().contains(&"DEEPSEEK_API_KEY"));
    assert_eq!(env.get("FISHD_PROVIDER"), Some("opencode"));
    for name in env.names() {
        assert!(allowed(name, Brain::Go), "{name}");
    }
}

#[test]
fn the_key_appears_only_as_deepseek_api_key_and_nowhere_else() {
    let host = busy_host();
    let env = build(&inputs(Brain::DeepSeek, &host, Some(KEY)));
    assert_eq!(env.get("DEEPSEEK_API_KEY"), Some(KEY));
    let holders: Vec<&str> = env.names().into_iter().filter(|n| env.get(n).is_some_and(|v| v.contains(KEY))).collect();
    assert_eq!(holders, ["DEEPSEEK_API_KEY"]);
    // no key, no variable (an empty key is no key)
    assert!(!build(&inputs(Brain::DeepSeek, &host, None)).names().contains(&"DEEPSEEK_API_KEY"));
    assert!(!build(&inputs(Brain::DeepSeek, &host, Some(""))).names().contains(&"DEEPSEEK_API_KEY"));
}

#[test]
fn an_inherited_key_or_token_never_passes() {
    let host = busy_host();
    let env = build(&inputs(Brain::DeepSeek, &host, None));
    for (name, secret) in [("ANTHROPIC_API_KEY", "sk-ant"), ("OPENAI_API_KEY", "sk-should"), ("GITHUB_TOKEN", "ghp_"), ("DISCORD_TOKEN", "should-never"), ("DEEPSEEK_API_KEY", "an-inherited")] {
        assert!(!env.names().contains(&name), "{name}");
        for n in env.names() {
            assert!(!env.get(n).unwrap().contains(secret), "{n} carries {secret}");
        }
    }
}

#[test]
fn no_puppet_master_variable_reaches_the_brain() {
    let host = busy_host();
    for brain in [Brain::DeepSeek, Brain::Go] {
        let env = build(&inputs(brain, &host, Some(KEY)));
        for name in env.names() {
            assert!(!name.to_ascii_uppercase().starts_with("PUPPET_MASTER"), "{name}");
        }
        let all = env.names().into_iter().map(|n| env.get(n).unwrap().to_string()).collect::<Vec<_>>().join("\n");
        assert!(!all.contains("bridge.port"), "no port file path");
        assert!(!all.contains("local-secret-value"), "no local secret");
    }
}

#[test]
fn debug_prints_names_only() {
    let host = busy_host();
    let env = build(&inputs(Brain::DeepSeek, &host, Some(KEY)));
    let shown = format!("{env:?}");
    assert!(shown.contains("DEEPSEEK_API_KEY") && shown.contains("PM_POTATO_TOKEN"), "{shown}");
    for value in [KEY, TOKEN, "C:\\Windows", "D:/data", "http://127.0.0.1"] {
        assert!(!shown.contains(value), "Debug shows a value: {value}\n{shown}");
    }
    let pretty = format!("{env:#?}");
    assert!(!pretty.contains(KEY) && !pretty.contains(TOKEN));
}

#[test]
fn the_secrets_list_is_the_key_and_the_token_for_the_log_cleaner() {
    let host = busy_host();
    let env = build(&inputs(Brain::DeepSeek, &host, Some(KEY)));
    let mut secrets = env.secrets();
    secrets.sort();
    assert_eq!(secrets, {
        let mut want = vec![KEY.to_string(), TOKEN.to_string()];
        want.sort();
        want
    });
    assert_eq!(build(&inputs(Brain::Go, &host, None)).secrets(), vec![TOKEN.to_string()]);
}

#[test]
fn the_environment_starts_from_nothing_when_it_is_applied() {
    let host = busy_host();
    let env = build(&inputs(Brain::DeepSeek, &host, Some(KEY)));
    // Build the command that would be spawned (never run) and read back what it would carry.
    let mut command = crate::proc_util::hidden_command("node");
    command.env("INHERITED_FROM_PARENT", "yes");
    env.apply(&mut command);
    assert!(command.get_envs().all(|(_, v)| v.is_some()), "env_clear leaves nothing to inherit, only what was set");
    let names: Vec<String> = command.get_envs().map(|(k, _)| k.to_string_lossy().into_owned()).collect();
    assert!(!names.contains(&"INHERITED_FROM_PARENT".to_string()), "{names:?}");
    for name in &names {
        assert!(allowed(name, Brain::DeepSeek), "{name}");
    }
}

#[test]
fn the_key_is_never_in_an_argument() {
    // The environment is the only way the key travels; the command line holds a script path and nothing else.
    let src = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/potato/sidecar.rs")).unwrap();
    let code = src.split("#[cfg(test)]").next().unwrap();
    assert!(code.contains(".arg(&spec.script)"));
    assert!(!code.contains(".args("), "the brain is started with its script and no other argument");
    assert!(!code.contains("deepseek_key") && !code.contains("DEEPSEEK"), "sidecar.rs never touches the key");
}

#[test]
fn a_model_override_is_passed_as_fishd_model_only_when_set() {
    let host = busy_host();
    let mut i = inputs(Brain::DeepSeek, &host, Some(KEY));
    assert!(!build(&i).names().contains(&"FISHD_MODEL"));
    i.model = Some("  ");
    assert!(!build(&i).names().contains(&"FISHD_MODEL"));
    i.model = Some("deepseek-flash");
    assert_eq!(build(&i).get("FISHD_MODEL"), Some("deepseek-flash"));
}
