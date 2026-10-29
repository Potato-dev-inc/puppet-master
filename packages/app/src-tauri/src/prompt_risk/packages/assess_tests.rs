//! The look-alike finding of `prompt_risk::assess` (SPEC-n33 section 3.3, risk) and Package check's S1 / S13 source scans.

use crate::prompt_risk::{assess, Risk, RiskInput, RiskLevel};

fn risk_of(command: &str) -> Risk {
    assess(
        &RiskInput { kind: "command_approval", command: Some(command), workspace: Some("/home/dev/app"), ..Default::default() },
        &[],
    )
}

#[test]
fn a_look_alike_install_is_high_with_the_exact_words() {
    let risk = risk_of("npm install lodahs");
    assert_eq!(risk.level, RiskLevel::High);
    assert_eq!(risk.headline, "Install a package with a look-alike name");
    assert_eq!(
        risk.reasons.first().map(String::as_str),
        Some("lodahs looks like the popular package lodash (2 letters swapped). Check the name before you allow it.")
    );
}

#[test]
fn a_popular_name_keeps_the_plain_install_risk() {
    let risk = risk_of("npm install lodash");
    assert_eq!(risk.level, RiskLevel::Medium);
    assert_eq!(risk.headline, "Install packages for this project");
}

#[test]
fn a_look_alike_beats_a_global_install_headline() {
    // Rank 72 is above GLOBAL_INSTALL (70): the look-alike is the headline, the global install stays a reason.
    let risk = risk_of("npm i -g lodahs");
    assert_eq!(risk.level, RiskLevel::High);
    assert_eq!(risk.headline, "Install a package with a look-alike name");
    assert!(risk.reasons.iter().any(|r| r.contains("every project on this computer")), "{:?}", risk.reasons);
}

#[test]
fn a_look_alike_is_not_looked_for_without_a_command() {
    let risk = assess(&RiskInput { kind: "workspace_trust", text: "npm install lodahs", ..Default::default() }, &[]);
    assert_eq!(risk.headline, "Trust this folder");
}

#[test]
fn publishing_still_outranks_a_look_alike() {
    let risk = risk_of("npm install lodahs && npm publish");
    assert_eq!(risk.headline, "Publish to a public registry");
}

#[test]
fn a_cut_name_is_never_judged_by_assess_alone() {
    // `assess` has no screen: a name that is merely unusual is not a look-alike.
    assert_eq!(risk_of("npm install some-unusual-name-here").level, RiskLevel::Medium);
}

/// Words that would write to a pane or answer a prompt (S1). Built from pieces so this file does not contain them.
fn write_words() -> Vec<String> {
    [("write", "_input"), ("send", "_keys"), ("press", "_key"), ("Typing", "Permit"), ("Gate", "Permit"), ("answer_pane", "_prompt"), ("reply", "_for"), ("answer", "_prompt")]
        .iter()
        .map(|(a, b)| format!("{a}{b}"))
        .collect()
}

/// Every non-test source file of the two Package check modules, as (name, text).
fn sources() -> Vec<(String, String)> {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = Vec::new();
    for dir in ["prompt_risk/packages", "package_check"] {
        let mut stack = vec![src.join(dir)];
        while let Some(path) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&path) else { continue };
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|e| e == "rs") {
                    let name = p.file_name().unwrap().to_string_lossy().to_string();
                    // Test files are not shipped code; the unit-test tails of shipped files are cut below.
                    if name.ends_with("_tests.rs") {
                        continue;
                    }
                    let text = std::fs::read_to_string(&p).unwrap_or_default();
                    out.push((format!("{dir}/{name}"), shipped_part(&text)));
                }
            }
        }
    }
    assert!(out.len() >= 15, "expected the Package check sources, found {}", out.len());
    out
}

/// The text before a file's `#[cfg(test)]` module.
fn shipped_part(text: &str) -> String {
    match text.find("#[cfg(test)]") {
        Some(at) => text[..at].to_string(),
        None => text.to_string(),
    }
}

#[test]
fn s1_no_write_path_in_either_module() {
    for (name, text) in sources() {
        for word in write_words() {
            assert!(!text.contains(&word), "{name} must not contain {word}");
        }
    }
}

#[test]
fn s13_generic_code_names_no_agent() {
    for (name, text) in sources() {
        let lower = text.to_ascii_lowercase();
        for agent in ["claude", "codex", "cursor", "opencode", "agy", "gemini"] {
            // `agy` also appears inside ordinary words ("legacy", "agyness" do not exist in our sources), so match whole words.
            let hit = lower
                .split(|c: char| !c.is_ascii_alphanumeric())
                .any(|w| w == agent || (agent != "agy" && w.starts_with(agent)));
            assert!(!hit, "{name} must not name an agent: {agent}");
        }
    }
}
