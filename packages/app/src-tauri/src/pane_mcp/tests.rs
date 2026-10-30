//! The launch each agent gets for Puppet Master's own MCP server, from the real profiles.

use super::*;
use crate::tui::profile_for;
use serde_json::json;

const PORT_FILE: &str = "C:/Users/a b/AppData/Roaming/com.puppetmaster.app/puppet-master.bridge.port";

fn dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pm-pane-mcp-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn server(pane_id: &str) -> PaneMcpServer {
    server_with("C:/Program Files/Puppet Master/bin/puppet-master-mcp.exe".into(), vec![], pane_id, PORT_FILE.into())
}

fn arg_after<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).map(String::as_str)
}

fn entry_env(entry: &Value) -> (&str, &str) {
    (entry["PUPPET_MASTER_BRIDGE_PORT_FILE"].as_str().unwrap(), entry["PUPPET_MASTER_PANE_ID"].as_str().unwrap())
}

#[test]
fn claude_gets_a_config_file_for_this_pane_and_the_note_as_added_system_text() {
    let d = dir("claude");
    let launch = prepare_in(profile_for("claude").as_ref(), "pane-1", &[], &server("pane-1"), &d);
    assert!(launch.env.is_empty(), "no variable: nothing in the person's environment changes");
    let file = arg_after(&launch.args, "--mcp-config").expect("--mcp-config");
    assert!(file.ends_with("/pane-1.mcp.json"), "{file}");
    assert_eq!(arg_after(&launch.args, "--append-system-prompt"), Some(NOTE));
    let text = std::fs::read_to_string(d.join("pane-1.mcp.json")).expect("file written");
    let written: Value = serde_json::from_str(&text).unwrap();
    let entry = &written["mcpServers"]["puppet-master"];
    assert_eq!(entry["command"], "C:/Program Files/Puppet Master/bin/puppet-master-mcp.exe");
    // THIS app's port file (never a hard-coded port) and the pane the agent runs in.
    assert_eq!(entry_env(&entry["env"]), (PORT_FILE, "pane-1"));
    assert!(!text.contains("17321"));
}

#[test]
fn claude_keeps_the_persons_own_system_text() {
    let d = dir("claude-own");
    let mine = vec!["--append-system-prompt".to_string(), "be brief".to_string()];
    let launch = prepare_in(profile_for("claude").as_ref(), "pane-2", &mine, &server("pane-2"), &d);
    assert!(arg_after(&launch.args, "--mcp-config").is_some(), "the server is still added");
    assert!(!launch.args.iter().any(|a| a == "--append-system-prompt"), "{:?}", launch.args);
}

#[test]
fn opencode_gets_one_variable_holding_the_config_and_a_note_file_in_it() {
    let d = dir("opencode");
    let profile = profile_for("opencode");
    // The profile's table, with the person's environment left out of it.
    let table = profile.pane_mcp_table().expect("opencode declares a way");
    let s = server("pane-3");
    let request = PaneMcpRequest { pane_id: "pane-3", user_args: &[], server: &s, dir: &d, note: NOTE };
    let launch = crate::tui::pane_mcp::render(&table, &request, &|_| false);
    assert!(launch.args.is_empty(), "no argument: the TUI's own arguments stay the person's");
    let (name, text) = &launch.env[0];
    assert_eq!(name, "OPENCODE_CONFIG_CONTENT");
    let config: Value = serde_json::from_str(text).expect("one JSON text");
    let entry = &config["mcp"]["puppet-master"];
    assert_eq!(entry["type"], "local");
    assert_eq!(entry["enabled"], true);
    assert_eq!(entry["command"], json!(["C:/Program Files/Puppet Master/bin/puppet-master-mcp.exe"]));
    assert_eq!(entry_env(&entry["environment"]), (PORT_FILE, "pane-3"));
    let note_path = config["instructions"][0].as_str().expect("instructions");
    assert!(note_path.ends_with("/helpers-note.md"), "{note_path}");
    // Through the profile as the spawn calls it (unless the developer's own shell sets the variable).
    if std::env::var_os("OPENCODE_CONFIG_CONTENT").is_none() {
        let launch = prepare_in(profile.as_ref(), "pane-3", &[], &s, &d);
        assert_eq!(launch.env.len(), 1);
        assert_eq!(std::fs::read_to_string(d.join("helpers-note.md")).unwrap().trim(), NOTE);
    }
}

#[test]
fn opencode_leaves_a_config_the_person_set_in_their_environment_alone() {
    let table = profile_for("opencode").pane_mcp_table().expect("table");
    let s = server("pane-4");
    let request = PaneMcpRequest { pane_id: "pane-4", user_args: &[], server: &s, dir: Path::new("C:/d"), note: NOTE };
    let launch = crate::tui::pane_mcp::render(&table, &request, &|name| name == "OPENCODE_CONFIG_CONTENT");
    assert_eq!(launch, PaneMcpLaunch::default());
}

