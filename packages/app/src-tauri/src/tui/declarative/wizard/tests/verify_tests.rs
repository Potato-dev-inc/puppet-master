//! Verify: the checks, and the error format meant for an AI.

use serde_json::{json, Value};

use super::support::{call, ok, start, Tree, GOOD, MINIMAL, NO_PATH, WRONG_BINARY};

fn verify(wizard: &crate::tui::declarative::wizard::Wizard, id: &str, toml: &str) -> Value {
    ok(wizard, "verify", json!({"session": id, "toml": toml}))
}

fn probed(wizard: &crate::tui::declarative::wizard::Wizard) -> String {
    let id = start(wizard, "fake-agent");
    ok(wizard, "probe", json!({"session": id}));
    id
}

fn codes(v: &Value) -> Vec<String> {
    v["errors"].as_array().unwrap().iter().map(|e| e["code"].as_str().unwrap().to_string()).collect()
}

#[test]
fn a_good_draft_passes_and_shows_the_commands_it_will_run() {
    let tree = Tree::new("v-good");
    let wizard = tree.wizard();
    let id = probed(&wizard);
    let v = verify(&wizard, &id, GOOD);
    assert_eq!(v["ok"], true, "{}", v["text"]);
    assert_eq!(v["state"], "verified");
    assert_eq!(v["errors"], json!([]));
    assert_eq!(v["text"], "");
    assert_eq!(v["sha256"].as_str().unwrap().len(), 64);
    assert_eq!(v["rendered"]["headless_argv"], json!(["run", "--format", "json", "--", "<task>"]));
    assert_eq!(
        v["rendered"]["resume_argv"],
        json!(["run", "--format", "json", "-m", "<model>", "-s", "<session>", "--", "<task>"])
    );
    assert!(v["checked"]["rules"].as_u64().unwrap() >= 8);
    assert!(v["checked"]["fixtures"].as_u64().unwrap() >= 1, "the garbage fixture ran");
    assert_eq!(v["warnings"], json!([]));
}

#[test]
fn the_same_text_always_gets_the_same_digest() {
    let tree = Tree::new("v-sha");
    let wizard = tree.wizard();
    let id = probed(&wizard);
    let a = verify(&wizard, &id, GOOD)["sha256"].clone();
    let b = verify(&wizard, &id, GOOD)["sha256"].clone();
    assert_eq!(a, b);
    assert_ne!(a, verify(&wizard, &id, &GOOD.replace("Fake Agent", "Other"))["sha256"]);
}

#[test]
fn a_validator_error_names_the_field_the_line_and_how_to_fix_it() {
    let tree = Tree::new("v-nopath");
    let wizard = tree.wizard();
    let id = probed(&wizard);
    let v = verify(&wizard, &id, NO_PATH);
    assert_eq!(v["ok"], false);
    assert_eq!(v["state"], "probed", "a failing verify changes nothing");
    let err = &v["errors"][0];
    assert_eq!(err["stage"], "validate");
    assert_eq!(err["code"], "AGENT_DEF_INCONSISTENT");
    assert_eq!(err["path"], "headless.output.result.path");
    assert!(err["message"].as_str().unwrap().contains("path"));
    assert!(err["fix"].as_str().unwrap().contains("path = "), "{}", err["fix"]);
    let text = v["text"].as_str().unwrap();
    assert!(text.starts_with("The checker found 1 problem. Fix every one and reply with the complete corrected file in one ```toml block."), "{text}");
    assert!(text.contains("1. [validate] AGENT_DEF_INCONSISTENT at `headless.output.result.path`"), "{text}");
    assert!(text.contains("   Problem: "), "{text}");
    assert!(text.contains("   Fix: "), "{text}");
}

#[test]
fn a_toml_syntax_error_has_a_line() {
    let tree = Tree::new("v-syntax");
    let wizard = tree.wizard();
    let id = probed(&wizard);
    let v = verify(&wizard, &id, "schema_version = 1\n[agent]\nid = \"unterminated\n");
    assert_eq!(v["ok"], false);
    assert_eq!(v["errors"][0]["code"], "AGENT_DEF_INVALID_TOML");
    assert!(v["errors"][0]["line"].as_u64().is_some(), "{}", v["errors"][0]);
    assert!(v["text"].as_str().unwrap().contains("(line "), "{}", v["text"]);
}

