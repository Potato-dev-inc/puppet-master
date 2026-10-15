//! Readiness verdicts and permits, on REAL captured screens (`tui/<agent>/fixtures/`), and the
//! ratchets: blank = `Unknown`, `Ready` and `Gate` never on the same screen, the process vetoes the
//! screen, and no readiness refusal ever falls back to a background run.

use super::*;
use crate::tui::profile_for;

const CLAUDE_FRESH: &str = include_str!("../tui/claude/fixtures/ready_fresh_2_1_299_live.txt");
const CLAUDE_AFTER_TURN: &str = include_str!("../tui/claude/fixtures/ready_after_turn_2_1_299_live.txt");
const CLAUDE_AFTER_FOLLOW_UP: &str = include_str!("../tui/claude/fixtures/ready_after_follow_up_2_1_299_live.txt");
const CLAUDE_WORKING: &str = include_str!("../tui/claude/fixtures/working_2_1_299_live.txt");
const CLAUDE_DRAFT: &str = include_str!("../tui/claude/fixtures/draft_2_1_299_live.txt");
const CLAUDE_KILLED: &str = include_str!("../tui/claude/fixtures/killed_host_shell_2_1_299_live.txt");
const CLAUDE_TRUST: &str = include_str!("../tui/claude/fixtures/trust_dialog_2_1_299.txt");
const CLAUDE_TRUST_CARD: &str = include_str!("../tui/claude/fixtures/trust_card_verify_r7_live.txt");
const CLAUDE_DECLINED: &str = include_str!("../tui/claude/fixtures/trust_declined_host_shell.txt");
const CLAUDE_BASH_APPROVAL: &str = include_str!("../tui/claude/fixtures/bash_simple_4_options_2_1_299.txt");
const CLAUDE_SIDE_PANEL: &str = include_str!("../tui/claude/fixtures/side_diff_panel_screen.txt");
const NOT_INSTALLED: &str = include_str!("../tui/shell/fixtures/command_not_found_powershell_live.txt");
const SHELL_AFTER_EXIT: &str = include_str!("../tui/shell/fixtures/host_shell_after_agent_exit_live.txt");
const CODEX_READY: &str = include_str!("../tui/codex/fixtures/standby_ready_live.txt");
const CODEX_LOADING: &str = include_str!("../tui/codex/fixtures/loading_0_162_1_live.txt");
const CODEX_UPDATE_MENU: &str = include_str!("../tui/codex/fixtures/update_prompt_live.txt");
const CODEX_WORKING: &str = include_str!("../tui/codex/fixtures/working_live.txt");
const CODEX_UNSENT: &str = include_str!("../tui/codex/fixtures/composer_unsent_live.txt");
const CURSOR_FOLLOW_UP: &str = include_str!("../tui/cursor/fixtures/reply_pytest_final_screen.txt");
const CURSOR_FRESH: &str = include_str!("../tui/cursor/fixtures/workspace_trust_visible_resolved.txt");
const CURSOR_TRUST: &str = include_str!("../tui/cursor/fixtures/workspace_trust_visible_pending.txt");
const CURSOR_APPROVAL: &str = include_str!("../tui/cursor/fixtures/command_approval_visible.txt");

fn look(agent: &str, screen: &str, process: AgentProcess) -> Verdict {
    let observation = Observation {
        pane_id: "pane-1".into(),
        screen: screen.to_string(),
        output_pos: Some(100),
        agent: process,
    };
    assess(profile_for(agent).as_ref(), observation)
}

fn readiness_of(agent: &str, screen: &str) -> Readiness {
    look(agent, screen, AgentProcess::Alive).readiness().clone()
}

// ---- Claude: the recogniser that replaces the legacy readiness -----------------------------------

