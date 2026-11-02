//! Model Bridge tests. Every file goes to a temp folder; the real Codex settings and the real credential store are
//! never touched (the key store is a memory one).

use super::adapter::{self, Adapter, Route, Shape};
use super::recipe::{self, NEEDS_TRANSLATOR};
use super::*;
use crate::secrets::MemoryBackend;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const KEY: &str = "sk-test-SECRET-1234567890abcdef";

/// The Labs switch is one global; tests that read or set it take turns.
static FLAG: Mutex<()> = Mutex::new(());

struct Temp(PathBuf);
impl Temp {
    fn new(name: &str) -> Self {
        let p = std::env::temp_dir().join(format!("pm-model-bridge-{name}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&p).unwrap();
        Temp(p)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Rig {
    data: Temp,
    codex_home: Temp,
    secrets: MemoryBackend,
}

impl Rig {
    fn new(name: &str) -> Self {
        Rig { data: Temp::new(&format!("{name}-data")), codex_home: Temp::new(&format!("{name}-codex")), secrets: MemoryBackend::new() }
    }
    fn config(&self) -> PathBuf {
        self.codex_home.path().join("config.toml")
    }
    fn with<T>(&self, f: impl FnOnce(&Deps) -> T) -> T {
        let home = self.codex_home.path().to_string_lossy().into_owned();
        let env = move |n: &str| (n == "CODEX_HOME").then(|| home.clone());
        let deps = Deps { data_dir: self.data.path().to_path_buf(), secrets: &self.secrets, env: &env, home: None, windows: true };
        f(&deps)
    }
}

fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                todo.push(p);
            } else {
                out.push(p);
            }
        }
    }
    out
}

const ORIGINAL: &str = "# my Codex settings\nmodel = \"gpt-x\"   # keep this comment\n\n[model_providers.mine]\nname = \"Mine\"\nbase_url = \"http://localhost:1234/v1\"\n\n[projects.\"C:\\\\work\"]\ntrust_level = \"trusted\"\n";

// ---- adapters ----

#[test]
fn embedded_adapters_parse_and_validate() {
    let loaded = adapter::load_all(None);
    assert!(loaded.problems.is_empty(), "{:?}", loaded.problems);
    let go = loaded.adapters.iter().find(|a| a.id == "opencode-go").expect("opencode-go");
    assert!(go.needs_review);
    assert_eq!(go.note.as_deref(), Some("Check OpenCode's terms for your plan."));
    assert_eq!(go.base_url, "https://opencode.ai/inference/go/openai/v1");
    assert_eq!(go.routes.len(), 1);
    assert_eq!(go.routes[0].shape, Shape::Responses);
    assert_eq!(go.key_slot(), "model_bridge:opencode-go");
    let blank = loaded.adapters.iter().find(|a| a.custom).expect("blank adapter");
    assert!(blank.base_url.is_empty());
    assert_eq!(blank.display_name, "Any service that speaks the Responses API");
}

fn sample() -> Adapter {
    adapter::parse(
        "id = \"svc\"\ndisplay_name = \"Svc\"\nbase_url = \"https://example.test/v1\"\n[auth]\nheader = \"Authorization\"\nscheme = \"Bearer\"\nenv_var = \"SVC_KEY\"\n[[routes]]\nshape = \"responses\"\nmodels = [\"m1\"]\n",
    )
    .unwrap()
}

#[test]
fn adapter_validation_rejects_bad_files_in_plain_words() {
    let good = "id = \"svc\"\ndisplay_name = \"Svc\"\nbase_url = \"https://example.test/v1\"\n[auth]\nheader = \"Authorization\"\nscheme = \"Bearer\"\nenv_var = \"SVC_KEY\"\n[[routes]]\nshape = \"responses\"\n";
    assert!(adapter::parse(good).is_ok());
    for (from, to) in [
        ("id = \"svc\"", "id = \"Bad Id\""),
        ("https://example.test/v1", "http://example.test/v1"),
        ("https://example.test/v1", "ftp://example.test"),
        ("https://example.test/v1", "https://user@example.test"),
        ("SVC_KEY", "svc key"),
        ("shape = \"responses\"", "shape = \"smoke_signals\""),
    ] {
        let bad = good.replace(from, to);
        assert!(adapter::parse(&bad).is_err(), "should refuse: {to}");
    }
    assert!(adapter::parse(&good.replace("[[routes]]\nshape = \"responses\"\n", "")).is_err());
    assert!(adapter::parse(&good.replace("https://example.test/v1", "http://localhost:8080/v1")).is_ok());
}