#[test]
fn codex_gets_command_line_overrides_and_no_file() {
    let d = dir("codex");
    let launch = prepare_in(profile_for("codex").as_ref(), "pane-5", &[], &server("pane-5"), &d);
    assert!(launch.env.is_empty());
    assert_eq!(launch.args.iter().filter(|a| *a == "-c").count(), 3);
    assert!(launch.args.iter().any(|a| a.starts_with("mcp_servers.puppet-master.command=")));
    assert!(launch.args.iter().any(|a| a.contains("PUPPET_MASTER_PANE_ID=\"pane-5\"") && a.contains(PORT_FILE)));
    assert!(!d.exists(), "no folder was made for an agent that needs no file");
}

#[test]
fn agents_with_no_verified_way_start_exactly_as_before() {
    let d = dir("none");
    for id in ["cursor_agent", "cursor", "opencode_native", "shell", "powershell"] {
        let Some(profile) = crate::tui::registry::registry().find(id) else { continue };
        assert_eq!(prepare_in(profile.as_ref(), "pane-6", &[], &server("pane-6"), &d), PaneMcpLaunch::default(), "{id}");
    }
}

const ACME: &str = r#"
schema_version = 1
[agent]
id = "acme"
label = "Acme"
[binary]
names = ["acme"]
[tui]
argv = ["acme"]
[pane_mcp]
shape = "mcp_servers"
args = ["--mcp", "{config_file}"]
[pane_mcp.env]
ACME_NOTE = "{note_file}"
"#;

#[test]
fn a_toml_agent_with_a_pane_mcp_table_gets_it_rendered() {
    let profile = crate::tui::declarative::build_for_test(ACME).expect("a valid definition");
    let d = dir("toml");
    let launch = prepare_in(&profile, "pane-8", &[], &server("pane-8"), &d);
    let want = format!("{}/pane-8.mcp.json", crate::app_paths::path_for_host_config(&d));
    assert_eq!(arg_after(&launch.args, "--mcp"), Some(want.as_str()));
    assert_eq!(launch.env[0].0, "ACME_NOTE");
    assert!(d.join("pane-8.mcp.json").is_file() && d.join("helpers-note.md").is_file());
}

#[test]
fn a_toml_agent_with_a_bad_pane_mcp_table_is_refused_when_it_loads() {
    let bad = ACME.replace("{config_file}", "{secret}");
    let error = crate::tui::declarative::build_for_test(&bad).err().expect("refused");
    assert!(error.contains("AGENT_DEF_PANE_MCP_INVALID"), "{error}");
}

#[test]
fn a_file_outside_the_folder_is_never_written() {
    let d = dir("escape");
    let file = PaneMcpFile { path: d.join("..").join("evil.json"), content: "x".into() };
    assert!(write_files(&d, &[file]).is_err());
}

#[test]
fn the_switch_defaults_on_reads_the_setting_and_turns_every_agent_off() {
    static LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());
    let _guard = LOCK.lock();
    apply_settings(&json!({}));
    assert!(enabled(), "on unless the person turns it off");
    apply_settings(&json!({ "pane_mcp_enabled": false }));
    assert!(!enabled());
    assert_eq!(prepare(profile_for("claude").as_ref(), "p", &[]), PaneMcpLaunch::default());
    apply_settings(&json!({ "pane_mcp_enabled": true }));
    assert!(enabled());
}

#[test]
fn the_default_settings_turn_it_on_and_the_key_can_be_patched() {
    let defaults = crate::settings_store::default_public_settings();
    assert_eq!(defaults["pane_mcp_enabled"], true);
    let merged = crate::settings_store::merge_public_settings(&defaults, &json!({ "pane_mcp_enabled": false }));
    assert_eq!(merged["pane_mcp_enabled"], false);
}

#[test]
fn a_dev_run_with_no_mcp_server_built_adds_nothing() {
    // The test build has no bundled server: no pane is given an entry that fails at start.
    assert!(server_for("p").is_none());
}

#[test]
fn old_pane_files_are_pruned_and_new_ones_stay() {
    let d = dir("prune");
    std::fs::create_dir_all(&d).unwrap();
    let old = d.join("old.mcp.json");
    let fresh = d.join("fresh.mcp.json");
    let other = d.join("keep.txt");
    for f in [&old, &fresh, &other] {
        std::fs::write(f, "x").unwrap();
    }
    let long_ago = SystemTime::now() - Duration::from_secs(10 * 24 * 3600);
    std::fs::File::options().write(true).open(&old).unwrap().set_modified(long_ago).unwrap();
    std::fs::File::options().write(true).open(&other).unwrap().set_modified(long_ago).unwrap();
    prune(&d);
    assert!(!old.exists() && fresh.exists() && other.exists(), "only stale *.mcp.json files go");
}