#[test]
fn claude_reads_ready_on_the_fresh_screen_and_after_every_turn() {
    for (name, screen) in [
        ("fresh", CLAUDE_FRESH),
        ("after a turn", CLAUDE_AFTER_TURN),
        ("after a follow-up", CLAUDE_AFTER_FOLLOW_UP),
    ] {
        let verdict = look("claude", screen, AgentProcess::Alive);
        assert_eq!(verdict.readiness(), &Readiness::Ready, "{name}: {}\n{screen}", verdict.why());
        assert_eq!(verdict.basis(), Basis::Recognised, "{name}");
        assert_eq!(verdict.composer(), ComposerState::Empty, "{name}");
    }
}

#[test]
fn claude_follow_up_screen_has_no_banner_and_is_still_ready() {
    // The old standby rule needed "Claude Code" on screen; after a turn scrolls the banner away a
    // follow-up never read ready (why Claude was left on the legacy readiness in round 7). The
    // rule glued in front of the prompt row (a soft wrap) still counts as the box's top rule.
    let follow_up = "\u{25cf} The first line of output is /d/pm-demo.\n\u{273b} Cooked for 3s \u{b7} done 11:21\n\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{276f}\n\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n\u{23f8} manual mode on \u{b7} ? for shortcuts \u{b7} \u{2190} for agents";
    assert!(!follow_up.contains("Claude Code"));
    assert_eq!(readiness_of("claude", follow_up), Readiness::Ready);
}

#[test]
fn claude_working_is_busy_and_a_draft_is_never_typed_into() {
    assert_eq!(readiness_of("claude", CLAUDE_WORKING), Readiness::Busy);
    // Claude has no clear key (Ctrl+U is typed as an invisible character): a box that holds
    // someone's text is not Ready, the task would be glued to it.
    let draft = look("claude", CLAUDE_DRAFT, AgentProcess::Alive);
    assert_eq!(draft.composer(), ComposerState::HasText);
    assert_eq!(draft.readiness(), &Readiness::Unknown, "{}", draft.why());
    // The side-panel capture has "commit this" typed in the box.
    assert_eq!(profile_for("claude").ready_recogniser(CLAUDE_SIDE_PANEL), Some(ComposerState::HasText));
}

#[test]
fn claude_dialogs_are_gates_not_input_boxes() {
    for (name, screen) in [("trust", CLAUDE_TRUST), ("trust card", CLAUDE_TRUST_CARD), ("bash approval", CLAUDE_BASH_APPROVAL)] {
        let readiness = readiness_of("claude", screen);
        assert!(matches!(readiness, Readiness::Gate(_)), "{name}: {readiness:?}");
        assert_ne!(profile_for("claude").ready_recogniser(screen), Some(ComposerState::Empty), "{name}");
    }
}

// ---- host shell: screen and process ----------------------------------------------------------------

#[test]
fn a_declined_trust_dialog_with_the_shell_prompt_below_is_the_host_shell() {
    assert_eq!(readiness_of("claude", CLAUDE_DECLINED), Readiness::HostShell(HostShell::AgentExited));
    assert_eq!(readiness_of("claude", SHELL_AFTER_EXIT), Readiness::HostShell(HostShell::AgentExited));
}

#[test]
fn a_killed_agent_whose_last_frame_still_shows_its_input_box_is_caught_by_the_process() {
    // Live: Claude at its box, its process killed. The screen still shows the box (nothing redrew
    // it), so the screen alone says Ready and a task would go into PowerShell. The process vetoes.
    assert_eq!(readiness_of("claude", CLAUDE_KILLED), Readiness::Ready, "the screen alone is fooled");
    let verdict = look("claude", CLAUDE_KILLED, AgentProcess::Exited);
    assert_eq!(verdict.readiness(), &Readiness::HostShell(HostShell::AgentExited));
    assert!(verdict.typing_permit().is_none());
    let error = host_shell_error(&HostShell::AgentExited, profile_for("claude").as_ref(), "pane-1");
    assert_eq!(error.code, "AGENT_EXITED");
    assert!(!error.recoverable);
    assert_eq!(error.context["typed_bytes"], 0);
}

