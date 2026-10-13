//! Toolbelt tests: temp config files in a JSON-style host, a TOML-style host, an array-command host
//! (OpenCode-shaped) and a YAML host. No agent names are involved; hosts are declared as data.

use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use super::edit::{self, Format, Op};
use super::import;
use super::service::{self, Env};
use super::store::{self, Scope, ToolServer, Transport, MASK};
use super::sync::{self, ChangeKind, HostSpec, SyncOptions};
use super::{host_from_toml, KEY_PREFIX};
use crate::mcp_install::declarative::HostLaunch;

const JSON_HOST: &str = r#"
config_path = "~/.alpha/mcp.json"
format = "json"
key_path = "mcpServers.puppet-master"
project_config_path = ".alpha/mcp.json"
[entry]
command = "{command}"
args = "{args}"
env = "{env}"
[entry_http]
type = "http"
url = "{url}"
headers = "{headers}"
"#;

const TOML_HOST: &str = r#"
config_path = "~/.beta/config.toml"
format = "toml"
key_path = "mcp_servers.puppet-master"
project_config_path = ".beta/config.toml"
[entry]
command = "{command}"
args = "{args}"
env = "{env}"
[entry_http]
url = "{url}"
http_headers = "{headers}"
"#;

const ARRAY_HOST: &str = r#"
config_path = "~/.gamma/gamma.json"
format = "json"
key_path = "mcp.puppet-master"
[entry]
type = "local"
command = ["{command}", "{args}"]
environment = "{env}"
enabled = true
[entry_http]
type = "remote"
url = "{url}"
headers = "{headers}"
enabled = true
"#;

const YAML_HOST: &str = r#"
config_path = "~/.delta/config.yaml"
format = "yaml"
key_path = "servers.puppet-master"
[entry]
command = "{command}"
args = "{args}"
"#;

