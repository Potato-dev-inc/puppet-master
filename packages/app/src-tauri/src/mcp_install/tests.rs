use super::*;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_dir(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir().join(format!("pm-mcp-{name}-{nonce}"))
}

#[test]
fn claude_install_writes_mcp_json_and_approves_project_servers() {
    let cwd = temp_dir("claude");
    fs::create_dir_all(&cwd).expect("cwd");

    let result = ensure_claude_mcp(&cwd, McpLaunchSource::Auto).expect("install");
    assert!(result.installed);
    assert!(result.changed);
    assert!(cwd.join(".mcp.json").is_file());
    assert!(cwd.join(".claude").join("settings.json").is_file());

    let again = ensure_claude_mcp(&cwd, McpLaunchSource::Auto).expect("reinstall");
    assert!(again.installed);
    assert!(!again.changed);
}

#[test]
fn codex_install_appends_project_config_toml() {
    let cwd = temp_dir("codex");
    fs::create_dir_all(&cwd).expect("cwd");

    let result = ensure_codex_mcp(&cwd, McpLaunchSource::Auto).expect("install");
    assert!(result.installed);
    assert!(result.changed);

    let content = fs::read_to_string(cwd.join(".codex").join("config.toml")).expect("toml");
    assert!(codex_has_puppet_master(&content));
}

#[test]
fn opencode_install_writes_project_config() {
    let cwd = temp_dir("opencode");
    fs::create_dir_all(&cwd).expect("cwd");

    let result = ensure_opencode_mcp(&cwd, McpLaunchSource::Auto).expect("install");
    assert!(result.installed);
    assert!(result.changed);
    assert!(opencode_mcp_installed_path(
        &cwd.join("opencode.json"),
        McpLaunchSource::Auto
    ));
}

#[test]
fn npm_install_rewrites_all_backends_to_npx_package() {
    let cwd = temp_dir("npm");
    fs::create_dir_all(&cwd).expect("cwd");
    write_json_pretty(
        &cwd.join(".mcp.json"),
        &json!({
            "mcpServers": {
                MCP_SERVER_NAME: {
                    "command": "/usr/local/bin/node",
                    "args": ["/tmp/mcp-stdio.bundle.cjs"]
                }
            }
        }),
    )
    .expect("claude seed");
    fs::create_dir_all(cwd.join(".codex")).expect("codex dir");
    fs::write(
        cwd.join(".codex").join("config.toml"),
        format!(
            "\n[mcp_servers.{MCP_SERVER_NAME}]\ncommand = \"/usr/local/bin/node\"\nargs = [\"/tmp/mcp-stdio.bundle.cjs\"]\nenabled = true\n"
        ),
    )
    .expect("codex seed");
    write_json_pretty(
        &cwd.join("opencode.json"),
        &json!({
            "mcp": {
                MCP_SERVER_NAME: {
                    "type": "local",
                    "command": ["/usr/local/bin/node", "/tmp/mcp-stdio.bundle.cjs"],
                    "enabled": true
                }
            }
        }),
    )
    .expect("opencode seed");

    let results = install_npm_mcp_configs(&cwd).expect("install all");
    assert_eq!(results.len(), 3);
    assert!(results.iter().all(|result| result.installed));

    let claude = read_json_safe(&cwd.join(".mcp.json")).expect("claude");
    assert!(json_server_uses_npm_package(
        claude
            .pointer(&format!("/mcpServers/{MCP_SERVER_NAME}"))
            .expect("server")
    ));

    let codex = fs::read_to_string(cwd.join(".codex").join("config.toml")).expect("codex");
    assert!(codex.contains("command = \"npx\""));
    assert!(codex.contains("@puppet-master/mcp"));
    assert!(!codex.contains("mcp-stdio.bundle.cjs"));

    let opencode = read_json_safe(&cwd.join("opencode.json")).expect("opencode");
    assert!(opencode_entry_uses_npm_package(
        opencode
            .pointer(&format!("/mcp/{MCP_SERVER_NAME}"))
            .expect("entry")
    ));
}

#[test]
fn rust_preference_writes_auto_runtime_when_available() {
    let cwd = temp_dir("rust-preference");
    fs::create_dir_all(&cwd).expect("cwd");

    let results = install_mcp_configs_with_preference(&cwd, true).expect("install all");
    assert_eq!(results.len(), 3);
    assert!(results.iter().all(|result| result.installed));

    let codex = fs::read_to_string(cwd.join(".codex").join("config.toml")).expect("codex");
    assert!(codex.contains("[mcp_servers.puppet-master]"));
    assert!(codex.contains(crate::app_paths::BRIDGE_PORT_FILE_ENV));
}