#[test]
fn an_agent_that_never_started_is_not_ready_however_its_screen_looks() {
    let verdict = look("claude", CLAUDE_FRESH, AgentProcess::Starting);
    assert_eq!(verdict.readiness(), &Readiness::Unknown);
}

#[test]
fn claude_is_not_recognized_is_not_installed() {
    // Live capture: PowerShell's "The term '...' is not recognized" and its prompt.
    let verdict = look("claude", NOT_INSTALLED, AgentProcess::Starting);
    assert_eq!(
        verdict.readiness(),
        &Readiness::HostShell(HostShell::NotInstalled("pm-agent-not-installed-7f3".into()))
    );
    let error = host_shell_error(
        &HostShell::NotInstalled("claude".into()),
        profile_for("claude").as_ref(),
        "pane-1",
    );
    assert_eq!(error.code, "AGENT_NOT_INSTALLED");
    assert!(error.message.contains("`claude` was not found"), "{}", error.message);
    assert!(error.message.contains("npm i -g"), "the profile's install hint: {}", error.message);
}

#[test]
fn the_not_found_detector_reads_each_shell_and_nothing_else() {
    use crate::tui::shell::command_not_found;
    assert_eq!(command_not_found(NOT_INSTALLED).as_deref(), Some("pm-agent-not-installed-7f3"));
    assert_eq!(
        command_not_found("'claude' is not recognized as an internal or external command,\noperable program or batch file.\n\nC:\\repo>").as_deref(),
        Some("claude")
    );
    assert_eq!(command_not_found("bash: claude: command not found\nme@box:~/repo$").as_deref(), Some("claude"));
    assert_eq!(command_not_found(CLAUDE_FRESH), None);
    assert_eq!(command_not_found("PS C:\\repo>"), None);
    // A reply that quotes the error but is not at a shell prompt is not a missing agent.
    assert_eq!(readiness_of("claude", &format!("\u{25cf} The term 'x' is not recognized as the name of a cmdlet\n{CLAUDE_FRESH}")), Readiness::Ready);
}

// ---- Codex and Cursor ------------------------------------------------------------------------------

#[test]
fn codex_ready_loading_menu_working_and_draft() {
    assert_eq!(readiness_of("codex", CODEX_READY), Readiness::Ready);
    // 0.162.1 draws its composer while still loading; keys typed then were lost for seconds.
    assert_eq!(readiness_of("codex", CODEX_LOADING), Readiness::Unknown);
    let menu = look("codex", CODEX_UPDATE_MENU, AgentProcess::Alive);
    assert!(matches!(menu.readiness(), Readiness::Gate(gate) if gate.id == "update_available"));
    assert!(menu.typing_permit().is_none());
    assert_eq!(menu.gate_permit().map(|p| p.keys()), Some("\u{1b}"), "its own skip key");
    assert_eq!(readiness_of("codex", CODEX_WORKING), Readiness::Busy);
    // Codex can clear its box: a draft is Ready, delivery clears it first.
    let unsent = look("codex", CODEX_UNSENT, AgentProcess::Alive);
    assert_eq!((unsent.readiness(), unsent.composer()), (&Readiness::Ready, ComposerState::HasText));
}

#[test]
fn cursor_reads_its_framed_box_and_never_a_dialog() {
    for (name, screen) in [("follow-up", CURSOR_FOLLOW_UP), ("fresh after trust", CURSOR_FRESH)] {
        let verdict = look("cursor_agent", screen, AgentProcess::Alive);
        assert_eq!(verdict.readiness(), &Readiness::Ready, "{name}: {}", verdict.why());
        assert_eq!(verdict.basis(), Basis::Recognised);
    }
    for (name, screen) in [("trust", CURSOR_TRUST), ("approval", CURSOR_APPROVAL)] {
        assert!(matches!(readiness_of("cursor_agent", screen), Readiness::Gate(_)), "{name}");
        assert_ne!(profile_for("cursor_agent").ready_recogniser(screen), Some(ComposerState::Empty), "{name}");
    }
    let typed = CURSOR_FOLLOW_UP.replace("\u{2192} Add a follow-up", "\u{2192} half a thought");
    assert_eq!(profile_for("cursor_agent").ready_recogniser(&typed), Some(ComposerState::HasText));
    let working = CURSOR_FOLLOW_UP.replace("\u{2192} Add a follow-up", "\u{2192} Add a follow-up   ctrl+c to stop");
    assert_eq!(readiness_of("cursor_agent", &working), Readiness::Busy);
}

