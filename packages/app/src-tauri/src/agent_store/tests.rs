//! Tests for the Store JSON api (`mod.rs`). Each test uses a temp folder for the record file, a test
//! catalog built with `catalog::load`, and a fake version probe. No test reads the app data folder or
//! runs a program: the test programs have names no PC has.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{json, Value};

use super::*;
use crate::agent_install::plan::{plan_install, Tools};
use crate::tui::install::InstallSpec;

/// The card fields `packages/app/src/lib/agent-store.ts` (`StoreCard`) declares, in that order.
const CARD_KEYS: &[&str] = &[
    "id",
    "label",
    "icon",
    "tagline",
    "tier",
    "builtin",
    "categories",
    "status",
    "replaced_by",
    "vendor",
    "sign_in",
    "checked",
    "state",
    "state_reason",
    "installed_version",
    "modes",
    "can_install",
    "ambiguous_name",
    "last_check",
];
/// The fields `StoreDetail` adds to a card.
const DETAIL_KEYS: &[&str] = &["install_plan", "run_preview", "fixtures", "uninstall", "found_path_display"];

/// One temp folder per test, removed when the test ends.
struct Root {
    dir: PathBuf,
}

impl Root {
    fn new() -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("pm-store-api-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        fs::create_dir_all(&dir).unwrap();
        Self { dir }
    }

