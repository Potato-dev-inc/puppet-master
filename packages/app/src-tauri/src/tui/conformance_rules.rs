//! Rules that hold for any `TuiProfile`, code or declarative, as checks that report instead of
//! asserting so a deliberately bad definition can be shown to fail them
//! (`declarative::tests::conformance`). The agent wizard runs the same rules on a draft definition
//! (`declarative::wizard::verify`), so this file is compiled in release builds too; the test-only
//! part (the subject lists and the tests) stays in `conformance.rs`.

use super::*;
use std::time::Duration;

pub type Rule = fn(&dyn TuiProfile) -> Result<(), String>;

/// Every rule by name (the bad-definition fixtures name the rule they must fail).
pub const ALL: &[(&str, Rule)] = &[
    ("feature_flags_agree_with_methods", feature_flags_agree_with_methods),
    ("extract_result_rejects_garbage_and_failed_exits", extract_result_rejects_garbage_and_failed_exits),
    ("prompts_round_trip", prompts_round_trip),
    ("empty_screen_never_blocked", empty_screen_never_blocked),
    ("idle_windows_are_sane", idle_windows_are_sane),
    ("screen_methods_survive_huge_input", screen_methods_survive_huge_input),
    ("submit_and_startup_declarations_are_sane", submit_and_startup_declarations_are_sane),
    ("blank_screen_is_unknown", blank_screen_is_unknown),
];

/// Readiness ratchet (`agent_runs::readiness`): a blank screen is never an input box and its
/// verdict is `Unknown`, so nothing is ever typed into a pane that has drawn nothing yet.
pub fn blank_screen_is_unknown(profile: &dyn TuiProfile) -> Result<(), String> {
    use crate::agent_runs::readiness::{assess, Observation, Readiness};
    let id = profile.id();
    for blank in ["", "\n\n", "   \n"] {
        if matches!(profile.ready_recogniser(blank), Some(ComposerState::Empty | ComposerState::HasText)) {
            return Err(format!("{id}: a blank screen {blank:?} reads as an input box"));
        }
        let verdict = assess(profile, Observation::screen_only(blank.to_string()));
        if verdict.readiness() != &Readiness::Unknown {
            return Err(format!("{id}: a blank screen {blank:?} is {:?}, not Unknown", verdict.readiness()));
        }
    }
    Ok(())
}

pub fn feature_flags_agree_with_methods(profile: &dyn TuiProfile) -> Result<(), String> {
    let id = profile.id();
    let features = profile.features();
    let launch = HeadlessLaunch::new(".", "task");
    if features.read_only_headless {
        // A CLI missing from PATH is an environment error; "unsupported" is a contract failure.
        if let Err(error) = profile.headless_command(&launch) {
            if error.contains("unsupported") {
                return Err(format!("{id}: read_only_headless needs a headless command: {error}"));
            }
        }
    }
    if features.read_only_headless && profile.is_installed() && profile.headless_command(&launch).is_err() {
        return Err(format!("{id}: is_installed() is true but the headless launch fails"));
    }
    if features.resumable_headless && features.kind != Kind::Cli {
        return Err(format!("{id}: only CLI backends resume headless sessions"));
    }
    if !features.dispatchable && profile.not_dispatchable_reason().is_empty() {
        return Err(format!("{id}: a non-dispatchable profile must say why"));
    }
    if features.kind != Kind::Shell && profile.prompt_visible("anything") {
        return Err(format!("{id}: prompt_visible is for shells"));
    }
    Ok(())
}

pub fn extract_result_rejects_garbage_and_failed_exits(profile: &dyn TuiProfile) -> Result<(), String> {
    let id = profile.id();
    if profile.extract_result("not json at all", 0).is_some() {
        return Err(format!("{id}: garbage output produced a result"));
    }
    if profile.extract_result("{}", 1).is_some() {
        return Err(format!("{id}: a failed exit produced a result"));
    }
    if profile.extract_provider_session("not json").is_some() {
        return Err(format!("{id}: garbage produced a provider session"));
    }
    if profile.detect_headless_trust("unrelated failure").is_some() {
        return Err(format!("{id}: unrelated failure detected as a trust screen"));
    }
    if profile.rejected_call_warning("all good").is_some() {
        return Err(format!("{id}: clean output produced a rejected-call warning"));
    }
    Ok(())
}