// ---- legacy readiness --------------------------------------------------------------------------------

/// The dispatchable code profiles left on the legacy readiness (no recogniser), named so a change
/// is a decision: OpenCode's TUI (no captured screens to write one from; it is typed unverified).
/// Declarative agents are legacy too unless their engine reads a composer: `[tui.ready]` describes
/// the home screen for a standby (kilo's reply screen has no "Ask anything"), so using it here
/// would park every follow-up.
#[test]
fn the_profiles_left_on_legacy_readiness_are_named() {
    let _serial = crate::tui::registry::test_serial();
    let legacy: Vec<&str> = crate::tui::profiles()
        .iter()
        .filter(|profile| profile.features().dispatchable && profile.features().kind == crate::tui::Kind::Cli)
        .filter(|profile| !has_recogniser(profile.as_ref()))
        .map(|profile| profile.id())
        .collect();
    assert_eq!(legacy, vec!["opencode"]);
    for id in ["claude", "codex", "cursor_agent"] {
        assert!(has_recogniser(profile_for(id).as_ref()), "{id}");
    }
    // Legacy: something drawn with no gate is Ready, marked Inferred.
    let verdict = look("opencode", "some agent screen\n> ", AgentProcess::Alive);
    assert_eq!((verdict.readiness(), verdict.basis()), (&Readiness::Ready, Basis::Inferred));
}

// ---- ratchets over every captured screen ---------------------------------------------------------------

/// Every `.txt` capture under `tui/*/fixtures/`.
fn every_fixture() -> Vec<(String, String)> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("tui");
    let mut out = Vec::new();
    for agent in std::fs::read_dir(&root).unwrap().flatten() {
        let dir = agent.path().join("fixtures");
        let Ok(files) = std::fs::read_dir(&dir) else { continue };
        for file in files.flatten() {
            let path = file.path();
            if path.extension().is_some_and(|ext| ext == "txt") {
                if let Ok(text) = std::fs::read_to_string(&path) {
                    out.push((path.display().to_string(), text));
                }
            }
        }
    }
    assert!(out.len() >= 40, "found only {} fixtures", out.len());
    out
}

#[test]
fn ready_and_gate_are_never_the_same_screen() {
    // For every profile on every captured screen: a recogniser that says "empty input box" is never
    // looking at a startup screen or a dialog, and the fused verdict is one thing only.
    let _serial = crate::tui::registry::test_serial();
    let mut ready_seen = std::collections::BTreeMap::<&str, usize>::new();
    for profile in crate::tui::profiles() {
        for (name, screen) in every_fixture() {
            let gate = profile.startup_gate(&screen).is_some()
                || crate::pane_prompt::detect_in_screen(profile.id(), &screen, false).is_some();
            if profile.ready_recogniser(&screen) == Some(ComposerState::Empty) {
                assert!(!gate, "{}: the recogniser reads an input box on a gate screen {name}", profile.id());
            }
            let verdict = assess(profile.as_ref(), Observation::screen_only(screen.clone()));
            if gate {
                assert!(!verdict.is_ready(), "{}: Ready on the gate screen {name}", profile.id());
                assert!(verdict.typing_permit().is_none());
            }
            if verdict.is_ready() && verdict.basis() == Basis::Recognised {
                *ready_seen.entry(profile.id()).or_default() += 1;
            }
        }
    }
    // Every code profile with a recogniser recognises Ready positively on real screens.
    for id in ["claude", "codex", "cursor_agent"] {
        assert!(ready_seen.get(id).copied().unwrap_or(0) >= 2, "{id}: {ready_seen:?}");
    }
}