#[test]
fn user_folder_adds_and_overrides_and_reports_bad_files() {
    let t = Temp::new("adapters");
    let override_go = "id = \"opencode-go\"\ndisplay_name = \"My Go\"\nbase_url = \"https://example.test/go\"\n[auth]\nheader = \"Authorization\"\nscheme = \"Bearer\"\nenv_var = \"GO_KEY\"\n[[routes]]\nshape = \"responses\"\n";
    std::fs::write(t.path().join("a.toml"), override_go).unwrap();
    std::fs::write(t.path().join("b.toml"), override_go.replace("opencode-go", "extra").replace("My Go", "Extra")).unwrap();
    std::fs::write(t.path().join("c.toml"), "id = [oops").unwrap();
    std::fs::write(t.path().join("notes.txt"), "ignored").unwrap();
    let loaded = adapter::load_all(Some(t.path()));
    let go = loaded.adapters.iter().find(|a| a.id == "opencode-go").unwrap();
    assert_eq!(go.display_name, "My Go");
    assert!(!go.needs_review, "the user's file fully replaces the default");
    assert!(loaded.adapters.iter().any(|a| a.id == "extra"));
    assert!(loaded.adapters.iter().any(|a| a.custom), "the blank adapter stays");
    assert_eq!(loaded.problems.len(), 1);
    assert!(loaded.problems[0].starts_with("c.toml:"));
}

// ---- keys ----

#[test]
fn key_is_stored_only_in_the_credential_store() {
    let rig = Rig::new("keys");
    let label = rig.with(|d| save_key(d, "opencode-go", &format!("  {KEY}\n"))).unwrap();
    assert_eq!(label, "memory");
    assert_eq!(rig.secrets.get("model_bridge:opencode-go").unwrap().as_deref(), Some(KEY));
    let id = rig.with(|d| add_custom(d, "My Box", "https://box.example.test/v1")).unwrap();
    rig.with(|d| save_key(d, &id, KEY)).unwrap();
    rig.with(|d| apply(d, "opencode-go")).unwrap();
    rig.with(|d| apply(d, &id)).unwrap();
    let listing = serde_json::to_string(&rig.with(list)).unwrap();
    assert!(!listing.contains(KEY));
    let preview = serde_json::to_string(&rig.with(|d| preview(d, "opencode-go")).unwrap()).unwrap();
    assert!(!preview.contains(KEY));
    for root in [rig.data.path(), rig.codex_home.path()] {
        for file in files_under(root) {
            let bytes = std::fs::read(&file).unwrap();
            assert!(!String::from_utf8_lossy(&bytes).contains("SECRET-1234567890"), "key found in {}", file.display());
        }
    }
}

#[test]
fn bad_keys_are_refused_without_repeating_them() {
    let rig = Rig::new("badkeys");
    for bad in ["", "   ", "two words", "line\nbreak"] {
        let err = rig.with(|d| save_key(d, "opencode-go", bad)).unwrap_err();
        assert!(!err.contains("two words") && !err.contains("break"));
    }
    assert!(rig.secrets.get("model_bridge:opencode-go").unwrap().is_none());
    let err = rig.with(|d| save_key(d, "any-responses", KEY)).unwrap_err();
    assert_eq!(err, "Type the name and address first.");
    let unavailable = Rig { secrets: MemoryBackend::unavailable(), ..Rig::new("nostore") };
    let err = unavailable.with(|d| save_key(d, "opencode-go", KEY)).unwrap_err();
    assert!(err.contains("not available") && !err.contains(KEY));
}

