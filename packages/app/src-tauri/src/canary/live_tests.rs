//! Live checks against the real CLIs installed on this machine. Not part of the normal run: they
//! start `codex` / `claude` on a hidden PTY in a scratch folder. They send NO prompt, so they spend
//! no tokens. Run with
//!
//! `cargo test --lib canary_live -- --ignored --nocapture --test-threads=1`
//!
//! `PM_CANARY_SCRATCH` picks the scratch root (default `D:/pm-scratch-canary`). The result is also
//! written to `PM_CANARY_OUT` (default `D:/pm-night/p0/canary/live`) as JSON.

use super::live::{run_live, scratch_dir};
use super::model::Outcome;

fn live(agent: &str) {
    if std::env::var_os("PM_CANARY_SCRATCH").is_none() {
        std::env::set_var("PM_CANARY_SCRATCH", "D:/pm-scratch-canary");
    }
    let source = crate::agent_health::collect_probes()
        .into_iter()
        .find(|p| p.id == agent)
        .map(|probe| super::engine::Source {
            facts: super::model::Facts {
                id: probe.id.clone(),
                label: probe.label.clone(),
                installed: probe.installed,
                tested: crate::tui::profile_for(&probe.id).tested_versions().map(|r| r.as_str().to_string()),
            },
            version_command: probe.version_command,
        })
        .unwrap_or_else(|| panic!("{agent} is not registered"));
    assert!(source.facts.installed, "{agent} is not installed on this machine");
    let version = source
        .version_command
        .as_ref()
        .and_then(|c| match crate::agent_health::run_version(c, crate::agent_health::VERSION_TIMEOUT) {
            crate::agent_health::VersionOutcome::Version(v) => Some(v),
            _ => None,
        });
    println!("{agent}: version {version:?}, scratch {}", scratch_dir(agent).display());
    let result = run_live(&source, version.clone());
    for shot in &result.screens {
        println!("--- screen {} ---\n{}\n", shot.label, shot.screen);
    }
    println!("outcome {:?}, dismissed {:?}, keys sent {:?}, prompts sent {}", result.outcome, result.dismissed, result.keys_sent, result.prompts_sent);
    let out = std::env::var("PM_CANARY_OUT").unwrap_or_else(|_| "D:/pm-night/p0/canary/live".into());
    let _ = std::fs::create_dir_all(&out);
    let _ = std::fs::write(format!("{out}/{agent}.json"), serde_json::to_vec_pretty(&result).unwrap());
    assert_eq!(result.prompts_sent, 0);
    // Only gate dismiss keys may ever have been written.
    assert!(result.keys_sent.iter().all(|k| result.dismissed.iter().any(|d| k.starts_with(d.as_str()))), "{:?}", result.keys_sent);
    assert!(result.outcome.passed(), "{:?}: {}", result.outcome, result.summary);
    if result.outcome == Outcome::AsksYou {
        println!("NOTE: stopped at a question only the person may answer: {:?}", result.gate);
    }
}

#[test]
#[ignore = "live: starts the real codex CLI; sends no prompt"]
fn canary_live_codex_reaches_ready_without_a_turn() {
    live("codex");
}

#[test]
#[ignore = "live: starts the real claude CLI; sends no prompt"]
fn canary_live_claude_reaches_ready_without_a_turn() {
    live("claude");
}