#[test]
fn a_blank_screen_is_unknown_for_every_profile_and_gets_no_permit() {
    let _serial = crate::tui::registry::test_serial();
    for profile in crate::tui::profiles() {
        for blank in ["", "\n\n\n", "    \n  "] {
            let verdict = assess(profile.as_ref(), Observation::screen_only(blank.into()));
            assert_eq!(verdict.readiness(), &Readiness::Unknown, "{}", profile.id());
            assert!(verdict.typing_permit().is_none() && verdict.gate_permit().is_none());
        }
    }
}

// ---- permits ---------------------------------------------------------------------------------------------

#[test]
fn a_typing_permit_needs_the_screen_to_still_be_the_latest() {
    let permit = look("codex", CODEX_READY, AgentProcess::Alive).typing_permit().unwrap();
    assert_eq!(permit.pane_id(), "pane-1");
    let stale = permit.admit("pane-1", Some(108), || AgentProcess::Alive).unwrap_err();
    assert!(stale.starts_with(PERMIT_STALE), "{stale}");
    assert!(permit.admit("pane-2", Some(100), || AgentProcess::Alive).unwrap_err().starts_with(PERMIT_REFUSED));
    assert!(permit.admit("pane-1", Some(100), || AgentProcess::Exited).unwrap_err().contains("exited"));
    permit.admit("pane-1", Some(100), || AgentProcess::Alive).unwrap();
    // The rest of the same delivery (Enter after the verified text) rides on it: the pane echoed.
    permit.admit("pane-1", Some(400), || AgentProcess::Alive).unwrap();
    // But never into a shell: the agent dying ends the permit on any write.
    assert!(permit.admit("pane-1", Some(400), || AgentProcess::Exited).is_err());
}

#[test]
fn a_gate_permit_presses_only_its_own_keys_once() {
    let permit = look("codex", CODEX_UPDATE_MENU, AgentProcess::Alive).gate_permit().unwrap();
    assert!(permit.admit("pane-1", "1", false, Some(100)).is_err(), "not another key");
    assert!(permit.admit("pane-1", "\u{1b}", true, Some(100)).is_err(), "never with Enter");
    assert!(permit.admit("pane-1", "\u{1b}", false, Some(101)).unwrap_err().starts_with(PERMIT_STALE));
    let permit = look("codex", CODEX_UPDATE_MENU, AgentProcess::Alive).gate_permit().unwrap();
    permit.admit("pane-1", "\u{1b}", false, Some(100)).unwrap();
    assert!(permit.admit("pane-1", "\u{1b}", false, Some(100)).is_err(), "once");
    // A gate only the user may pass has no permit at all.
    assert!(look("claude", CLAUDE_TRUST, AgentProcess::Alive).gate_permit().is_none());
}

fn answer_permit(observed: Option<u64>) -> AnswerPermit {
    AnswerPermit::for_prompt("pane-1", "pid-1", "f3c1f3c1f3c1f3c1", Decider::Device("dev-a".into()), observed, AgentProcess::Alive).unwrap()
}

#[test]
fn an_answer_permit_is_for_one_prompt_on_one_pane_and_is_used_once() {
    use crate::pty::WriteOrigin;
    let permit = answer_permit(Some(100));
    assert_eq!((permit.prompt_id(), permit.fingerprint()), ("pid-1", "f3c1f3c1f3c1f3c1"));
    assert_eq!(permit.decider(), &Decider::Device("dev-a".into()));
    assert!(permit.admit("pane-2", Some(100), || AgentProcess::Alive).unwrap_err().starts_with(PERMIT_REFUSED), "another pane");
    // (The refused look above spent nothing: the pane check comes first.)
    WriteOrigin::Decided(&permit).admit_terminal_write("pane-1", "1", true, Some(100), || AgentProcess::Alive).unwrap();
    let again = permit.admit("pane-1", Some(100), || AgentProcess::Alive).unwrap_err();
    assert!(again.starts_with(PERMIT_REFUSED) && again.contains("once"), "{again}");
}

