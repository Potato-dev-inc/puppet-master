//! Rules every registered profile must satisfy. A new TUI gets these checks for free by being
//! added to `PROFILES`; a failure here means the profile contradicts itself or the registry.

use super::*;
use std::collections::HashSet;

#[test]
fn ids_are_unique_lowercase_and_known_to_agent_type() {
    let mut seen = HashSet::new();
    for profile in profiles() {
        let id = profile.id();
        assert_eq!(id, id.to_ascii_lowercase(), "{id}: ids are lowercase");
        assert!(seen.insert(id), "{id}: registered twice");
        let agent = crate::pty::agents::AgentType::parse(id)
            .unwrap_or_else(|| panic!("{id}: add an AgentType variant for this profile"));
        assert_eq!(agent.as_str(), id, "{id}: AgentType::as_str must round-trip");
    }
}

#[test]
fn every_agent_type_has_a_profile() {
    use crate::pty::agents::AgentType;
    for agent in [
        AgentType::Claude,
        AgentType::Codex,
        AgentType::Opencode,
        AgentType::OpencodeNative,
        AgentType::Cmd,
        AgentType::Powershell,
        AgentType::Bash,
        AgentType::Cursor,
        AgentType::CursorAgent,
    ] {
        assert!(
            is_registered(agent.as_str()),
            "{}: AgentType has no TuiProfile",
            agent.as_str()
        );
    }
}

#[test]
fn lookup_is_case_insensitive_and_unknown_is_generic() {
    assert_eq!(profile_for("CURSOR_AGENT").id(), "cursor_agent");
    assert_eq!(profile_for("no-such-agent").features().kind, Kind::Unknown);
    assert!(!is_registered("no-such-agent"));
    assert!(!is_agent_backend("no-such-agent"));
}

#[test]
fn feature_flags_agree_with_the_methods_they_promise() {
    for profile in profiles() {
        let id = profile.id();
        let features = profile.features();
        let launch = HeadlessLaunch::new(".", "task");

        if features.read_only_headless {
            // A CLI missing from PATH is an environment error; "unsupported" is a contract failure.
            if let Err(error) = profile.headless_command(&launch) {
                assert!(!error.contains("unsupported"), "{id}: read_only_headless needs a headless command: {error}");
            }
        }
        if features.read_only_headless && profile.is_installed() {
            // is_installed must agree with the launcher: a CLI it calls installed must launch.
            assert!(
                profile.headless_command(&launch).is_ok(),
                "{id}: is_installed() is true but the headless launch fails"
            );
        }
        if features.resumable_headless {
            assert!(
                features.kind == Kind::Cli,
                "{id}: only CLI backends resume headless sessions"
            );
        }
        if !features.dispatchable {
            assert!(
                !profile.not_dispatchable_reason().is_empty(),
                "{id}: a non-dispatchable profile must say why"
            );
        }
        if features.kind == Kind::Shell {
            assert!(!features.dispatchable, "{id}: shells are not agent backends");
            assert!(!is_agent_backend(id));
        } else {
            assert!(
                !profile.prompt_visible("anything"),
                "{id}: prompt_visible is for shells"
            );
        }
    }
}

#[test]
fn headless_launch_enforces_the_shared_policy() {
    for profile in profiles() {
        let id = profile.id();
        let read_only = HeadlessLaunch {
            read_only: true,
            ..HeadlessLaunch::new(".", "task")
        };
        let result = headless_launch(id, &read_only);
        if !profile.features().read_only_headless {
            let error = result.expect_err(id);
            assert!(error.starts_with("READ_ONLY_UNSUPPORTED:"), "{id}: {error}");
        }
        let conflicting = HeadlessLaunch {
            read_only: true,
            allow_shell: true,
            ..HeadlessLaunch::new(".", "task")
        };
        assert!(headless_launch(id, &conflicting).is_err(), "{id}");
    }
}

#[test]
fn output_parsers_reject_garbage_and_failed_exits() {
    for profile in profiles() {
        let id = profile.id();
        assert_eq!(profile.extract_result("not json at all", 0), None, "{id}");
        assert_eq!(profile.extract_result("{}", 1), None, "{id}");
        assert_eq!(profile.extract_provider_session("not json"), None, "{id}");
        assert!(profile.detect_headless_trust("unrelated failure").is_none(), "{id}");
        assert!(profile.rejected_call_warning("all good").is_none(), "{id}");
    }
}

#[test]
fn prompts_round_trip_through_their_own_reply() {
    // Whatever a profile detects, it must be able to answer with at least one advertised choice.
    let screens = [
        "Allow once?\n",
        "Trust this workspace?\n",
        "Run this command?\n",
        "Approve this command?\n",
    ];
    for profile in profiles() {
        let id = profile.id();
        for screen in screens {
            let Some(prompt) = profile.detect_prompt(screen, false) else {
                continue;
            };
            let choices = prompt["choices"].as_array().expect("choices array");
            assert!(!choices.is_empty(), "{id}: prompt with no choices");
            assert!(
                choices
                    .iter()
                    .filter_map(Value::as_str)
                    .any(|choice| profile.prompt_reply(&prompt, choice, false).is_ok()),
                "{id}: no advertised choice produces a reply for {screen:?}"
            );
            assert_eq!(prompt["agent_type"], id, "{id}");
        }
    }
}

#[test]
fn an_empty_screen_is_never_blocked() {
    for profile in profiles() {
        let id = profile.id();
        assert!(profile.detect_prompt("", true).is_none(), "{id}");
        if let Some(status) = profile.classify_screen("") {
            assert_ne!(status, PaneStatus::WaitingInput, "{id}");
        }
    }
}

#[test]
fn shell_helper_agrees_with_kind() {
    for profile in profiles() {
        let id = profile.id();
        assert_eq!(is_shell(id), profile.features().kind == Kind::Shell, "{id}");
    }
}

#[test]
fn idle_windows_are_sane_and_only_animated_tuis_shorten_them() {
    use std::time::Duration;
    for profile in profiles() {
        let id = profile.id();
        let window = profile.idle_after();
        assert!(window >= Duration::from_millis(1500), "{id}: {window:?} is too short to ride out a quiet spinner frame");
        assert!(window <= Duration::from_secs(5), "{id}: {window:?} is longer than the default");
    }
    // Cursor's spinner repaints every ~250ms, which is what justifies its shorter window.
    assert!(crate::tui::profile_for("cursor_agent").idle_after() < crate::pty::registry::DEFAULT_IDLE_AFTER);
}
