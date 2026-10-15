//! `tui::conformance` rules run over the test definitions; deliberately bad definitions must fail
//! exactly the rules they name.

use std::path::Path;

use crate::tui::conformance::{definition_subjects, rules};
use crate::tui::declarative::build_for_test;

fn bad_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tui/declarative/fixtures/definitions-bad")
}

#[test]
fn every_rule_holds_for_every_test_definition() {
    for profile in definition_subjects() {
        for (name, rule) in rules::ALL {
            rule(&*profile).unwrap_or_else(|e| panic!("{}: rule {name}: {e}", profile.id()));
        }
    }
}

#[test]
fn extract_result_rejects_garbage_and_failed_exits() {
    for profile in definition_subjects() {
        rules::extract_result_rejects_garbage_and_failed_exits(&*profile).unwrap_or_else(|e| panic!("{e}"));
    }
}

#[test]
fn prompts_round_trip() {
    let profiles = definition_subjects();
    let mut detected = 0;
    for profile in &profiles {
        rules::prompts_round_trip(&**profile).unwrap_or_else(|e| panic!("{e}"));
        detected += usize::from(profile.detect_prompt("Allow once?\n", false).is_some());
    }
    assert!(detected >= 1, "at least one test definition detects a prompt, so the rule is not vacuous");
}

#[test]
fn empty_screen_never_blocked() {
    for profile in definition_subjects() {
        rules::empty_screen_never_blocked(&*profile).unwrap_or_else(|e| panic!("{e}"));
        assert!(profile.detect_prompt("", true).is_none(), "{}", profile.id());
    }
}

#[test]
fn bad_definitions_fail_expected_rules() {
    let mut files: Vec<_> = std::fs::read_dir(bad_dir())
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .collect();
    files.sort();
    assert!(files.len() >= 4, "{files:?}");
    for path in files {
        let text = std::fs::read_to_string(&path).unwrap();
        let expected: Vec<String> = text
            .lines()
            .find_map(|l| l.strip_prefix("# expect:"))
            .unwrap_or_else(|| panic!("{}: missing `# expect:` line", path.display()))
            .split(',')
            .map(|s| s.trim().to_string())
            .collect();
        let built = build_for_test(&text);
        if expected == ["validates"] {
            assert!(built.is_err(), "{}: expected the definition to be rejected", path.display());
            continue;
        }
        let profile = built.unwrap_or_else(|e| panic!("{}: should build: {e}", path.display()));
        for name in &expected {
            let (_, rule) = rules::ALL
                .iter()
                .find(|(n, _)| n == name)
                .unwrap_or_else(|| panic!("{}: unknown rule {name}", path.display()));
            assert!(rule(&profile).is_err(), "{}: rule {name} should fail", path.display());
        }
        // and a bad definition does not fail rules it is not about
        for (name, rule) in rules::ALL {
            if !expected.iter().any(|e| e == name) {
                rule(&profile).unwrap_or_else(|e| panic!("{}: unexpected failure of {name}: {e}", path.display()));
            }
        }
    }
}