#[test]
fn uninstall_removes_all_project_backends() {
    let cwd = temp_dir("uninstall");
    fs::create_dir_all(&cwd).expect("cwd");
    install_npm_mcp_configs(&cwd).expect("install");

    let results = uninstall_npm_mcp_configs(&cwd).expect("uninstall");
    assert_eq!(results.len(), 3);
    assert!(results.iter().all(|result| result.changed));
    assert!(results.iter().all(|result| !result.installed));
    assert!(!claude_mcp_installed(&cwd));
    let codex = fs::read_to_string(cwd.join(".codex").join("config.toml")).unwrap_or_default();
    assert!(!codex_has_puppet_master(&codex));
    assert!(!opencode_mcp_installed_path(
        &cwd.join("opencode.json"),
        McpLaunchSource::NpmPackage
    ));
}

#[test]
fn global_uninstall_removes_home_mcp_json() {
    let home = temp_dir("home-mcp");
    fs::create_dir_all(&home).expect("home");
    let mut doc = json!({ "mcpServers": {} });
    merge_mcp_servers(&mut doc, McpLaunchSource::NpmPackage);
    write_json_pretty(&home.join(".mcp.json"), &doc).expect("write");

    let result = uninstall_claude_mcp(&home).expect("uninstall home mcp");
    assert!(result.changed);
    assert!(!result.installed);
    assert!(!home.join(".mcp.json").is_file());
}

#[test]
fn claude_mcp_remove_not_found_matches_cli_errors() {
    assert!(claude_mcp_remove_not_found(
        "No user-scoped MCP server found with name: puppet-master"
    ));
    assert!(claude_mcp_remove_not_found(
        "No project-local MCP server found with name: puppet-master"
    ));
    assert!(!claude_mcp_remove_not_found(
        "could not run claude: No such file"
    ));
}

#[test]
fn all_cli_backends_reject_root_project_path() {
    for backend in ["claude_cli", "codex_cli", "opencode_cli"] {
        let err =
            ensure_orchestrator_mcp_with_preference(backend, Path::new("/"), false).unwrap_err();
        assert!(
            err.contains("Pick a project folder"),
            "backend {backend}: {err}"
        );
    }
}

mod declarative {
    use super::*;
    use crate::mcp_install::declarative::{write_host_entry, HostLaunch};
    use crate::tui::declarative::schema::McpHostTable;
    use crate::tui::declarative::{parse_definition, validate, Source, ValidationContext};

    fn launch() -> HostLaunch {
        HostLaunch {
            command: "pm-mcp".into(),
            args: vec!["--serve".into(), "--x".into()],
            env: vec![("PM_PORT_FILE".into(), "C:/p/port".into())],
            ..Default::default()
        }
    }

    fn table(path: &str, format: &str, key: &str) -> McpHostTable {
        let text = format!(
            "config_path = {path:?}\nformat = \"{format}\"\nkey_path = \"{key}\"\n[entry]\ncommand = \"{{command}}\"\nargs = \"{{args}}\"\nenabled = true\n[entry.env]\nPM_PORT_FILE = \"C:/p/port\"\n"
        );
        toml::from_str(&text).expect("table")
    }

    fn home(name: &str) -> PathBuf {
        let d = temp_dir(name);
        fs::create_dir_all(&d).expect("home");
        d
    }

