//! Rules every registered profile must satisfy. A new TUI gets these checks for free by being
//! added to the registry; a failure here means the profile contradicts itself or the registry.

use super::*;
use std::collections::HashSet;

/// The rules live in `conformance_rules.rs` (compiled in every build, the wizard uses them); the
/// subject lists are test-only.
pub(crate) mod rules {
    pub use crate::tui::conformance_rules::*;
    use crate::tui::{profiles, TuiProfile};
    use std::sync::Arc;

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