fn home(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let d = std::env::temp_dir().join(format!("pm-toolbelt-{name}-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&d).unwrap();
    d
}

fn host(id: &str, toml_text: &str, installed: bool) -> HostSpec {
    HostSpec { agent_id: id.into(), label: id.to_uppercase(), installed, table: host_from_toml(toml_text).expect("host parses") }
}

fn stdio(id: &str, name: &str, command: &str) -> ToolServer {
    ToolServer {
        id: id.into(),
        name: name.into(),
        transport: Transport::Stdio,
        command: Some(command.into()),
        args: vec!["-y".into(), "pkg".into()],
        env: BTreeMap::new(),
        url: None,
        headers: BTreeMap::new(),
        enabled: true,
        scope: Scope::Global,
    }
}

fn http(id: &str, name: &str, url: &str) -> ToolServer {
    ToolServer { transport: Transport::Http, command: None, args: vec![], url: Some(url.into()), ..stdio(id, name, "") }
}

fn opts(h: &PathBuf) -> SyncOptions {
    SyncOptions { home: h.clone(), project: None, pm_launch: None }
}

fn read(p: &PathBuf) -> String {
    fs::read_to_string(p).unwrap()
}

const USER_JSON: &str = "{\n  \"theme\": \"dark\",\n  \"mcpServers\": {\n    \"mine\": {\n      \"command\": \"my-server\"\n    }\n  },\n  \"zzz\": [1, 2]\n}\n";
const USER_TOML: &str = "# my config\nmodel = \"x\"\n\n[mcp_servers.mine]\ncommand = \"mine\"\n\n[ui]\nverbose = true\n";

fn two_hosts(h: &PathBuf) -> (Vec<HostSpec>, PathBuf, PathBuf) {
    fs::create_dir_all(h.join(".alpha")).unwrap();
    fs::create_dir_all(h.join(".beta")).unwrap();
    let (a, b) = (h.join(".alpha/mcp.json"), h.join(".beta/config.toml"));
    fs::write(&a, USER_JSON).unwrap();
    fs::write(&b, USER_TOML).unwrap();
    (vec![host("alpha", JSON_HOST, true), host("beta", TOML_HOST, true)], a, b)
}

#[test]
fn sync_adds_entries_in_json_and_toml_and_keeps_the_users_bytes() {
    let h = home("add");
    let (hosts, a, b) = two_hosts(&h);
    let servers = vec![stdio("browser", "Browser", "npx"), http("docs", "Docs", "https://docs.example/mcp")];
    let report = sync::run(&servers, &hosts, vec![], &opts(&h), true);
    assert_eq!(report.hosts.len(), 2);
    assert!(report.hosts.iter().all(|r| r.status == "ready" && r.written), "{:?}", report.hosts.iter().map(|r| &r.reason).collect::<Vec<_>>());

    let json_doc: Value = serde_json::from_str(&read(&a)).unwrap();
    assert_eq!(json_doc["mcpServers"]["mine"]["command"], "my-server");
    assert_eq!(json_doc["mcpServers"]["pm-toolbelt-browser"]["command"], "npx");
    assert_eq!(json_doc["mcpServers"]["pm-toolbelt-browser"]["args"], json!(["-y", "pkg"]));
    assert_eq!(json_doc["mcpServers"]["pm-toolbelt-docs"], json!({"type": "http", "url": "https://docs.example/mcp", "headers": {}}));
    // Key order of the user's file is kept (a re-serialise would sort `mcpServers` before `theme`).
    assert!(read(&a).starts_with("{\n  \"theme\": \"dark\",\n  \"mcpServers\": {\n    \"mine\": {"));

    let toml_text = read(&b);
    assert!(toml_text.starts_with(USER_TOML.trim_end()), "{toml_text}");
    let doc: toml::Table = toml_text.parse().unwrap();
    assert_eq!(doc["mcp_servers"]["pm-toolbelt-browser"]["command"].as_str(), Some("npx"));
    assert_eq!(doc["mcp_servers"]["pm-toolbelt-docs"]["url"].as_str(), Some("https://docs.example/mcp"));
    assert_eq!(doc["mcp_servers"]["mine"]["command"].as_str(), Some("mine"));

    // Byte-level: removing every managed entry gives back exactly the user's original file.
    let none: Vec<ToolServer> = vec![];
    let report = sync::run(&none, &hosts, vec![], &opts(&h), true);
    assert!(report.hosts.iter().all(|r| r.changes.iter().all(|c| c.kind == ChangeKind::Remove)));
    assert_eq!(read(&a), USER_JSON);
    assert_eq!(read(&b), USER_TOML);
}

#[test]
fn second_sync_is_idempotent() {
    let h = home("idem");
    let (hosts, a, b) = two_hosts(&h);
    let servers = vec![stdio("browser", "Browser", "npx")];
    sync::run(&servers, &hosts, vec![], &opts(&h), true);
    let (ja, jb) = (fs::read(&a).unwrap(), fs::read(&b).unwrap());
    let again = sync::run(&servers, &hosts, vec![], &opts(&h), true);
    assert!(again.hosts.iter().all(|r| r.status == "unchanged" && !r.written && r.backup.is_none()));
    assert!(again.hosts.iter().all(|r| r.changes.iter().all(|c| c.kind == ChangeKind::Unchanged)));
    assert_eq!(fs::read(&a).unwrap(), ja);
    assert_eq!(fs::read(&b).unwrap(), jb);
}

#[test]
fn update_changes_only_the_managed_entry_and_removal_follows_the_store() {
    let h = home("upd");
    let (hosts, a, _b) = two_hosts(&h);
    let mut s = stdio("browser", "Browser", "npx");
    sync::run(&[s.clone()], &hosts, vec![], &opts(&h), true);
    let before = read(&a);
    s.args = vec!["-y".into(), "other".into()];
    let r = sync::run(&[s.clone()], &hosts, vec![], &opts(&h), true);
    assert_eq!(r.hosts[0].changes[0].kind, ChangeKind::Update);
    let after = read(&a);
    assert!(after.contains("\"other\"") && !after.contains("\"pkg\""));
    // Everything outside the managed member is the same bytes.
    let cut = |t: &str| t.replace(&t[t.find("\"pm-toolbelt-browser\"").unwrap()..t.rfind("}\n  },").unwrap()], "");
    assert_eq!(cut(&before), cut(&after));

    // Disabled counts as removed.
    s.enabled = false;
    let r = sync::run(&[s], &hosts, vec![], &opts(&h), true);
    assert!(r.hosts.iter().all(|x| x.changes.iter().all(|c| c.kind == ChangeKind::Remove)));
    assert_eq!(read(&a), USER_JSON);
}

#[test]
fn first_write_makes_a_bak_and_later_writes_keep_it() {
    let h = home("bak");
    let (hosts, a, b) = two_hosts(&h);
    let r = sync::run(&[stdio("one", "One", "npx")], &hosts, vec![], &opts(&h), true);
    assert!(r.hosts.iter().all(|x| x.backup.is_some()));
    assert_eq!(read(&PathBuf::from(format!("{}.bak", a.display()))), USER_JSON);
    assert_eq!(read(&PathBuf::from(format!("{}.bak", b.display()))), USER_TOML);
    sync::run(&[stdio("one", "One", "npx"), stdio("two", "Two", "npx")], &hosts, vec![], &opts(&h), true);
    assert_eq!(read(&PathBuf::from(format!("{}.bak", a.display()))), USER_JSON, "the .bak keeps the file as it was before the first write");

    // A config that did not exist gets no .bak.
    let h2 = home("bak-new");
    let r = sync::run(&[stdio("one", "One", "npx")], &[host("alpha", JSON_HOST, true)], vec![], &opts(&h2), true);
    assert!(r.hosts[0].written && r.hosts[0].backup.is_none());
    assert!(!h2.join(".alpha/mcp.json.bak").exists());
}

#[test]
fn dry_run_writes_nothing_and_shows_a_diff_without_secrets() {
    let h = home("dry");
    let (hosts, a, b) = two_hosts(&h);
    let mut s = stdio("db", "Database", "npx");
    s.env.insert("DB_PASSWORD".into(), "hunter2-secret".into());
    s.env.insert("DB_HOST".into(), "localhost".into());
    let r = sync::run(&[s], &hosts, vec![], &opts(&h), false);
    assert!(r.dry_run);
    assert_eq!(read(&a), USER_JSON);
    assert_eq!(read(&b), USER_TOML);
    assert!(!h.join(".alpha/mcp.json.bak").exists());
    for host in &r.hosts {
        assert_eq!(host.status, "ready");
        assert!(!host.written);
        assert!(host.diff.lines().any(|l| l.starts_with("+ ")), "{}", host.diff);
        assert!(host.diff.contains("pm-toolbelt-db"));
        assert!(!host.diff.contains("hunter2-secret"), "secret leaked into preview: {}", host.diff);
        assert!(host.diff.contains(MASK));
        assert!(host.diff.contains("localhost"), "non-secret values are shown");
    }
    // The real value does reach the agent's file on apply.
    let s2 = {
        let mut s = stdio("db", "Database", "npx");
        s.env.insert("DB_PASSWORD".into(), "hunter2-secret".into());
        s
    };
    sync::run(&[s2], &hosts, vec![], &opts(&h), true);
    assert!(read(&a).contains("hunter2-secret"));
}

#[test]
fn array_command_and_yaml_hosts_work() {
    let h = home("arr");
    fs::create_dir_all(h.join(".gamma")).unwrap();
    fs::create_dir_all(h.join(".delta")).unwrap();
    fs::write(h.join(".gamma/gamma.json"), "{\"mcp\": {\"keep\": {\"type\": \"local\", \"command\": [\"k\"]}}}").unwrap();
    fs::write(h.join(".delta/config.yaml"), "# top\nname: demo\nservers:\n  other: {\"command\":\"o\"}\nlast: 2\n").unwrap();
    let hosts = vec![host("gamma", ARRAY_HOST, true), host("delta", YAML_HOST, true)];
    let servers = vec![stdio("browser", "Browser", "npx"), http("docs", "Docs", "https://d.example/mcp")];
    let r = sync::run(&servers, &hosts, vec![], &opts(&h), true);
    let gamma: Value = serde_json::from_str(&read(&h.join(".gamma/gamma.json"))).unwrap();
    assert_eq!(gamma["mcp"]["pm-toolbelt-browser"]["command"], json!(["npx", "-y", "pkg"]));
    assert_eq!(gamma["mcp"]["pm-toolbelt-browser"]["type"], "local");
    assert_eq!(gamma["mcp"]["pm-toolbelt-docs"]["type"], "remote");
    assert_eq!(gamma["mcp"]["keep"]["command"], json!(["k"]));
    // The YAML host has no URL template: the URL server is skipped with a plain note.
    let yaml = read(&h.join(".delta/config.yaml"));
    assert!(yaml.starts_with("# top\nname: demo\nservers:\n  other: {\"command\":\"o\"}\n  pm-toolbelt-browser: {"), "{yaml}");
    assert!(yaml.trim_end().ends_with("last: 2"));
    let delta = r.hosts.iter().find(|x| x.agents[0].id == "delta").unwrap();
    assert!(delta.notes.iter().any(|n| n.contains("Docs was skipped")), "{:?}", delta.notes);
    // Idempotent and removable.
    let again = sync::run(&servers, &hosts, vec![], &opts(&h), true);
    assert!(again.hosts.iter().all(|x| x.status == "unchanged"), "{:?}", again.hosts.iter().map(|x| (&x.status, &x.reason)).collect::<Vec<_>>());
    sync::run(&[], &hosts, vec![], &opts(&h), true);
    assert_eq!(read(&h.join(".delta/config.yaml")), "# top\nname: demo\nservers:\n  other: {\"command\":\"o\"}\nlast: 2\n");
    let gamma: Value = serde_json::from_str(&read(&h.join(".gamma/gamma.json"))).unwrap();
    assert!(gamma["mcp"].get("pm-toolbelt-browser").is_none() && gamma["mcp"]["keep"].is_object());
}

#[test]
fn project_scope_goes_to_the_project_file_only() {
    let h = home("scope");
    let project = h.join("work/app");
    fs::create_dir_all(&project).unwrap();
    let hosts = vec![host("alpha", JSON_HOST, true)];
    let mut p = stdio("local", "Local", "npx");
    p.scope = Scope::Project;
    let g = stdio("shared", "Shared", "npx");
    // Without a project, the project server is not written anywhere.
    sync::run(&[p.clone(), g.clone()], &hosts, vec![], &opts(&h), true);
    let global: Value = serde_json::from_str(&read(&h.join(".alpha/mcp.json"))).unwrap();
    assert!(global["mcpServers"].get("pm-toolbelt-local").is_none() && global["mcpServers"].get("pm-toolbelt-shared").is_some());
    let with_project = SyncOptions { home: h.clone(), project: Some(project.clone()), pm_launch: None };
    let r = sync::run(&[p, g], &hosts, vec![], &with_project, true);
    assert_eq!(r.hosts.len(), 2);
    let proj: Value = serde_json::from_str(&read(&project.join(".alpha/mcp.json"))).unwrap();
    assert!(proj["mcpServers"].get("pm-toolbelt-local").is_some() && proj["mcpServers"].get("pm-toolbelt-shared").is_none());
}

#[test]
fn uninstalled_agent_without_a_config_is_skipped_and_pm_entry_is_optional() {
    let h = home("skip");
    let r = sync::run(&[stdio("a", "A", "npx")], &[host("alpha", JSON_HOST, false)], vec![], &opts(&h), true);
    assert_eq!(r.hosts[0].status, "skipped");
    assert!(!h.join(".alpha/mcp.json").exists());

    let launch = HostLaunch { command: "pm-mcp".into(), args: vec!["--serve".into()], ..Default::default() };
    let with_pm = SyncOptions { home: h.clone(), project: None, pm_launch: Some(launch) };
    sync::run(&[stdio("a", "A", "npx")], &[host("alpha", JSON_HOST, true)], vec![], &with_pm, true);
    let doc: Value = serde_json::from_str(&read(&h.join(".alpha/mcp.json"))).unwrap();
    assert_eq!(doc["mcpServers"]["puppet-master"]["command"], "pm-mcp");
    // Dropping the PM option never removes PM's entry (it is not a toolbelt-owned key).
    sync::run(&[], &[host("alpha", JSON_HOST, true)], vec![], &opts(&h), true);
    let doc: Value = serde_json::from_str(&read(&h.join(".alpha/mcp.json"))).unwrap();
    assert!(doc["mcpServers"].get("puppet-master").is_some() && doc["mcpServers"].get("pm-toolbelt-a").is_none());
}

#[test]
fn invalid_config_is_left_untouched() {
    let h = home("bad");
    fs::create_dir_all(h.join(".alpha")).unwrap();
    fs::write(h.join(".alpha/mcp.json"), "{nope").unwrap();
    let r = sync::run(&[stdio("a", "A", "npx")], &[host("alpha", JSON_HOST, true)], vec![], &opts(&h), true);
    assert_eq!(r.hosts[0].status, "error");
    assert_eq!(read(&h.join(".alpha/mcp.json")), "{nope");
}

#[test]
fn import_reads_servers_back_through_the_hosts_templates() {
    let h = home("imp");
    fs::create_dir_all(h.join(".alpha")).unwrap();
    fs::create_dir_all(h.join(".gamma")).unwrap();
    fs::write(
        h.join(".alpha/mcp.json"),
        r#"{"mcpServers": {"pg": {"command": "npx", "args": ["-y", "pg-mcp"], "env": {"PG_TOKEN": "tok-123", "PG_HOST": "db"}}, "web": {"type": "http", "url": "https://w.example/mcp", "headers": {"Authorization": "Bearer abc"}}, "puppet-master": {"command": "pm"}, "pm-toolbelt-old": {"command": "x"}}}"#,
    )
    .unwrap();
    fs::write(h.join(".gamma/gamma.json"), r#"{"mcp": {"pg2": {"type": "local", "command": ["npx", "-y", "pg-mcp"], "environment": {"PG_TOKEN": "tok-123", "PG_HOST": "db"}}}}"#).unwrap();
    let hosts = vec![host("alpha", JSON_HOST, true), host("gamma", ARRAY_HOST, true)];
    let found = import::preview(&hosts, &opts(&h), &[]);
    let names: Vec<&str> = found.iter().map(|c| c.server.name.as_str()).collect();
    assert_eq!(names, vec!["pg", "web"], "pg2 is the same server as pg; PM and toolbelt-owned entries are not offered");
    let pg = &found[0];
    assert_eq!(pg.agents.len(), 2, "both agents have it");
    assert_eq!(pg.server.command.as_deref(), Some("npx"));
    assert_eq!(pg.server.args, vec!["-y", "pg-mcp"]);
    assert_eq!(pg.server.env["PG_TOKEN"], MASK, "secrets are masked in the preview");
    assert_eq!(pg.server.env["PG_HOST"], "db");
    assert_eq!(found[1].server.transport, Transport::Http);
    assert_eq!(found[1].server.headers["Authorization"], MASK);

    // Apply reads the real values server side.
    let store_path = h.join("toolbelt.json");
    let added = import::apply(&hosts, &opts(&h), &store_path, &[pg.reference.clone(), found[1].reference.clone()]).unwrap();
    assert_eq!(added.len(), 2);
    let stored = store::load_servers(&store_path).unwrap();
    assert_eq!(stored[0].env["PG_TOKEN"], "tok-123");
    assert_eq!(stored[1].headers["Authorization"], "Bearer abc");
    assert_eq!(stored[0].id, "pg");
    // Now they are flagged as already in the toolbelt, and a second apply adds nothing.
    let again = import::preview(&hosts, &opts(&h), &stored);
    assert!(again.iter().all(|c| c.already));
    assert!(import::apply(&hosts, &opts(&h), &store_path, &[pg.reference.clone()]).unwrap().is_empty());
}

#[test]
fn store_validates_masks_and_keeps_secrets_on_edit() {
    let h = home("store");
    let p = h.join("toolbelt.json");
    assert!(store::add(&p, ToolServer { name: "".into(), ..stdio("", "", "npx") }).is_err());
    assert!(store::add(&p, ToolServer { command: None, ..stdio("", "No command", "") }).is_err());
    assert!(store::add(&p, http("", "Bad url", "ftp://x")).is_err());

    let mut s = stdio("", "My Browser!", "npx");
    s.env.insert("API_KEY".into(), "sekret-value".into());
    let added = store::add(&p, s.clone()).unwrap();
    assert_eq!(added.id, "my-browser");
    assert_eq!(added.env["API_KEY"], MASK);
    assert_eq!(store::add(&p, s).unwrap().id, "my-browser-2");
    assert!(read(&p).contains("sekret-value"), "stored as typed; see the module doc");
    assert_eq!(store::list(&p).unwrap()[0].env["API_KEY"], MASK);

    let mut edit_it = store::list(&p).unwrap().remove(0);
    edit_it.name = "Renamed".into();
    let updated = store::update(&p, "my-browser", edit_it).unwrap();
    assert_eq!(updated.name, "Renamed");
    assert_eq!(store::load_servers(&p).unwrap()[0].env["API_KEY"], "sekret-value", "******** kept the stored value");
    assert!(store::remove(&p, "my-browser").unwrap() && !store::remove(&p, "my-browser").unwrap());
    assert_eq!(store::load_servers(&p).unwrap().len(), 1);
}

#[test]
fn service_actions_round_trip() {
    let h = home("svc");
    let (hosts, a, _b) = two_hosts(&h);
    let env = Env { store_path: h.join("toolbelt.json"), hosts, unsupported: vec![], home: h.clone(), pm_launch: None };
    let added = service::call(&env, "add", &json!({"server": {"name": "Web", "transport": "http", "url": "https://x.example/mcp", "enabled": true, "scope": "global"}})).unwrap();
    assert_eq!(added["server"]["id"], "web");
    let preview = service::call(&env, "sync_preview", &json!({})).unwrap();
    assert_eq!(preview["dry_run"], true);
    assert_eq!(read(&a), USER_JSON);
    let applied = service::call(&env, "sync_apply", &json!({})).unwrap();
    assert_eq!(applied["hosts"][0]["written"], true);
    assert!(read(&a).contains("pm-toolbelt-web"));
    assert_eq!(service::call(&env, "remove", &json!({"id": "web"})).unwrap()["removed"], true);
    service::call(&env, "sync_apply", &json!({})).unwrap();
    assert_eq!(read(&a), USER_JSON);
    assert!(service::call(&env, "nope", &json!({})).is_err());
}

#[test]
fn edit_json_creates_missing_parents_and_removes_cleanly() {
    let parent = ["a", "b"];
    let set = |k: &str| Op::Set { key: k.into(), value: json!({"x": 1}) };
    let out = edit::apply(Format::Json, "{\"keep\": 1}", &parent, &[set("one")]).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&out).unwrap(), json!({"keep": 1, "a": {"b": {"one": {"x": 1}}}}));
    let out2 = edit::apply(Format::Json, &out, &parent, &[Op::Remove { key: "one".into() }]).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&out2).unwrap(), json!({"keep": 1, "a": {"b": {}}}));
    // New file.
    let fresh = edit::apply(Format::Json, "", &parent, &[set("one")]).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&fresh).unwrap(), json!({"a": {"b": {"one": {"x": 1}}}}));
    // Strings that look like JSON structure do not confuse the scanner.
    let tricky = "{\"a\": {\"b\": {\"q\": \"}\\\"{\"}}, \"z\": 1}";
    let out = edit::apply(Format::Json, tricky, &parent, &[set("one")]).unwrap();
    assert!(out.ends_with(", \"z\": 1}") && serde_json::from_str::<Value>(&out).unwrap()["a"]["b"]["q"] == "}\"{");
}