#[test]
fn an_answer_permit_is_refused_when_the_pane_printed_since_the_prompt_was_read() {
    let permit = answer_permit(Some(100));
    let stale = permit.admit("pane-1", Some(108), || AgentProcess::Alive).unwrap_err();
    assert!(stale.starts_with(PERMIT_STALE), "{stale}");
    // No position known on either side: nothing to compare (a test double), the write goes.
    answer_permit(None).admit("pane-1", Some(5), || AgentProcess::Alive).unwrap();
}

#[test]
fn an_answer_permit_never_reaches_a_host_shell() {
    let refused = AnswerPermit::for_prompt("pane-1", "pid-1", "f", Decider::Desktop, Some(1), AgentProcess::Exited).unwrap_err();
    assert!(refused.starts_with(PERMIT_REFUSED) && refused.contains("exited"), "{refused}");
    let permit = answer_permit(Some(1));
    assert!(permit.admit("pane-1", Some(1), || AgentProcess::Exited).unwrap_err().contains("exited"), "the agent died between the read and the write");
}

#[test]
fn a_decided_answer_is_tagged_as_an_answer_not_a_gate() {
    use crate::pty::WriteOrigin;
    let permit = answer_permit(Some(1));
    assert_eq!(WriteOrigin::Decided(&permit).tag(), "answer");
    assert_eq!(crate::flight::tag_for(&WriteOrigin::Decided(&permit)), crate::flight::InputTag::Answer);
    // A gate permit cannot answer a prompt: it covers only the gate's own dismiss keys, once.
    let gate = look("codex", CODEX_UPDATE_MENU, AgentProcess::Alive).gate_permit().unwrap();
    assert!(gate.admit("pane-1", "1", true, Some(1)).is_err());
}

#[test]
fn the_write_origins_that_are_not_run_input_pass_and_a_structured_prompt_never_types() {
    use crate::pty::WriteOrigin;
    for origin in [WriteOrigin::User, WriteOrigin::Answer, WriteOrigin::Nudge, WriteOrigin::Api] {
        origin.admit_terminal_write("p", "x", true, Some(1), || AgentProcess::Exited).unwrap();
    }
    assert!(WriteOrigin::Structured.admit_terminal_write("p", "task", true, None, || AgentProcess::Alive).is_err());
}

// ---- parked runs -----------------------------------------------------------------------------------------