    #[test]
    fn json_toml_yaml_writers() {
        let h = home("decl-formats");
        let p = Some(h.as_path());

        // JSON: other keys kept, nested key path created, args array substituted.
        fs::write(h.join("host.json"), "{\"other\": 1, \"mcpServers\": {\"a\": {\"x\": 1}}}").unwrap();
        let t = table("~/host.json", "json", "mcpServers.puppet-master");
        let r = write_host_entry(&t, &launch(), &h, p, true).expect("json");
        assert!(r.changed && r.backup.is_some());
        let doc: Value = serde_json::from_str(&fs::read_to_string(h.join("host.json")).unwrap()).unwrap();
        assert_eq!(doc["other"], 1);
        assert_eq!(doc["mcpServers"]["a"]["x"], 1);
        assert_eq!(doc["mcpServers"]["puppet-master"]["command"], "pm-mcp");
        assert_eq!(doc["mcpServers"]["puppet-master"]["args"], json!(["--serve", "--x"]));
        assert_eq!(doc["mcpServers"]["puppet-master"]["enabled"], true);

        // TOML: comments and other tables kept, old sub-table replaced.
        fs::write(
            h.join("host.toml"),
            "# keep me\n[general]\nname = \"x\"\n\n[mcp_servers.puppet-master]\ncommand = \"old\"\n\n[mcp_servers.puppet-master.env]\nOLD = \"1\"\n\n[mcp_servers.other]\ncommand = \"o\"\n",
        )
        .unwrap();
        let t = table("~/host.toml", "toml", "mcp_servers.puppet-master");
        let r = write_host_entry(&t, &launch(), &h, p, true).expect("toml");
        assert!(r.changed);
        let text = fs::read_to_string(h.join("host.toml")).unwrap();
        assert!(text.contains("# keep me") && text.contains("[mcp_servers.other]"), "{text}");
        let doc: toml::Table = text.parse().unwrap();
        let pm = &doc["mcp_servers"]["puppet-master"];
        assert_eq!(pm["command"].as_str(), Some("pm-mcp"));
        assert_eq!(pm["args"].as_array().unwrap().len(), 2);
        assert!(pm["env"].get("OLD").is_none());
        assert_eq!(doc["mcp_servers"]["other"]["command"].as_str(), Some("o"));

        // YAML: existing parent gets the key; comments and siblings kept.
        fs::write(h.join("host.yaml"), "# top\nname: demo\nservers:\n  other:\n    a: 1\nlast: 2\n").unwrap();
        let t = table("~/host.yaml", "yaml", "servers.puppet-master");
        let r = write_host_entry(&t, &launch(), &h, p, true).expect("yaml");
        assert!(r.changed);
        let text = fs::read_to_string(h.join("host.yaml")).unwrap();
        assert!(text.starts_with("# top\nname: demo\nservers:\n  other:\n    a: 1\n  puppet-master: {"), "{text}");
        assert!(text.trim_end().ends_with("last: 2"), "{text}");
        assert!(text.contains("\"command\":\"pm-mcp\""));

        // YAML: missing parents are created; a new file gets no .bak.
        let t = table("~/new/deep.yaml", "yaml", "a.b.c");
        let r = write_host_entry(&t, &launch(), &h, p, true).expect("yaml new");
        assert!(r.changed && r.backup.is_none());
        assert!(fs::read_to_string(h.join("new/deep.yaml")).unwrap().starts_with("a:\n  b:\n    c: {"));
    }

    #[test]
    fn yaml_replaces_existing_entry_in_place() {
        let h = home("decl-yaml-replace");
        fs::write(h.join("c.yml"), "servers:\n  puppet-master: {\"command\":\"old\"}\n  zed: 1\n").unwrap();
        let t = table("~/c.yml", "yaml", "servers.puppet-master");
        write_host_entry(&t, &launch(), &h, Some(h.as_path()), true).unwrap();
        let text = fs::read_to_string(h.join("c.yml")).unwrap();
        assert!(text.contains("\"command\":\"pm-mcp\"") && !text.contains("old") && text.contains("  zed: 1"), "{text}");
    }

    #[test]
    fn refuses_outside_home() {
        let h = home("decl-home");
        let outside = home("decl-outside");
        let abs = outside.join("evil.json").to_string_lossy().to_string();
        for path in [abs.as_str(), "~/../evil.json", "../evil.json"] {
            let t = table(path, "json", "mcpServers.puppet-master");
            let err = write_host_entry(&t, &launch(), &h, Some(h.as_path()), true).unwrap_err();
            assert!(err.contains("HST-3") || err.contains("outside"), "{path}: {err}");
        }
        assert!(!outside.join("evil.json").exists());
        // The project directory is allowed even when it is not under home.
        let t = table(&abs, "json", "mcpServers.puppet-master");
        write_host_entry(&t, &launch(), &h, Some(outside.as_path()), true).expect("project ok");
        assert!(outside.join("evil.json").is_file());
        // Unapproved definitions never write.
        let t = table("~/n.json", "json", "a.b");
        assert!(write_host_entry(&t, &launch(), &h, None, false).unwrap_err().contains("not approved"));
        assert!(!h.join("n.json").exists());
    }

    #[test]
    fn idempotent() {
        let h = home("decl-idem");
        for (file, format, key) in [("a.json", "json", "mcpServers.pm"), ("a.toml", "toml", "mcp_servers.pm"), ("a.yaml", "yaml", "s.pm")] {
            let t = table(&format!("~/{file}"), format, key);
            let first = write_host_entry(&t, &launch(), &h, None, true).unwrap();
            assert!(first.changed, "{file}");
            let bytes = fs::read(h.join(file)).unwrap();
            let second = write_host_entry(&t, &launch(), &h, None, true).unwrap();
            assert!(!second.changed && second.backup.is_none(), "{file}");
            assert_eq!(bytes, fs::read(h.join(file)).unwrap(), "{file}");
        }
    }

