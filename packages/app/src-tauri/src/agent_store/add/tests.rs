//! Tests for `add_in` and `remove_in`. Temp folders only (never the real agents folder), a test catalog built
//! with `catalog::load`, and the program `cargo`, which is on PATH while the tests run (as in the approval
//! tests). The program name `pm-no-such-agent` is never installed.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use super::{add_in, remove_in};
use crate::agent_store::catalog::{self, CatalogEntry};
use crate::tui::declarative::approvals::Approvals;
use crate::tui::declarative::loader::LoadInputs;
use crate::tui::declarative::service::listing;
use crate::tui::registry::Registry;

const CODE_IDS: [&str; 1] = ["claude"];

/// One temp folder for a test: `agents/` for the definitions, `approvals.json` beside it.
struct Root {
    dir: PathBuf,
}

impl Root {
    fn new() -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("pm-store-add-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        fs::create_dir_all(&dir).unwrap();
        Self { dir }
    }

    fn agents(&self) -> PathBuf {
        self.dir.join("agents")
    }

    fn approvals(&self) -> PathBuf {
        self.dir.join("approvals.json")
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// A valid `entry.toml` for `id`.
fn entry_toml(id: &str, builtin: bool, ambiguous: bool) -> String {
    format!(
        r#"schema_version = 1

[store]
id = "{id}"
tagline = "A test agent."
tier = "starter"
builtin = {builtin}
categories = ["general"]
status = "active"
ambiguous_name = {ambiguous}
first_listed = "2026-10-28"

[vendor]
name = "Test Vendor"
homepage = "https://example.com/{id}"
license = "Apache-2.0"
license_kind = "open_source"

[sign_in]
kind = "none"
summary = "No sign-in needed."

[checked]
date = "2026-10-28"
version = "1.0.0"
how = "vendor_docs"
windows_tested = false
"#
    )
}

/// A definition that runs `binary` in a pane with the given argv and extra tables.
fn definition(id: &str, binary: &str, argv: &str, extra: &str) -> String {
    format!(
        "schema_version = 1\n[agent]\nid = \"{id}\"\nlabel = \"Test {id}\"\n[binary]\nnames = [\"{binary}\"]\n\
         install_hint = \"install {binary}\"\n[headless]\nargv = {argv}\ntask_separator = \"--\"\n\
         [headless.output]\nformat = \"plain_text\"\n{extra}"
    )
}

/// The files of one catalog folder, as the generator lists them.
fn folder(id: &str, entry: String, definition: Option<String>, fixtures: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut out = vec![(format!("agents/{id}/entry.toml"), entry)];
    if let Some(text) = definition {
        out.push((format!("agents/{id}/definition.toml"), text));
    }
    for (name, text) in fixtures {
        out.push((format!("agents/{id}/fixtures/{name}"), text.to_string()));
    }
    out
}

/// A test catalog from the given folders: its entries and its version.
fn catalog_of(files: &[Vec<(String, String)>]) -> (Vec<CatalogEntry>, String) {
    let all: Vec<(String, String)> = files.concat();
    let refs: Vec<(&str, &str)> = all.iter().map(|(p, t)| (p.as_str(), t.as_str())).collect();
    let loaded = catalog::load(&refs);
    assert!(loaded.problems.is_empty(), "the test catalog is broken: {:?}", loaded.problems);
    (loaded.entries, catalog::version_of(&refs))
}

/// A catalog with one entry `nimbus` that runs `cargo`, with one fixture.
fn nimbus(binary: &str, ambiguous: bool, argv: &str, extra: &str) -> (Vec<CatalogEntry>, String) {
    let def = definition("nimbus", binary, argv, extra);
    catalog_of(&[folder("nimbus", entry_toml("nimbus", false, ambiguous), Some(def), &[("idle.txt", "idle screen\n")])])
}

fn inputs(agents: &Path) -> LoadInputs<'_> {
    LoadInputs { bundled: &[], user_dir: Some(agents), code_ids: &CODE_IDS, projects: &[], extra: &[] }
}

fn shipped_text(binary: &str) -> String {
    definition("nimbus", binary, "[\"run\"]", "")
}

/// Every file or folder name under `dir`, sorted. Empty when `dir` does not exist.
fn names(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = fs::read_dir(dir).map(|rd| rd.filter_map(Result::ok).map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
    out.sort();
    out
}

#[test]
fn adding_writes_the_shipped_text_and_fixtures_and_lists_active() {
    let root = Root::new();
    let (entries, v) = nimbus("cargo", false, "[\"run\"]", "");
    let registry = Registry::seeded();
    let agents = root.agents();
    let inputs = inputs(&agents);

    let result = add_in(&entries, &v, &inputs, &root.approvals(), &registry, "nimbus", false).unwrap();
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(result["id"], "nimbus");
    assert_eq!(result["state"], "active", "{result}");

    assert_eq!(fs::read_to_string(agents.join("nimbus.toml")).unwrap(), shipped_text("cargo"), "the shipped bytes, exactly");
    assert_eq!(fs::read_to_string(agents.join("nimbus.fixtures").join("idle.txt")).unwrap(), "idle screen\n");

    let listed = listing(&inputs, &root.approvals());
    let row = listed["definitions"].as_array().unwrap().iter().find(|d| d["id"] == "nimbus").expect("a row for nimbus");
    assert_eq!(row["state"], "active");
    assert_eq!(row["approved_by"], "store");
    assert!(registry.ordered().iter().any(|p| p.id() == "nimbus"), "the reload registers the agent");
}

#[test]
fn an_existing_file_or_fixtures_folder_refuses_and_is_never_overwritten() {
    let root = Root::new();
    let (entries, v) = nimbus("cargo", false, "[\"run\"]", "");
    let registry = Registry::seeded();
    let agents = root.agents();
    let inputs = inputs(&agents);
    fs::create_dir_all(&agents).unwrap();

    // Another file, under another name, already holds the id.
    fs::write(agents.join("other.toml"), shipped_text("cargo")).unwrap();
    let err = add_in(&entries, &v, &inputs, &root.approvals(), &registry, "nimbus", false).unwrap_err();
    assert!(err.starts_with("AGENT_DEF_DUPLICATE_ID"), "{err}");
    assert!(!agents.join("nimbus.toml").exists());
    fs::remove_file(agents.join("other.toml")).unwrap();

    // The person's own file with the same name.
    fs::write(agents.join("nimbus.toml"), "# mine\n").unwrap();
    let err = add_in(&entries, &v, &inputs, &root.approvals(), &registry, "nimbus", false).unwrap_err();
    assert!(err.starts_with("AGENT_DEF_DUPLICATE_ID"), "{err}");
    assert_eq!(fs::read_to_string(agents.join("nimbus.toml")).unwrap(), "# mine\n");
    fs::remove_file(agents.join("nimbus.toml")).unwrap();

    // A fixtures folder that is not ours is never merged into.
    fs::create_dir_all(agents.join("nimbus.fixtures")).unwrap();
    fs::write(agents.join("nimbus.fixtures").join("mine.txt"), "mine").unwrap();
    let err = add_in(&entries, &v, &inputs, &root.approvals(), &registry, "nimbus", false).unwrap_err();
    assert!(err.starts_with("AGENT_DEF_DUPLICATE_ID"), "{err}");
    assert_eq!(names(&agents.join("nimbus.fixtures")), ["mine.txt"]);
    assert!(!agents.join("nimbus.toml").exists());
}

#[test]
fn a_missing_program_is_refused_before_anything_is_written() {
    let root = Root::new();
    let (entries, v) = nimbus("pm-no-such-agent", false, "[\"run\"]", "");
    let registry = Registry::seeded();
    let agents = root.agents();

    let err = add_in(&entries, &v, &inputs(&agents), &root.approvals(), &registry, "nimbus", true).unwrap_err();
    assert!(err.starts_with("ADD_PROGRAM_MISSING"), "{err}");
    assert!(!agents.exists(), "no agents folder is created when nothing is added");
}

#[test]
fn an_ambiguous_name_needs_the_person_to_confirm_it() {
    let root = Root::new();
    let (entries, v) = nimbus("cargo", true, "[\"run\"]", "");
    let registry = Registry::seeded();
    let agents = root.agents();
    let inputs = inputs(&agents);

    let err = add_in(&entries, &v, &inputs, &root.approvals(), &registry, "nimbus", false).unwrap_err();
    assert!(err.starts_with("ADD_AMBIGUOUS_NOT_CONFIRMED"), "{err}");
    assert!(!agents.exists(), "nothing is written before the confirm");

    let result = add_in(&entries, &v, &inputs, &root.approvals(), &registry, "nimbus", true).unwrap();
    assert_eq!(result["state"], "active", "{result}");
}

#[test]
fn an_unknown_id_and_a_built_in_agent_are_refused() {
    let root = Root::new();
    let built_in = entry_toml("claude", true, false);
    let (entries, v) = catalog_of(&[folder("claude", built_in, None, &[])]);
    let registry = Registry::seeded();
    let agents = root.agents();
    let inputs = inputs(&agents);

    let err = add_in(&entries, &v, &inputs, &root.approvals(), &registry, "nope", false).unwrap_err();
    assert!(err.starts_with("STORE_UNKNOWN_ID"), "{err}");
    let err = add_in(&entries, &v, &inputs, &root.approvals(), &registry, "claude", false).unwrap_err();
    assert!(err.starts_with("STORE_BUILTIN"), "{err}");
    assert!(!agents.exists());
    assert!(remove_in(&entries, &inputs, &root.approvals(), &registry, "claude").unwrap_err().starts_with("STORE_BUILTIN"));
}

#[test]
fn a_definition_with_a_setting_a_store_agent_may_not_carry_is_refused() {
    let root = Root::new();
    let registry = Registry::seeded();
    let agents = root.agents();
    let inputs = inputs(&agents);
    // (argv, extra tables, the words the refusal must name)
    let cases = [
        ("[\"run\", \"--yolo\"]", "", "--yolo"),
        ("[\"run\"]", "[env]\nset = { FOO = \"bar\" }\n", "[env] set"),
    ];
    for (argv, extra, named) in cases {
        let (entries, v) = nimbus("cargo", false, argv, extra);
        let err = add_in(&entries, &v, &inputs, &root.approvals(), &registry, "nimbus", false).unwrap_err();
        assert!(err.starts_with("STORE_FORBIDDEN_SETTING"), "{named}: {err}");
        assert!(err.contains(named), "{named}: {err}");
        assert!(!agents.exists(), "{named}: nothing is written");
    }
}

#[test]
fn a_failed_approval_takes_back_the_files_it_wrote() {
    let root = Root::new();
    let (entries, v) = nimbus("cargo", false, "[\"run\"]", "");
    let registry = Registry::seeded();
    let agents = root.agents();
    // The approvals "file" sits under a regular file, so saving the record fails after the files are written.
    fs::create_dir_all(&root.dir).unwrap();
    fs::write(root.dir.join("blocker"), "x").unwrap();
    let approvals = root.dir.join("blocker").join("approvals.json");

    let err = add_in(&entries, &v, &inputs(&agents), &approvals, &registry, "nimbus", false).unwrap_err();
    assert!(err.starts_with("AGENT_DEF_STORE_FAILED"), "{err}");
    assert_eq!(names(&agents), Vec::<String>::new(), "the definition and its fixtures are gone again");
}

#[test]
fn remove_takes_back_an_unedited_file_and_refuses_an_edited_one() {
    let root = Root::new();
    let (entries, v) = nimbus("cargo", false, "[\"run\"]", "");
    let registry = Registry::seeded();
    let agents = root.agents();
    let inputs = inputs(&agents);
    add_in(&entries, &v, &inputs, &root.approvals(), &registry, "nimbus", false).unwrap();

    // The person edits the file: it is kept, byte for byte.
    let edited = format!("{}# my change\n", shipped_text("cargo"));
    fs::write(agents.join("nimbus.toml"), &edited).unwrap();
    let err = remove_in(&entries, &inputs, &root.approvals(), &registry, "nimbus").unwrap_err();
    assert!(err.starts_with("REMOVE_NOT_STORE_FILE"), "{err}");
    assert_eq!(fs::read_to_string(agents.join("nimbus.toml")).unwrap(), edited);
    assert!(agents.join("nimbus.fixtures").join("idle.txt").exists());

    // The shipped copy again: it is removed (to the removed folder, so it can be put back) with its approval.
    fs::write(agents.join("nimbus.toml"), shipped_text("cargo")).unwrap();
    let result = remove_in(&entries, &inputs, &root.approvals(), &registry, "nimbus").unwrap();
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(result["fixtures_kept"], 0);
    assert!(!agents.join("nimbus.toml").exists());
    assert!(!agents.join("nimbus.fixtures").exists(), "the shipped fixture and its empty folder are removed");
    assert!(Approvals::load(&root.approvals()).records().is_empty(), "the approval goes with the file");
    assert!(agents.join(".removed").is_dir(), "the file is kept in .removed, not deleted");
}

#[test]
fn remove_keeps_a_fixture_the_person_changed() {
    let root = Root::new();
    let (entries, v) = nimbus("cargo", false, "[\"run\"]", "");
    let registry = Registry::seeded();
    let agents = root.agents();
    let inputs = inputs(&agents);
    add_in(&entries, &v, &inputs, &root.approvals(), &registry, "nimbus", false).unwrap();
    let fixture = agents.join("nimbus.fixtures").join("idle.txt");
    fs::write(&fixture, "my own screen").unwrap();

    let result = remove_in(&entries, &inputs, &root.approvals(), &registry, "nimbus").unwrap();
    assert_eq!(result["fixtures_kept"], 1, "{result}");
    assert_eq!(fs::read_to_string(&fixture).unwrap(), "my own screen", "the changed fixture is not deleted");
    assert!(agents.join("nimbus.fixtures").is_dir(), "its folder stays with it");
}
