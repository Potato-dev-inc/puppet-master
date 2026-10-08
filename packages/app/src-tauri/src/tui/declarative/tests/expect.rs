//! Walks every shipped definition with its fixture folder (`tests/agent-definitions/<id>.toml` ->
//! `fixtures/<id>/`), and checks the runner reports problems.

use std::path::{Path, PathBuf};

use crate::tui::declarative::check::{check_dir, check_file};

const CODE_IDS: &[&str] = &["claude", "codex"];

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn all_fixtures_meet_expectations() {
    let defs = manifest().join("tests/agent-definitions");
    let mut checked = 0;
    for entry in std::fs::read_dir(&defs).unwrap().flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        let dir = manifest().join("src/tui/declarative/fixtures").join(path.file_stem().unwrap());
        let results = check_file(&path, Some(&dir), CODE_IDS).unwrap();
        assert!(!results.is_empty(), "{}: no fixtures in {}", path.display(), dir.display());
        for r in &results {
            assert!(r.passed, "{}: {} -> {:?}", path.display(), r.fixture, r.failures);
        }
        checked += results.len();
    }
    assert!(checked >= 9, "kilo's nine captures are all described, got {checked}");
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pm-expect-{name}-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn kilo_profile_path() -> PathBuf {
    manifest().join("tests/agent-definitions/kilo.toml")
}

#[test]
fn missing_expect_file_is_reported() {
    let dir = scratch("missing");
    std::fs::write(dir.join("headless_x.jsonl"), "{}\n").unwrap();
    let results = check_file(&kilo_profile_path(), Some(&dir), CODE_IDS).unwrap();
    assert_eq!(results.len(), 1);
    assert!(!results[0].passed);
    assert!(results[0].failures[0].contains("no expectation file"), "{:?}", results[0].failures);
}

#[test]
fn a_wrong_expectation_fails_with_the_field_named() {
    let dir = scratch("wrong");
    std::fs::write(dir.join("headless_x.jsonl"), "not json\n").unwrap();
    std::fs::write(dir.join("headless_x.expect.toml"), "result = \"pong\"\ndone = true\n").unwrap();
    let results = check_file(&kilo_profile_path(), Some(&dir), CODE_IDS).unwrap();
    assert!(!results[0].passed);
    let all = results[0].failures.join("|");
    assert!(all.contains("result:") && all.contains("done:"), "{all}");
}

#[test]
fn expect_file_without_a_capture_and_unreadable_folder_are_reported() {
    let dir = scratch("orphan");
    std::fs::write(dir.join("tui_gone.expect.toml"), "busy = true\n").unwrap();
    let results = check_file(&kilo_profile_path(), Some(&dir), CODE_IDS).unwrap();
    assert!(results[0].failures[0].contains("no capture"));
    let none = check_file(&kilo_profile_path(), Some(Path::new("Z:/definitely/not/here")), CODE_IDS).unwrap();
    assert!(!none[0].passed);
}

#[test]
fn an_invalid_definition_is_an_error_not_a_pass() {
    let dir = scratch("bad");
    let bad = dir.join("bad.toml");
    std::fs::write(&bad, "schema_version = 1\nnonsense = 1\n").unwrap();
    assert!(check_file(&bad, Some(&dir), CODE_IDS).is_err());
    let _ = check_dir; // runner is also usable with an already built profile
}