#[test]
fn an_unknown_field_is_an_error_not_ignored() {
    let tree = Tree::new("v-unknown");
    let wizard = tree.wizard();
    let id = probed(&wizard);
    let v = verify(&wizard, &id, &format!("{GOOD}\n[surprise]\nx = 1\n"));
    assert_eq!(v["ok"], false);
    assert_eq!(v["errors"][0]["code"], "AGENT_DEF_UNKNOWN_FIELD");
    assert!(!v["errors"][0]["fix"].as_str().unwrap().is_empty());
}

#[test]
fn every_problem_is_reported_at_once() {
    let tree = Tree::new("v-many");
    let wizard = tree.wizard();
    let id = probed(&wizard);
    // wrong program name AND a result selector without a path
    let draft = NO_PATH.replace("names = [\"fake-agent\"]", "names = [\"other-agent\"]");
    let v = verify(&wizard, &id, &draft);
    let c = codes(&v);
    assert!(c.contains(&"AGENT_DEF_INCONSISTENT".to_string()), "{c:?}");
    assert!(c.contains(&"WIZARD_BINARY_MISMATCH".to_string()), "{c:?}");
    assert!(c.contains(&"WIZARD_BINARY_UNRESOLVED".to_string()), "{c:?}");
    let text = v["text"].as_str().unwrap();
    assert!(text.contains("found 3 problems"), "{text}");
    assert!(text.contains("\n2. [") && text.contains("\n3. ["), "{text}");
}

#[test]
fn the_program_name_must_be_the_one_the_person_typed() {
    let tree = Tree::new("v-binary");
    let wizard = tree.wizard();
    let id = probed(&wizard);
    let v = verify(&wizard, &id, WRONG_BINARY);
    let c = codes(&v);
    assert!(c.contains(&"WIZARD_BINARY_MISMATCH".to_string()), "{c:?}");
    let mismatch = v["errors"].as_array().unwrap().iter().find(|e| e["code"] == "WIZARD_BINARY_MISMATCH").unwrap();
    assert!(mismatch["message"].as_str().unwrap().contains("fake-agent"));
    // an extension or different case on the right name is fine
    let fine = verify(&wizard, &id, &GOOD.replace("names = [\"fake-agent\"]", "names = [\"Fake-Agent\"]"));
    assert_eq!(fine["ok"], true, "{}", fine["text"]);
}

#[test]
fn an_id_that_already_has_a_file_is_refused() {
    let tree = Tree::new("v-taken");
    let wizard = tree.wizard();
    let id = probed(&wizard);
    std::fs::create_dir_all(&tree.agents).unwrap();
    std::fs::write(tree.agents.join("fake_agent.toml"), "# someone else's file\n").unwrap();
    let v = verify(&wizard, &id, GOOD);
    assert_eq!(v["ok"], false);
    assert!(codes(&v).contains(&"WIZARD_ID_TAKEN".to_string()), "{}", v["text"]);
    // a leftover fixtures folder or provenance file counts too
    std::fs::remove_file(tree.agents.join("fake_agent.toml")).unwrap();
    std::fs::create_dir_all(tree.agents.join("fake_agent")).unwrap();
    assert!(codes(&verify(&wizard, &id, GOOD)).contains(&"WIZARD_ID_TAKEN".to_string()));
}

#[test]
fn a_built_in_agent_id_is_refused_by_the_validator() {
    let tree = Tree::new("v-reserved");
    let wizard = tree.wizard();
    let id = probed(&wizard);
    let v = verify(&wizard, &id, &GOOD.replace("id = \"fake_agent\"", "id = \"claude\""));
    assert_eq!(v["ok"], false);
    assert!(codes(&v).contains(&"AGENT_DEF_RESERVED_ID".to_string()), "{}", v["text"]);
}

#[test]
fn only_headless_cli_definitions_are_accepted() {
    let tree = Tree::new("v-kind");
    let wizard = tree.wizard();
    let id = probed(&wizard);
    let generic = "schema_version = 1\n[agent]\nid = \"fake_agent\"\nlabel = \"F\"\nkind = \"generic\"\nexperimental = true\n[binary]\nnames = [\"fake-agent\"]\nwindows_extensions = [\"exe\"]\n";
    let v = verify(&wizard, &id, generic);
    let c = codes(&v);
    assert!(c.contains(&"WIZARD_KIND".to_string()), "{c:?}");
    assert!(c.contains(&"WIZARD_NO_HEADLESS".to_string()), "{c:?}");
}