#[test]
fn custom_service_needs_a_name_and_a_safe_address() {
    let rig = Rig::new("custom");
    assert!(rig.with(|d| add_custom(d, "  ", "https://x.test")).is_err());
    assert!(rig.with(|d| add_custom(d, "Box", "http://box.example.test")).is_err());
    assert!(rig.with(|d| add_custom(d, "Box", "not a url")).is_err());
    let id = rig.with(|d| add_custom(d, "My Box", "https://box.example.test/v1/")).unwrap();
    assert_eq!(id, "custom-my-box");
    assert!(rig.with(|d| add_custom(d, "My Box", "https://box.example.test/v1")).is_err());
    let view = rig.with(list);
    let mine = view.adapters.iter().find(|a| a.id == id).unwrap();
    assert_eq!(mine.base_url, "https://box.example.test/v1");
    assert!(mine.codex_direct);
    assert!(mine.env_line.contains("PM_MODEL_KEY_MY_BOX"));
    assert!(!mine.env_line.contains(KEY));
}

// ---- the Codex recipe ----

fn go_plan(rig: &Rig) -> recipe::Plan {
    let a = adapter::load_all(None).adapters.into_iter().find(|a| a.id == "opencode-go").unwrap();
    let r = recipe::find("codex").unwrap();
    recipe::plan(&r, &a, rig.config(), None).unwrap()
}

const ADDED: &str = "\n[model_providers.pm-opencode-go]\nname = \"OpenCode Go\"\nbase_url = \"https://opencode.ai/inference/go/openai/v1\"\nenv_key = \"OPENCODE_API_KEY\"\nwire_api = \"responses\"\n\n[profiles.pm-opencode-go]\nmodel_provider = \"pm-opencode-go\"\nmodel = \"kimi-k3\"\n";

#[test]
fn recipe_writes_exactly_its_tables_and_keeps_the_rest_byte_for_byte() {
    let rig = Rig::new("golden");
    std::fs::write(rig.config(), ORIGINAL).unwrap();
    let plan = go_plan(&rig);
    let state_path = rig.data.path().join("state.json");
    recipe::apply(&plan, &state_path).unwrap();
    let now = std::fs::read_to_string(rig.config()).unwrap();
    assert!(now.starts_with(ORIGINAL), "the original bytes must be a prefix:\n{now}");
    assert_eq!(&now[ORIGINAL.len()..], ADDED);
    let v: toml::Value = toml::from_str(&now).unwrap();
    assert_eq!(v["model_providers"]["pm-opencode-go"]["wire_api"].as_str(), Some("responses"));
    assert_eq!(v["model_providers"]["mine"]["name"].as_str(), Some("Mine"));
}

#[test]
fn recipe_handles_a_file_with_no_final_line_break_and_a_missing_file() {
    let rig = Rig::new("edges");
    std::fs::write(rig.config(), "model = \"x\"").unwrap();
    recipe::apply(&go_plan(&rig), &rig.data.path().join("s.json")).unwrap();
    let now = std::fs::read_to_string(rig.config()).unwrap();
    assert!(now.starts_with("model = \"x\"\n"));
    assert!(now.contains("[model_providers.pm-opencode-go]"));

    let fresh = Rig::new("fresh");
    let state = recipe::apply(&go_plan(&fresh), &fresh.data.path().join("s.json")).unwrap();
    assert!(!state.existed_before && state.backup_path.is_none());
    assert!(std::fs::read_to_string(fresh.config()).unwrap().contains("wire_api = \"responses\""));
    let msg = recipe::undo(&state, &fresh.data.path().join("s.json")).unwrap();
    assert!(msg.contains("no file before"));
    assert!(!fresh.config().exists());
}

#[test]
fn adding_twice_replaces_our_table_and_does_not_duplicate_it() {
    let rig = Rig::new("twice");
    std::fs::write(rig.config(), ORIGINAL).unwrap();
    let plan = go_plan(&rig);
    recipe::apply(&plan, &rig.data.path().join("s.json")).unwrap();
    recipe::apply(&plan, &rig.data.path().join("s.json")).unwrap();
    let now = std::fs::read_to_string(rig.config()).unwrap();
    assert_eq!(now.matches("[model_providers.pm-opencode-go]").count(), 1);
    assert_eq!(now.matches("[profiles.pm-opencode-go]").count(), 1);
    assert!(now.starts_with(ORIGINAL));
}

