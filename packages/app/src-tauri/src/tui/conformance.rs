//! Rules every registered profile must satisfy. A new TUI gets these checks for free by being
//! added to the registry; a failure here means the profile contradicts itself or the registry.

use super::*;
use std::collections::HashSet;

/// Rules that hold for any `TuiProfile`, code or declarative, as checks that report instead of
/// asserting so a deliberately bad definition can be shown to fail them
/// (`declarative::tests::conformance`).
pub(crate) mod rules {
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
    ];

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
        Ok(())
    }

    /// Code profiles plus every test definition under `declarative/fixtures/definitions/`.
    pub fn subjects() -> Vec<Arc<dyn TuiProfile>> {
        let mut all = profiles();
        all.extend(super::definition_subjects());
        all
    }
}

/// One `DeclarativeProfile` per file in `declarative/fixtures/definitions/`, sorted by name; each
/// must parse, validate clean and build.
pub(crate) fn definition_subjects() -> Vec<Arc<dyn TuiProfile>> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tui/declarative/fixtures/definitions");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .collect();
    files.sort();
    assert!(files.len() >= 3, "kilo-like, minimal-tui and generic test definitions");
    files
        .iter()
        .map(|path| {
            let text = std::fs::read_to_string(path).unwrap();
            let built = declarative::build_for_test(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            Arc::new(built) as Arc<dyn TuiProfile>
        })
        .collect()
}

fn check_all(rule: &str, run: rules::Rule) {
    for profile in rules::subjects() {
        run(&*profile).unwrap_or_else(|e| panic!("rule {rule}: {e}"));
    }
}

#[test]
fn ids_are_unique_and_lowercase() {
    let mut seen = HashSet::new();
    for profile in profiles() {
        let id = profile.id();
        assert_eq!(id, id.to_ascii_lowercase(), "{id}: ids are lowercase");
        assert!(seen.insert(id), "{id}: registered twice");
    }
}

/// Every code profile id resolves through the registry gate (`require_registered`) and the
/// hot-path lookup (`profile_for`) to itself, in any letter case.
#[test]
fn every_code_id_resolves_in_registry() {
    let code = registry::Registry::seeded().ordered();
    assert_eq!(code.len(), 9, "nine code profiles");
    for profile in code {
        let id = profile.id();
        for spelling in [id.to_string(), id.to_ascii_uppercase()] {
            let resolved = require_registered(&spelling)
                .unwrap_or_else(|unknown| panic!("{id}: not registered ({unknown:?})"));
            assert_eq!(resolved.id(), id, "{spelling}: gate resolves to the canonical id");
            assert_eq!(profile_for(&spelling).id(), id, "{spelling}: profile_for agrees");
            assert!(is_registered(&spelling), "{spelling}: is_registered agrees");
        }
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
    check_all("feature_flags_agree_with_methods", rules::feature_flags_agree_with_methods);
    for profile in profiles() {
        if profile.features().kind == Kind::Shell {
            let id = profile.id();
            assert!(!profile.features().dispatchable, "{id}: shells are not agent backends");
            assert!(!is_agent_backend(id));
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
    check_all("extract_result_rejects_garbage_and_failed_exits", rules::extract_result_rejects_garbage_and_failed_exits);
}

#[test]
fn prompts_round_trip_through_their_own_reply() {
    check_all("prompts_round_trip", rules::prompts_round_trip);
}

#[test]
fn an_empty_screen_is_never_blocked() {
    check_all("empty_screen_never_blocked", rules::empty_screen_never_blocked);
}

#[test]
fn screen_methods_survive_huge_input() {
    check_all("screen_methods_survive_huge_input", rules::screen_methods_survive_huge_input);
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
    check_all("idle_windows_are_sane", rules::idle_windows_are_sane);
}

#[test]
fn scoped_dispatchers_match_profile_for_each_registered_id() {
    const PROBE: &str = "probe line\n";
    const LOWER: &str = "probe line";
    for profile in profiles() {
        let id = profile.id();
        assert_eq!(
            is_chrome_line(LOWER, id),
            profile.is_chrome_line(LOWER),
            "{id}: is_chrome_line"
        );
        assert_eq!(
            is_tool_activity_line(LOWER, id),
            profile.is_tool_activity_line(LOWER),
            "{id}: is_tool_activity_line"
        );
        assert_eq!(
            strip_dialogs(PROBE, id),
            profile.strip_dialogs(PROBE),
            "{id}: strip_dialogs"
        );
        assert_eq!(
            tidy_reply(PROBE, id),
            profile.tidy_reply(PROBE),
            "{id}: tidy_reply"
        );
        assert_eq!(
            looks_like_menu(PROBE, id),
            profile.looks_like_menu(PROBE),
            "{id}: looks_like_menu"
        );
    }
}

/// Claude's finished-turn summary row (`claude/screen.rs`); Cursor's idle placeholder
/// (`cursor/chrome/tests.rs`) is chrome only for `cursor_agent`.
#[test]
fn scoping_chrome_rule_of_a_does_not_touch_b() {
    let claude_done = "\u{273b} churned for 16s \u{b7} done 23:03";
    assert!(is_chrome_line(claude_done, "claude"));
    assert!(!is_chrome_line(claude_done, "cursor_agent"));

    let cursor_placeholder = "→ plan, search, build anything";
    assert!(is_chrome_line(cursor_placeholder, "cursor_agent"));
    assert!(!is_chrome_line(cursor_placeholder, "claude"));
}

#[test]
fn scoping_strip_dialogs_only_runs_selected_profile() {
    const TRUST: &str = include_str!("cursor/fixtures/workspace_trust_visible_resolved.txt");
    let claude_stripped = strip_dialogs(TRUST, "claude");
    assert!(
        claude_stripped.contains("Workspace Trust Required"),
        "claude profile must not run Cursor dialog stripping"
    );
    let cursor_stripped = strip_dialogs(TRUST, "cursor_agent");
    assert!(
        !cursor_stripped.contains("Workspace Trust Required"),
        "{cursor_stripped}"
    );
}

/// Same golden expectations as `cursor/chrome/tests.rs` on the trust capture, via the scoped dispatcher.
#[test]
fn cursor_fixture_replays_scoped_strip_dialogs() {
    const TRUST: &str = include_str!("cursor/fixtures/workspace_trust_visible_resolved.txt");
    let stripped = strip_dialogs(TRUST, "cursor_agent");
    assert!(!stripped.contains("Workspace Trust Required"), "{stripped}");
    assert!(!stripped.contains("Trust this workspace"), "{stripped}");
    assert!(
        stripped.contains("Plan, search, build anything"),
        "TUI below the dialog stays"
    );
}

/// Same expectations as `opencode/screen.rs` menu test, via the scoped dispatcher.
#[test]
fn opencode_fixture_replays_scoped_looks_like_menu() {
    let menu =
        "1. Yes\n2. No\n3. Type your own answer\n↑↓ select  enter submit  esc dismiss";
    assert!(looks_like_menu(menu, "opencode"));
    assert!(looks_like_menu(menu, "opencode_native"));
    assert!(!looks_like_menu(menu, "claude"));
    assert!(!looks_like_menu("just some output", "opencode"));
}