#[test]
fn a_parser_that_reads_garbage_as_an_answer_fails_the_shared_rules() {
    let tree = Tree::new("v-plain");
    let wizard = tree.wizard();
    let id = probed(&wizard);
    let plain = "schema_version = 1\n[agent]\nid = \"fake_agent\"\nlabel = \"F\"\nexperimental = true\n[binary]\nnames = [\"fake-agent\"]\nwindows_extensions = [\"exe\"]\n[headless]\nargv = [\"run\"]\ntask_separator = \"--\"\n[headless.output]\nformat = \"plain_text\"\n";
    let v = verify(&wizard, &id, plain);
    assert_eq!(v["ok"], false);
    let c = codes(&v);
    assert!(c.contains(&"CONFORMANCE_EXTRACT_RESULT_REJECTS_GARBAGE_AND_FAILED_EXITS".to_string()), "{c:?}");
    assert!(c.contains(&"FIXTURE_FAILED".to_string()), "{c:?}");
    let conf = v["errors"].as_array().unwrap().iter().find(|e| e["stage"] == "conformance").unwrap();
    assert!(conf["fix"].as_str().unwrap().contains("stricter"), "{}", conf["fix"]);
    let fixture = v["errors"].as_array().unwrap().iter().find(|e| e["stage"] == "fixture").unwrap();
    assert_eq!(fixture["path"], "headless_garbage.txt");
}

#[test]
fn risky_flags_are_warnings_for_the_review_screen() {
    let tree = Tree::new("v-warn");
    let wizard = tree.wizard();
    let id = probed(&wizard);
    let v = verify(&wizard, &id, &GOOD.replace("argv = [\"run\", \"--format\", \"json\"]", "argv = [\"run\", \"--format\", \"json\", \"--yolo\"]"));
    assert_eq!(v["ok"], true, "{}", v["text"]);
    assert!(v["warnings"].to_string().contains("--yolo"), "{}", v["warnings"]);
}

#[test]
fn a_draft_without_resume_gets_a_warning() {
    let tree = Tree::new("v-noresume");
    let wizard = tree.wizard();
    let id = probed(&wizard);
    let v = verify(&wizard, &id, MINIMAL);
    assert_eq!(v["ok"], true, "{}", v["text"]);
    assert!(v["warnings"].to_string().contains("no resume segment"), "{}", v["warnings"]);
}

#[test]
fn a_draft_over_the_size_limit_is_refused_early() {
    let tree = Tree::new("v-size");
    let wizard = tree.wizard();
    let id = probed(&wizard);
    let huge = format!("{GOOD}\n# {}\n", "x".repeat(70_000));
    let err = call(&wizard, "verify", json!({"session": id, "toml": huge})).unwrap_err();
    assert_eq!((err.status, err.code), (400, "AGENT_DEF_TOO_LARGE"));
    assert_eq!(call(&wizard, "verify", json!({"session": id})).unwrap_err().code, "INVALID_ARGUMENT");
}

#[test]
fn a_passing_verify_stores_exactly_that_text() {
    let tree = Tree::new("v-store");
    let wizard = tree.wizard();
    let id = probed(&wizard);
    verify(&wizard, &id, GOOD);
    let kept = wizard.read(&id, |s| s.draft.clone()).unwrap().unwrap();
    assert_eq!(kept.text, GOOD);
    // a failing draft does not replace it
    verify(&wizard, &id, NO_PATH);
    assert_eq!(wizard.read(&id, |s| s.draft.clone().unwrap().text).unwrap(), GOOD);
}

#[test]
fn verify_never_touches_the_agents_folder_or_leaves_preview_files() {
    let tree = Tree::new("v-clean");
    let wizard = tree.wizard();
    let id = probed(&wizard);
    verify(&wizard, &id, GOOD);
    assert!(!tree.agents.exists());
    let work = wizard.read(&id, |s| s.work_dir.clone()).unwrap();
    assert!(!work.join("preview").exists());
}