    #[test]
    fn backup_has_original_and_invalid_files_are_untouched() {
        let h = home("decl-bak");
        fs::write(h.join("o.json"), "{\"keep\":true}").unwrap();
        let t = table("~/o.json", "json", "x.y");
        let r = write_host_entry(&t, &launch(), &h, None, true).unwrap();
        assert_eq!(fs::read_to_string(r.backup.unwrap()).unwrap(), "{\"keep\":true}");
        fs::write(h.join("bad.json"), "{nope").unwrap();
        let t = table("~/bad.json", "json", "x.y");
        assert!(write_host_entry(&t, &launch(), &h, None, true).is_err());
        assert_eq!(fs::read_to_string(h.join("bad.json")).unwrap(), "{nope");
    }

    const FAKE: &str = r##"schema_version = 1
[agent]
id = "fakehost"
label = "Fake Host"
[binary]
names = ["fakehost"]
[features]
dispatchable = false
not_dispatchable_reason = "test"
[mcp_host]
config_path = "~/.fakehost/mcp.json"
format = "json"
key_path = "mcpServers.puppet-master"
[mcp_host.entry]
command = "{command}"
args = "{args}"
"##;

    fn codes(text: &str) -> Vec<(&'static str, String)> {
        let def = parse_definition(text).expect("parse");
        let ctx = ValidationContext { source: Source::User, code_ids: &["claude"], raw_len: text.len() };
        validate(&def, &ctx).into_iter().map(|e| (e.code, e.path)).collect()
    }

    #[test]
    fn validates_mcp_host_table() {
        assert!(codes(FAKE).iter().all(|(_, p)| !p.starts_with("mcp_host")), "{:?}", codes(FAKE));
        let bad = FAKE.replace("mcpServers.puppet-master", "bad key");
        assert!(codes(&bad).iter().any(|(_, p)| p == "mcp_host.key_path"));
        let bad = FAKE.replace("~/.fakehost/mcp.json", "~/../x.json");
        assert!(codes(&bad).iter().any(|(_, p)| p == "mcp_host.config_path"));
        let bad = FAKE.replace("{command}", "{nope}");
        assert!(codes(&bad).iter().any(|(c, p)| *c == "AGENT_DEF_BAD_PLACEHOLDER" && p == "mcp_host.entry"));
        let bad = FAKE.replace("\"{args}\"", "\"x {args}\"");
        assert!(codes(&bad).iter().any(|(c, _)| *c == "AGENT_DEF_BAD_PLACEHOLDER"));
    }
}

mod toolbelt_declaration {
    use crate::tui::declarative::{parse_definition, validate, Source, ValidationContext};

    const DEF: &str = r##"schema_version = 1
[agent]
id = "fakehost"
label = "Fake Host"
[binary]
names = ["fakehost"]
[features]
dispatchable = false
not_dispatchable_reason = "test"
[mcp_host]
config_path = "~/.fakehost/mcp.json"
format = "json"
key_path = "mcpServers.puppet-master"
project_config_path = ".fakehost/mcp.json"
[mcp_host.entry]
command = "{command}"
args = "{args}"
[mcp_host.entry_http]
url = "{url}"
headers = "{headers}"
"##;

    fn bad_paths(text: &str) -> Vec<String> {
        let def = parse_definition(text).expect("parse");
        let ctx = ValidationContext { source: Source::User, code_ids: &["claude"], raw_len: text.len() };
        validate(&def, &ctx).into_iter().filter(|e| e.path.starts_with("mcp_host")).map(|e| e.path).collect()
    }

    #[test]
    fn toolbelt_fields_validate() {
        assert!(bad_paths(DEF).is_empty(), "{:?}", bad_paths(DEF));
        assert!(bad_paths(&DEF.replace("\".fakehost/mcp.json\"", "\"../x.json\"")).contains(&"mcp_host.project_config_path".to_string()));
        assert!(bad_paths(&DEF.replace("\".fakehost/mcp.json\"", "\"~/x.json\"")).contains(&"mcp_host.project_config_path".to_string()));
        assert!(bad_paths(&DEF.replace("{url}", "{nope}")).contains(&"mcp_host.entry_http".to_string()));
        assert!(bad_paths(&DEF.replace("\"{headers}\"", "\"x {headers}\"")).contains(&"mcp_host.entry_http".to_string()));
    }

    #[test]
    fn a_declarative_agent_becomes_a_toolbelt_host_with_no_code() {
        let def = parse_definition(DEF).unwrap();
        let table = def.mcp_host.expect("table");
        assert_eq!(table.project_config_path.as_deref(), Some(".fakehost/mcp.json"));
        assert!(table.entry_http.is_some());
    }
}