fn park(agent: &str, screens: Vec<(&'static str, AgentProcess)>, declined_at: Option<usize>) -> (ParkEnd, usize) {
    let profile = profile_for(agent);
    let mut at = 0usize;
    let looks = std::cell::Cell::new(0usize);
    let end = wait_while_parked(
        profile.as_ref(),
        || {
            let (screen, process) = screens[at.min(screens.len() - 1)];
            at += 1;
            looks.set(at);
            Observation { pane_id: "p".into(), screen: screen.into(), output_pos: None, agent: process }
        },
        || false,
        || declined_at.is_some_and(|n| looks.get() >= n),
        || looks.get() >= 50,
        || {},
    );
    (end, looks.get())
}

#[test]
fn a_parked_run_resumes_when_the_box_shows_and_ends_when_the_agent_goes() {
    let (end, looks) = park("codex", vec![(CODEX_LOADING, AgentProcess::Alive), (CODEX_LOADING, AgentProcess::Alive), (CODEX_READY, AgentProcess::Alive)], None);
    assert_eq!((end, looks), (ParkEnd::Resume, 3));
    // A notice delivery can close itself: resume (delivery closes it with its permit).
    assert_eq!(park("codex", vec![(CODEX_UPDATE_MENU, AgentProcess::Alive)], None).0, ParkEnd::Resume);
    // A dialog only the user answers keeps it parked; then the user declines.
    assert_eq!(park("claude", vec![(CLAUDE_TRUST, AgentProcess::Alive)], Some(3)).0, ParkEnd::Declined);
    assert_eq!(
        park("claude", vec![(CLAUDE_FRESH, AgentProcess::Exited)], None).0,
        ParkEnd::Gone(HostShell::AgentExited)
    );
    let (end, looks) = park("codex", vec![(CODEX_LOADING, AgentProcess::Alive)], None);
    assert!(matches!(&end, ParkEnd::TimedOut { why, screen } if why.contains("input box") && screen.contains("loading")), "{end:?}");
    assert_eq!(looks, 50);
}

#[test]
fn the_error_of_a_screen_that_never_became_ready_explains_itself() {
    let error = not_ready_error("the agent's input box already holds text that Puppet Master did not type", CLAUDE_DRAFT, "pane-1");
    assert_eq!(error.code, SCREEN_NOT_READY);
    assert!(error.message.contains("did not switch to a background run"), "{}", error.message);
    assert!(error.context["screen"].as_str().unwrap().contains("half a thought"));
    assert_eq!(error.context["typed_bytes"], 0);
}

// ---- positive readiness after a startup screen (live v0.2.0 hunt, Codex after Trust) ----------

/// A scripted pane for `wait_until_settled`: each look returns the next (screen, output position),
/// the last one repeating; the clock advances 100 ms per pause.
fn settle_run(agent: &str, looks: &[(&str, u64, AgentProcess)], quiet_ms: u64, max_ms: u64) -> (Settle, u64) {
    let clock = std::cell::Cell::new(0u64);
    let index = std::cell::Cell::new(0usize);
    let outcome = wait_until_settled(
        profile_for(agent).as_ref(),
        || {
            let (screen, pos, agent) = looks[index.get().min(looks.len() - 1)];
            index.set(index.get() + 1);
            Observation { pane_id: "p".into(), screen: screen.to_string(), output_pos: Some(pos), agent }
        },
        || clock.get(),
        || clock.set(clock.get() + 100),
        std::time::Duration::from_millis(quiet_ms),
        std::time::Duration::from_millis(max_ms),
    );
    (outcome, clock.get())
}

/// The composer is drawn at once after Trust, but the pane is still printing: not settled until it
/// has been quiet for the whole quiet time, even though every look reads `Ready`.
#[test]
fn a_ready_looking_composer_that_still_prints_is_not_settled_until_it_is_quiet() {
    let printing: Vec<(&str, u64, AgentProcess)> = (0..20).map(|i| (CODEX_READY, 1_000 + i * 64, AgentProcess::Alive)).collect();
    let (outcome, waited) = settle_run("codex", &printing, 1_500, 30_000);
    assert_eq!(outcome, Settle::Settled);
    // 20 looks that printed (2 s), then 1.5 s of quiet.
    assert!(waited >= 1_900 + 1_500, "settled after {waited} ms");
}

#[test]
fn a_quiet_ready_pane_settles_after_the_quiet_time_and_a_loading_one_does_not() {
    let (outcome, waited) = settle_run("codex", &[(CODEX_READY, 500, AgentProcess::Alive)], 1_500, 30_000);
    assert_eq!((outcome, waited), (Settle::Settled, 1_500));
    // Codex's own `loading` banner is never ready, however quiet.
    let (outcome, waited) = settle_run("codex", &[(CODEX_LOADING, 500, AgentProcess::Alive)], 1_500, 5_000);
    assert_eq!((outcome, waited), (Settle::TimedOut, 5_000));
}

#[test]
fn an_agent_that_left_ends_the_settle_wait_at_once() {
    let (outcome, waited) = settle_run("codex", &[(CODEX_READY, 500, AgentProcess::Exited)], 1_500, 30_000);
    assert!(matches!(outcome, Settle::Gone(HostShell::AgentExited)), "{outcome:?}");
    assert_eq!(waited, 0);
}