    fn record(&self) -> PathBuf {
        self.dir.join("agent-store.json")
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

const QWEN_ENTRY: &str = r#"schema_version = 1

[store]
id = "qwen"
tagline = "Alibaba's terminal coding agent."
tier = "starter"
builtin = false
categories = ["open_source"]
status = "active"
ambiguous_name = false
first_listed = "2026-10-28"

[vendor]
name = "Alibaba (Qwen)"
homepage = "https://github.com/QwenLM/qwen-code"
license = "Apache-2.0"
license_kind = "open_source"
license_url = "https://github.com/QwenLM/qwen-code/blob/main/LICENSE"

[sign_in]
kind = "account"
summary = "Open it and type /auth, then follow the screen."

[checked]
date = "2026-10-28"
version = "0.26.2"
how = "package_registry"
windows_tested = false
"#;

const QWEN_DEFINITION: &str = r#"schema_version = 1

[agent]
id = "qwen"
label = "Qwen Code"
kind = "generic"
experimental = true

[binary]
names = ["pm-as13-no-such-qwen"]

[install.windows]
source = "https://github.com/QwenLM/qwen-code"
npm = "@qwen-code/qwen-code"
"#;

const BUILTIN_ENTRY: &str = r#"schema_version = 1

[store]
id = "pm_test_builtin"
tagline = "A built-in agent."
tier = "verified"
builtin = true
categories = ["general"]
status = "active"
first_listed = "2026-10-28"

[vendor]
name = "Test Maker"
homepage = "https://example.com/builtin"
license = "Proprietary"
license_kind = "proprietary"

[sign_in]
kind = "none"
summary = "No sign-in needed."

[checked]
date = "2026-10-28"
version = "1.0.0"
how = "ran_on_windows"
windows_tested = true
"#;

/// Two entries: one with a definition (`qwen`) and one built into the app (`pm_test_builtin`).
fn test_catalog() -> Vec<CatalogEntry> {
    let loaded = catalog::load(&[
        ("agents/qwen/entry.toml", QWEN_ENTRY),
        ("agents/qwen/definition.toml", QWEN_DEFINITION),
        ("agents/pm_test_builtin/entry.toml", BUILTIN_ENTRY),
    ]);
    assert!(loaded.problems.is_empty(), "{:?}", loaded.problems);
    loaded.entries
}

fn qwen(entries: &[CatalogEntry]) -> &CatalogEntry {
    entries.iter().find(|e| e.entry.store.id == "qwen").expect("qwen is in the test catalog")
}

/// A probe that finds nothing.
fn no_version(_: &CatalogEntry, _: &Inputs) -> Option<String> {
    None
}

/// A probe that says every found program is version 0.26.2.
fn version_0_26_2(_: &CatalogEntry, _: &Inputs) -> Option<String> {
    Some("0.26.2".into())
}

fn card_ids(list: &Value) -> Vec<String> {
    list["entries"].as_array().unwrap().iter().map(|card| card["id"].as_str().unwrap().to_string()).collect()
}

fn keys_of(value: &Value) -> Vec<String> {
    let mut keys: Vec<String> = value.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    keys
}

fn sorted(list: &[&str]) -> Vec<String> {
    let mut out: Vec<String> = list.iter().map(|s| s.to_string()).collect();
    out.sort();
    out
}

fn spec(winget: Option<&str>, npm: Option<&str>, uv: Option<&str>, python: Option<&str>, script: Option<&str>) -> InstallSpec {
    InstallSpec {
        source: "https://example.com/install".into(),
        winget: winget.map(str::to_string),
        npm: npm.map(str::to_string),
        uv: uv.map(str::to_string),
        python: python.map(str::to_string),
        script_url: script.map(str::to_string),
        ..Default::default()
    }
}

fn everything_here() -> Tools {
    Tools {
        winget: Some("winget".into()),
        present: ["node", "git", "uv"].iter().map(|s| s.to_string()).collect(),
    }
}

#[test]
fn list_contains_every_shipped_id_and_every_id_of_a_test_catalog() {
    let root = Root::new();
    let record = root.record();
    let env = Env { record_path: &record, version: &no_version };

    let shipped = list_in(catalog::all(), &catalog::catalog_version(), &env).unwrap();
    let shipped_ids = card_ids(&shipped);
    for entry in catalog::all() {
        assert!(shipped_ids.contains(&entry.entry.store.id), "{} is missing from the list", entry.entry.store.id);
    }

    let custom = test_catalog();
    let out = list_in(&custom, "1a2b3c4d", &env).unwrap();
    assert_eq!(out["catalog_version"], "1a2b3c4d");
    assert_eq!(card_ids(&out), ["pm_test_builtin", "qwen"]);
}

#[test]
fn a_card_matches_the_snapshot() {
    let entries = test_catalog();
    let inputs = Inputs { windows: true, ..Inputs::default() };
    let card = card_value(qwen(&entries), &inputs, None);
    assert_eq!(
        card,
        json!({
            "id": "qwen",
            "label": "Qwen Code",
            "icon": "QW",
            "tagline": "Alibaba's terminal coding agent.",
            "tier": "starter",
            "builtin": false,
            "categories": ["open_source"],
            "status": "active",
            "replaced_by": "",
            "vendor": {
                "name": "Alibaba (Qwen)",
                "homepage": "https://github.com/QwenLM/qwen-code",
                "license": "Apache-2.0",
                "license_kind": "open_source",
                "license_url": "https://github.com/QwenLM/qwen-code/blob/main/LICENSE",
                "terms_url": null,
            },
            "sign_in": { "kind": "account", "summary": "Open it and type /auth, then follow the screen." },
            "checked": {
                "date": "2026-10-28",
                "version": "0.26.2",
                "how": "package_registry",
                "windows_tested": false,
            },
            "state": "available",
            "state_reason": "Not on this PC yet. Install it from here, or add it once it is installed.",
            "installed_version": null,
            "modes": ["Pane"],
            "can_install": true,
            "ambiguous_name": false,
            "last_check": null,
        })
    );
}

#[test]
fn a_found_program_shows_its_version_and_the_last_check() {
    let root = Root::new();
    let record = root.record();
    let mut rec = StoreRecord::default();
    rec.mark_added("qwen", "2026-10-28", "1a2b3c4d").unwrap();
    rec.mark_check("qwen", CheckRecord { date: "2026-10-28".into(), version: "0.26.2".into(), passed: 7, total: 7 })
        .unwrap();
    record::save(&record, &rec).unwrap();

    let entries = test_catalog();
    let env = Env { record_path: &record, version: &version_0_26_2 };
    let out = list_in(&entries, "1a2b3c4d", &env).unwrap();
    let card = &out["entries"][1];
    assert_eq!(card["id"], "qwen");
    assert_eq!(card["installed_version"], "0.26.2");
    assert_eq!(card["last_check"], json!({ "passed": 7, "total": 7, "version": "0.26.2", "at": "2026-10-28" }));
}

#[test]
fn the_card_and_detail_keys_are_the_ones_the_store_ts_reads() {
    let root = Root::new();
    let record = root.record();
    let entries = test_catalog();
    let env = Env { record_path: &record, version: &no_version };
    let card = card_for(qwen(&entries), &env);
    assert_eq!(keys_of(&card), sorted(CARD_KEYS));
    assert_eq!(keys_of(&card["vendor"]), sorted(&["name", "homepage", "license", "license_kind", "license_url", "terms_url"]));
    assert_eq!(keys_of(&card["sign_in"]), sorted(&["kind", "summary"]));
    assert_eq!(keys_of(&card["checked"]), sorted(&["date", "version", "how", "windows_tested"]));

    let mut rec = StoreRecord::default();
    rec.mark_check("qwen", CheckRecord { date: "2026-10-28".into(), version: "0.26.2".into(), passed: 1, total: 1 }).unwrap();
    record::save(&record, &rec).unwrap();
    let with_check = card_for(qwen(&entries), &env);
    assert_eq!(keys_of(&with_check["last_check"]), sorted(&["passed", "total", "version", "at"]));

    let tools = Tools::default();
    let plan = plan_install("qwen", "Qwen Code", catalog::install_spec_of(qwen(&entries)).as_ref(), &tools);
    let detail = detail_in(qwen(&entries), &env, &tools, &plan, None).unwrap();
    let mut expected: Vec<&str> = CARD_KEYS.to_vec();
    expected.extend_from_slice(DETAIL_KEYS);
    assert_eq!(keys_of(&detail), sorted(&expected));
}

#[test]
fn detail_has_the_install_plan_run_preview_and_uninstall_summary() {
    let root = Root::new();
    let record = root.record();
    let entries = test_catalog();
    let env = Env { record_path: &record, version: &no_version };
    let tools = Tools { winget: None, present: ["node".to_string()].into_iter().collect() };
    let plan = plan_install("qwen", "Qwen Code", catalog::install_spec_of(qwen(&entries)).as_ref(), &tools);

    let detail = detail_in(qwen(&entries), &env, &tools, &plan, None).unwrap();
    assert_eq!(detail["install_plan"]["available"], true);
    assert_eq!(detail["install_plan"]["steps"][0]["method"], "npm");
    assert_eq!(detail["run_preview"], json!(["pm-as13-no-such-qwen"]));
    assert_eq!(detail["fixtures"]["count"], 0);
    assert_eq!(detail["uninstall"]["available"], false, "the Store did not install it");
    assert_eq!(detail["uninstall"]["steps"], json!([]));
    assert!(detail["uninstall"]["reason"].as_str().unwrap_or_default().contains("did not install"), "{}", detail["uninstall"]);
    assert_eq!(detail["found_path_display"], Value::Null);
}

#[test]
fn a_path_shows_the_home_folder_as_tilde() {
    let home = Path::new("C:/Users/you");
    let found = home.join("AppData").join("Local").join("qwen.exe");
    let shown = display_path_in(&found, Some(home));
    assert_eq!(shown, format!("~{MAIN_SEPARATOR}AppData{MAIN_SEPARATOR}Local{MAIN_SEPARATOR}qwen.exe"));
    assert!(!shown.contains("Users"), "{shown}");
    assert_eq!(display_path_in(home, Some(home)), "~");
    assert_eq!(display_path_in(Path::new("C:/Program Files/x.exe"), Some(home)), "C:/Program Files/x.exe");
    assert_eq!(display_path_in(&found, None), found.to_string_lossy());
}

#[test]
fn nothing_the_api_returns_holds_the_record_folder_or_a_user_folder() {
    let root = Root::new();
    let record = root.record();
    let entries = test_catalog();
    let env = Env { record_path: &record, version: &version_0_26_2 };
    let mut rec = StoreRecord::default();
    rec.mark_added("qwen", "2026-10-28", "1a2b3c4d").unwrap();
    record::save(&record, &rec).unwrap();
    let text = list_in(&entries, "1a2b3c4d", &env).unwrap().to_string();
    assert!(!text.contains(&*root.dir.to_string_lossy()), "{text}");
    assert!(!text.contains("Users"), "{text}");
    let written = fs::read_to_string(&record).unwrap();
    assert!(!written.contains("Users") && !written.contains(":\\") && !written.contains(":/"), "{written}");
}

#[test]
fn the_label_is_the_definition_name_or_the_vendor_name() {
    let entries = test_catalog();
    assert_eq!(qwen(&entries).label(), "Qwen Code");
    let builtin = entries.iter().find(|e| e.entry.store.id == "pm_test_builtin").unwrap();
    assert_eq!(builtin.label(), "Test Maker");
}

/// A built-in agent has no definition, so its card title is the label its TUI profile shows in the
/// rest of the app (`tui::descriptor_for`), never the vendor name ("Anthropic", "SST", ...).
#[test]
fn a_built_in_card_is_titled_with_its_tui_profile_label() {
    let _serial = crate::tui::registry::test_serial();
    let expected = [
        ("claude", "Claude Code"),
        ("codex", "Codex CLI"),
        ("opencode", "OpenCode"),
        ("cursor_agent", "Cursor Agent CLI"),
    ];
    for (id, label) in expected {
        let entry = catalog::all().iter().find(|e| e.entry.store.id == id).unwrap_or_else(|| panic!("{id} is shipped"));
        assert!(entry.definition_text.is_none(), "{id} is built in");
        assert_eq!(entry.label(), label, "{id}");
        assert_ne!(entry.label(), entry.entry.vendor.name, "{id} must not show its vendor");
    }
}

#[test]
fn a_built_in_card_has_no_install_no_run_command_and_no_modes_from_a_definition() {
    let entries = test_catalog();
    let builtin = entries.iter().find(|e| e.entry.store.id == "pm_test_builtin").unwrap();
    let inputs = Inputs { windows: true, ..Inputs::default() };
    let card = card_value(builtin, &inputs, None);
    assert_eq!(card["icon"], "PM");
    assert_eq!(card["state"], "available");
    assert_eq!(card["can_install"], false);
    assert_eq!(card["modes"], json!([]));
    assert_eq!(card["vendor"]["license_url"], Value::Null);
    assert_eq!(run_preview(builtin), Value::Null);
}

#[test]
fn the_add_and_remove_record_helpers_keep_an_install_record() {
    let root = Root::new();
    let record = root.record();
    mark_added_in(&record, "qwen", "2026-10-28", "1a2b3c4d").unwrap();
    let loaded = record::load(&record);
    assert_eq!(loaded.agents["qwen"].added, "2026-10-28");
    assert_eq!(loaded.agents["qwen"].catalog, "1a2b3c4d");

    update_record(&record, |rec| {
        rec.mark_install(
            "qwen",
            InstallRecord {
                method: "npm".into(),
                package: "@qwen-code/qwen-code".into(),
                version: "0.26.2".into(),
                date: "2026-10-28".into(),
            },
        )
    })
    .unwrap();

    update_record(&record, |rec| {
        rec.mark_removed("qwen");
        Ok(())
    })
    .unwrap();
    let after = record::load(&record);
    assert!(after.agents["qwen"].added.is_empty(), "the added flag is cleared");
    assert_eq!(after.agents["qwen"].install.as_ref().unwrap().package, "@qwen-code/qwen-code", "the install stays");
}

#[test]
fn the_recorded_method_and_package_come_from_the_plan_step() {
    let tools = everything_here();
    let winget = plan_install("claude", "Claude Code", Some(&spec(Some("Anthropic.ClaudeCode"), None, None, None, None)), &tools);
    assert_eq!(step_record(&winget, 0).unwrap(), ("winget", "Anthropic.ClaudeCode".to_string()));

    let npm = plan_install("claude", "Claude Code", Some(&spec(None, Some("@anthropic-ai/claude-code"), None, None, None)), &tools);
    assert_eq!(step_record(&npm, 0).unwrap(), ("npm", "@anthropic-ai/claude-code".to_string()));

    let uv = plan_install("aider", "Aider", Some(&spec(None, None, Some("aider-chat"), Some("3.12"), None)), &tools);
    assert_eq!(step_record(&uv, 0).unwrap(), ("uv", "aider-chat".to_string()));

    let script = plan_install("cursor", "Cursor", Some(&spec(None, None, None, None, Some("https://example.com/install.ps1"))), &tools);
    assert_eq!(step_record(&script, 0).unwrap(), ("script", String::new()));

    assert!(step_record(&npm, 7).unwrap_err().starts_with("INSTALL_NO_SUCH_STEP"));
}

#[test]
fn a_prerequisite_step_is_never_recorded_as_the_agent() {
    let tools = Tools { winget: Some("winget".into()), present: ["git".to_string()].into_iter().collect() };
    let plan = plan_install("claude", "Claude Code", Some(&spec(None, Some("@anthropic-ai/claude-code"), None, None, None)), &tools);
    assert!(plan.steps.first().is_some_and(|step| step.tool.is_some()), "node comes first: {:?}", plan.steps);
    let refused = step_record(&plan, 0).unwrap_err();
    assert!(refused.starts_with("STORE_NOT_AGENT_STEP"), "{refused}");
}

#[test]
fn record_check_keeps_numbers_only() {
    let root = Root::new();
    let record = root.record();
    let entries = test_catalog();
    let out = record_check_in(&entries, &record, "qwen", 7, 7, "v0.26.2 (Qwen)", "2026-10-28").unwrap();
    assert_eq!(out["last_check"], json!({ "passed": 7, "total": 7, "version": "0.26.2", "at": "2026-10-28" }));
    let stored = record::load(&record).agents["qwen"].check.clone().unwrap();
    assert_eq!((stored.passed, stored.total, stored.version.as_str()), (7, 7, "0.26.2"));

    let unknown = record_check_in(&entries, &record, "nope", 1, 1, "1.0", "2026-10-28").unwrap_err();
    assert!(unknown.starts_with("STORE_UNKNOWN_ID"), "{unknown}");
    let too_many = record_check_in(&entries, &record, "qwen", 8, 7, "1.0", "2026-10-28").unwrap_err();
    assert!(too_many.contains("more than total"), "{too_many}");
}

#[test]
fn version_only_keeps_one_version_number() {
    assert_eq!(version_only("0.26.2"), "0.26.2");
    assert_eq!(version_only("claude v2.1.0 (Claude Code)"), "2.1.0");
    assert_eq!(version_only("Version: 3.2"), "3.2");
    assert_eq!(version_only("1.0.0 extra"), "1.0.0");
    assert_eq!(version_only("C:\\Users\\you\\qwen.exe"), "");
    assert_eq!(version_only(""), "");
}

#[test]
fn the_card_key_list_has_no_key_the_ui_does_not_read() {
    let entries = test_catalog();
    let inputs = Inputs { windows: true, ..Inputs::default() };
    let card = card_value(qwen(&entries), &inputs, None);
    let known: BTreeSet<&str> = CARD_KEYS.iter().copied().collect();
    for key in keys_of(&card) {
        assert!(known.contains(key.as_str()), "{key} is not in CARD_KEYS");
    }
}

#[test]
fn check_source_gives_the_definition_and_fixtures_of_an_entry_and_nothing_for_a_built_in() {
    let files = [
        ("agents/qwen/entry.toml", QWEN_ENTRY),
        ("agents/qwen/definition.toml", QWEN_DEFINITION),
        ("agents/qwen/fixtures/tui_idle.txt", "idle screen
"),
        ("agents/pm_test_builtin/entry.toml", BUILTIN_ENTRY),
    ];
    let loaded = catalog::load(&files);
    let source = check_source_in(&loaded.entries, "qwen").unwrap();
    assert_eq!(source["definition"], QWEN_DEFINITION);
    assert_eq!(source["fixtures"], json!([{ "name": "tui_idle.txt", "text": "idle screen
" }]));
    let builtin = check_source_in(&loaded.entries, "pm_test_builtin").unwrap();
    assert_eq!(builtin["definition"], Value::Null);
    assert_eq!(builtin["fixtures"], json!([]));
    let err = check_source_in(&loaded.entries, "nope").unwrap_err();
    assert!(err.starts_with("STORE_UNKNOWN_ID:"), "{err}");
}