#[test]
fn backup_and_undo_round_trip_restores_the_exact_bytes() {
    let rig = Rig::new("undo");
    let original = format!("{ORIGINAL}\r\n# crlf line\r\n");
    std::fs::write(rig.config(), &original).unwrap();
    let sp = rig.data.path().join("s.json");
    let state = recipe::apply(&go_plan(&rig), &sp).unwrap();
    let backup = state.backup_path.clone().expect("backup");
    assert_eq!(backup.parent(), rig.config().parent(), "next to the file");
    assert!(backup.file_name().unwrap().to_string_lossy().starts_with("config.toml.pm-backup-"));
    assert_eq!(std::fs::read(&backup).unwrap(), original.as_bytes());
    assert_ne!(std::fs::read(rig.config()).unwrap(), original.as_bytes());
    let msg = recipe::undo(&state, &sp).unwrap();
    assert!(msg.contains("exactly"));
    assert_eq!(std::fs::read(rig.config()).unwrap(), original.as_bytes());
    assert!(backup.exists(), "the backup copy is kept");
    assert!(!sp.exists());
}

#[test]
fn undo_after_the_person_edited_removes_only_our_tables() {
    let rig = Rig::new("undo-edited");
    std::fs::write(rig.config(), ORIGINAL).unwrap();
    let sp = rig.data.path().join("s.json");
    let state = recipe::apply(&go_plan(&rig), &sp).unwrap();
    let mut edited = std::fs::read_to_string(rig.config()).unwrap();
    edited.push_str("\n[mcp_servers.later]\ncommand = \"x\"\n");
    std::fs::write(rig.config(), &edited).unwrap();
    let msg = recipe::undo(&state, &sp).unwrap();
    assert!(msg.contains("changed since"));
    let now = std::fs::read_to_string(rig.config()).unwrap();
    assert!(!now.contains("pm-opencode-go"));
    assert!(now.contains("[mcp_servers.later]") && now.contains("# keep this comment"));
}

#[test]
fn an_unreadable_settings_file_is_left_alone() {
    let rig = Rig::new("broken");
    std::fs::write(rig.config(), "this is = = not toml [").unwrap();
    let err = recipe::apply(&go_plan(&rig), &rig.data.path().join("s.json")).unwrap_err();
    assert!(err.contains("nothing was changed"));
    assert_eq!(std::fs::read_to_string(rig.config()).unwrap(), "this is = = not toml [");
    assert_eq!(files_under(rig.codex_home.path()).len(), 1, "no backup, no temp file");
}

#[test]
fn a_service_without_the_direct_route_is_refused_and_nothing_is_written() {
    let rig = Rig::new("refuse");
    std::fs::write(rig.config(), ORIGINAL).unwrap();
    let mut a = sample();
    a.routes = vec![
        Route { shape: Shape::ChatCompletions, base_url: None, models: vec![] },
        Route { shape: Shape::AnthropicMessages, base_url: None, models: vec![] },
    ];
    let r = recipe::find("codex").unwrap();
    let err = recipe::plan(&r, &a, rig.config(), None).unwrap_err();
    assert_eq!(err, NEEDS_TRANSLATOR);
    assert_eq!(err, "This service needs the translator (coming next).");
    // Through the user folder too: the whole flow refuses.
    let dir = rig.data.path().join("model_bridge").join("adapters");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("svc.toml"), adapter::to_toml(&a).unwrap()).unwrap();
    rig.secrets.set("model_bridge:svc", KEY).unwrap();
    assert_eq!(rig.with(|d| preview(d, "svc")).unwrap_err(), NEEDS_TRANSLATOR);
    assert_eq!(rig.with(|d| apply(d, "svc")).unwrap_err(), NEEDS_TRANSLATOR);
    assert_eq!(std::fs::read_to_string(rig.config()).unwrap(), ORIGINAL);
    assert_eq!(files_under(rig.codex_home.path()).len(), 1);
    let view = rig.with(list);
    assert!(!view.adapters.iter().find(|a| a.id == "svc").unwrap().codex_direct);
}

#[test]
fn apply_needs_a_saved_key_and_preview_shows_no_key() {
    let rig = Rig::new("needkey");
    assert_eq!(rig.with(|d| apply(d, "opencode-go")).unwrap_err(), "Save your key first.");
    assert!(!rig.config().exists());
    let p = rig.with(|d| preview(d, "opencode-go")).unwrap();
    assert!(p.title.starts_with("PM will add a provider to your Codex settings. A backup is kept."));
    assert_eq!(p.lines[0], "[model_providers.pm-opencode-go]");
    assert!(p.lines.contains(&"env_key = \"OPENCODE_API_KEY\"".to_string()));
    assert!(p.lines.contains(&"wire_api = \"responses\"".to_string()));
    assert_eq!(p.env_line, "$env:OPENCODE_API_KEY = \"<paste your key here>\"");
}