#[test]
fn edit_toml_replaces_in_place_without_moving_other_sections() {
    let text = "[x]\na = 1\n\n[m.pm-toolbelt-one]\ncommand = \"old\"\n\n[m.pm-toolbelt-one.env]\nK = \"v\"\n\n[m.mine]\ncommand = \"m\"\n";
    let out = edit::apply(Format::Toml, text, &["m"], &[Op::Set { key: "pm-toolbelt-one".into(), value: json!({"command": "new"}) }]).unwrap();
    assert!(out.starts_with("[x]\na = 1\n\n[m.pm-toolbelt-one]\ncommand = \"new\"\n\n[m.mine]"), "{out}");
    assert!(!out.contains("K = "), "{out}");
}

#[test]
fn builtin_agents_declare_hosts_and_shells_do_not() {
    let (hosts, unsupported) = sync::collect_hosts();
    let ids: Vec<&str> = hosts.iter().map(|h| h.agent_id.as_str()).collect();
    for expected in ["claude", "codex", "cursor_agent", "opencode"] {
        assert!(ids.contains(&expected), "{expected} should declare an MCP host: {ids:?}");
    }
    for h in &hosts {
        assert!(crate::mcp_install::declarative::key_segments(&h.table.key_path).is_ok(), "{}", h.agent_id);
        assert!(h.table.entry_http.is_some(), "{}", h.agent_id);
    }
    assert!(unsupported.iter().all(|u| !u.reason.is_empty()));
    assert!(!ids.contains(&"powershell") && !unsupported.iter().any(|u| u.id == "powershell"), "shells are not agents here");
    // Shared files collapse into one row: the two OpenCode profiles write the same file.
    let o = SyncOptions { home: home("groups"), project: None, pm_launch: None };
    let groups = sync::groups(&hosts, &o);
    assert!(groups.iter().any(|g| g.scope == Scope::Global && g.agents.len() == 2), "opencode and opencode_native share a file");
    let _ = KEY_PREFIX;
}