/// Whatever a profile detects, it must answer with at least one advertised choice.
pub fn prompts_round_trip(profile: &dyn TuiProfile) -> Result<(), String> {
    let id = profile.id();
    let screens = [
        "Allow once?\n",
        "Trust this workspace?\n",
        "Run this command?\n",
        "Approve this command?\n",
        "Always allow this command?\n",
        "permission required\n",
    ];
    for screen in screens {
        for allow_broad in [false, true] {
            let Some(prompt) = profile.detect_prompt(screen, allow_broad) else { continue };
            let Some(choices) = prompt["choices"].as_array() else {
                return Err(format!("{id}: prompt without a choices array for {screen:?}"));
            };
            if choices.is_empty() {
                return Err(format!("{id}: prompt with no choices for {screen:?}"));
            }
            let answerable = choices
                .iter()
                .filter_map(Value::as_str)
                .any(|choice| profile.prompt_reply(&prompt, choice, allow_broad).is_ok());
            if !answerable {
                return Err(format!("{id}: no advertised choice produces a reply for {screen:?}"));
            }
            if prompt["agent_type"] != id {
                return Err(format!("{id}: prompt agent_type is {}", prompt["agent_type"]));
            }
            // SEC-7: allow_all is never accepted without allow_broad.
            if !allow_broad && profile.prompt_reply(&prompt, "allow_all", false).is_ok() {
                return Err(format!("{id}: allow_all accepted without allow_broad"));
            }
        }
    }
    Ok(())
}

pub fn empty_screen_never_blocked(profile: &dyn TuiProfile) -> Result<(), String> {
    let id = profile.id();
    for blank in ["", "\n\n", "   \n"] {
        if profile.detect_prompt(blank, true).is_some() {
            return Err(format!("{id}: a blank screen {blank:?} reads as a prompt"));
        }
        if profile.classify_screen(blank) == Some(PaneStatus::WaitingInput) {
            return Err(format!("{id}: a blank screen {blank:?} reads as waiting_input"));
        }
        if profile.standby_screen(blank) == StandbyScreen::Dialog {
            return Err(format!("{id}: a blank screen {blank:?} reads as a dialog"));
        }
    }
    Ok(())
}

pub fn idle_windows_are_sane(profile: &dyn TuiProfile) -> Result<(), String> {
    let id = profile.id();
    let window = profile.idle_after();
    if window < Duration::from_millis(1500) {
        return Err(format!("{id}: {window:?} is too short to ride out a quiet spinner frame"));
    }
    if window > Duration::from_secs(5) {
        return Err(format!("{id}: {window:?} is longer than the default"));
    }
    let quiet = profile.quiet_idle_after();
    if quiet > window {
        return Err(format!("{id}: the quiet window {quiet:?} must not exceed {window:?}"));
    }
    if quiet < Duration::from_millis(500) {
        return Err(format!("{id}: {quiet:?} would flip between spinner frames"));
    }
    Ok(())
}

/// A 300 KiB screen must not panic or hang any screen method.
pub fn screen_methods_survive_huge_input(profile: &dyn TuiProfile) -> Result<(), String> {
    let huge = "a line of screen text \u{2022} \u{1b}[31mred\u{1b}[0m\n".repeat(7000);
    let _ = profile.busy_marker(&huge);
    let _ = profile.classify_screen(&huge);
    let _ = profile.detect_prompt(&huge, false);
    let _ = profile.looks_like_menu(&huge);
    let _ = profile.strip_dialogs(&huge);
    let _ = profile.tidy_reply(&huge);
    let _ = profile.detect_footer_model(&huge);
    let _ = profile.standby_screen(&huge);
    let _ = profile.is_chrome_line(&huge);
    let _ = profile.is_tool_activity_line(&huge);
    let _ = profile.is_thinking_line(&huge);
    let _ = profile.startup_gate(&huge);
    let _ = profile.composer_state(&huge);
    Ok(())
}

/// The submit sequence, composer reader and startup gates a profile declares:
/// - Enter goes out within a few seconds of the text and is a real key sequence;
/// - a blank screen is no startup gate and no input box with text or a running turn;
/// - a gate's dismiss keys never include Enter (Enter picks the highlighted option, which for
///   an update menu is "Update now" and for a trust question is "Yes").
pub fn submit_and_startup_declarations_are_sane(profile: &dyn TuiProfile) -> Result<(), String> {
    let id = profile.id();
    let submit = profile.submit_sequence();
    if submit.keys.is_empty() {
        return Err(format!("{id}: the submit sequence writes nothing"));
    }
    if submit.settle > Duration::from_secs(3) {
        return Err(format!("{id}: a {:?} pause before Enter would stall every task", submit.settle));
    }
    for blank in ["", "\n\n", "   \n"] {
        if let Some(gate) = profile.startup_gate(blank) {
            return Err(format!("{id}: a blank screen {blank:?} reads as the startup gate {}", gate.id));
        }
        if matches!(profile.composer_state(blank), ComposerState::HasText | ComposerState::Working) {
            return Err(format!("{id}: a blank screen {blank:?} reads as a busy input box"));
        }
    }
    Ok(())
}