#[test]
fn use_then_remove_undoes_the_settings_and_forgets_the_key() {
    let rig = Rig::new("remove");
    std::fs::write(rig.config(), ORIGINAL).unwrap();
    rig.with(|d| save_key(d, "opencode-go", KEY)).unwrap();
    let done = rig.with(|d| apply(d, "opencode-go")).unwrap();
    assert_eq!(done.message, "Codex can now use OpenCode Go.");
    assert!(done.backup_path.is_some());
    let view = rig.with(list);
    let go = view.adapters.iter().find(|a| a.id == "opencode-go").unwrap();
    assert!(go.key_saved && go.applied.is_some());
    rig.with(|d| remove_key(d, "opencode-go")).unwrap();
    assert_eq!(std::fs::read_to_string(rig.config()).unwrap(), ORIGINAL);
    assert!(rig.secrets.get("model_bridge:opencode-go").unwrap().is_none());
    assert_eq!(rig.with(|d| undo(d, "opencode-go")).unwrap_err(), "There is nothing to undo.");
}

// ---- environment ----

#[test]
fn a_started_pane_gets_the_key_in_its_environment_only() {
    let _g = FLAG.lock().unwrap();
    let rig = Rig::new("env");
    rig.with(|d| save_key(d, "opencode-go", KEY)).unwrap();
    apply_settings(&serde_json::json!({ "model_bridge_enabled": false }));
    assert!(rig.with(|d| launch_env_with(d, "codex")).is_empty(), "off by default");
    apply_settings(&serde_json::json!({ "model_bridge_enabled": true }));
    assert!(rig.with(|d| launch_env_with(d, "codex")).is_empty(), "nothing added to Codex yet");
    rig.with(|d| apply(d, "opencode-go")).unwrap();
    let env = rig.with(|d| launch_env_with(d, "codex"));
    assert_eq!(env, vec![("OPENCODE_API_KEY".to_string(), KEY.to_string())]);
    assert!(rig.with(|d| launch_env_with(d, "some-other-agent")).is_empty());
    apply_settings(&serde_json::json!({ "model_bridge_enabled": false }));
    assert!(rig.with(|d| launch_env_with(d, "codex")).is_empty(), "the switch is the kill switch");
    // The settings file holds the NAME of the variable, never the key.
    assert!(!std::fs::read_to_string(rig.config()).unwrap().contains("SECRET"));
}

#[test]
fn labs_flag_follows_the_settings() {
    let _g = FLAG.lock().unwrap();
    apply_settings(&serde_json::json!({}));
    assert!(!enabled());
    apply_settings(&serde_json::json!({ "model_bridge_enabled": "yes" }));
    assert!(!enabled());
    apply_settings(&serde_json::json!({ "model_bridge_enabled": true }));
    assert!(enabled());
    apply_settings(&serde_json::json!({ "model_bridge_enabled": false }));
    assert!(!enabled());
}

#[test]
fn config_path_uses_the_variable_then_the_home_folder() {
    let none = |_: &str| None;
    let set = |n: &str| (n == "CODEX_HOME").then(|| "D:/codex-home".to_string());
    let t = "{env:CODEX_HOME|~/.codex}/config.toml";
    assert_eq!(recipe::resolve_path(t, &set, None).unwrap(), PathBuf::from("D:/codex-home/config.toml"));
    let from_home = recipe::resolve_path(t, &none, Some(Path::new("/home/a"))).unwrap();
    assert!(from_home.ends_with(".codex/config.toml") || from_home.ends_with(".codex\\config.toml"));
    assert!(recipe::resolve_path(t, &none, None).is_err());
}

#[test]
fn embedded_recipe_parses() {
    let r = recipe::find("codex").unwrap();
    assert_eq!(r.agent_id, "codex");
    assert_eq!(r.requires_shape, Shape::Responses);
    assert_eq!(r.provider.table, "model_providers");
    assert!(r.profile.is_some());
}
