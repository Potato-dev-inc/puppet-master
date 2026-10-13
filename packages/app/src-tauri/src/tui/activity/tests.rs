use super::*;
use crate::tui::declarative::{parse_definition, DeclarativeProfile};
use crate::tui::profile_for;

const GREP_TURN: &str = include_str!("../cursor/fixtures/reply_grep_turn.txt");
const PYTEST_SCREEN: &str = include_str!("../cursor/fixtures/reply_pytest_final_screen.txt");
const APPROVAL: &str = include_str!("../cursor/fixtures/command_approval_visible.txt");
const TRUST: &str = include_str!("../cursor/fixtures/workspace_trust_visible_pending.txt");

fn cursor(tail: &str, status: PaneStatus) -> Activity {
    profile_for("cursor_agent").activity(tail, status).expect("activity")
}

#[test]
fn cursor_tool_rows_become_running_phrases() {
    let a = cursor(GREP_TURN, PaneStatus::Running);
    assert_eq!(a.kind, ActivityKind::Working);
    assert_eq!(a.text, "Searching \"def \" in .");

    let a = cursor(PYTEST_SCREEN, PaneStatus::Running);
    assert_eq!(a.text, "Running python -m pytest -q");
}

#[test]
fn cursor_approval_prompt_says_what_it_waits_for() {
    let a = cursor(APPROVAL, PaneStatus::WaitingInput);
    assert_eq!(a.kind, ActivityKind::Waiting);
    assert!(a.text.starts_with("Waiting: allow `cd "), "{}", a.text);
    assert!(a.text.chars().count() <= MAX_LEN, "{}", a.text);

    let a = cursor(TRUST, PaneStatus::WaitingInput);
    assert_eq!(a.text, "Waiting: trust this folder?");
}

#[test]
fn status_only_phrases_for_agents_that_teach_nothing() {
    // An unregistered agent gets the generic profile: no tool rows, so status-level phrases.
    let p = profile_for("some-unregistered-agent");
    let tail = "  $ npm test\n  something is happening";
    assert_eq!(p.activity(tail, PaneStatus::Running).unwrap().text, "Working");
    assert_eq!(p.activity(tail, PaneStatus::Idle).unwrap(), Activity::new(ActivityKind::Idle, "Idle"));
    assert_eq!(p.activity(tail, PaneStatus::WaitingInput).unwrap().text, "Waiting for you");
}

#[test]
fn error_status_quotes_the_error_line() {
    let p = profile_for("some-unregistered-agent");
    let tail = "thinking\nError: rate limit exceeded, try again later\n> ";
    let a = p.activity(tail, PaneStatus::Error).unwrap();
    assert_eq!(a.kind, ActivityKind::Error);
    assert_eq!(a.text, "Error: rate limit exceeded, try again later");
    assert_eq!(p.activity("nothing useful", PaneStatus::Error).unwrap().text, "Error");
}

#[test]
fn humanize_maps_verbs_and_shell_rows() {
    assert_eq!(humanize_tool_line("● Edited src/app.ts"), "Editing src/app.ts");
    assert_eq!(humanize_tool_line("  └ Read package.json"), "Reading package.json");
    assert_eq!(humanize_tool_line("$ npm test 12.5s"), "Running npm test");
    assert_eq!(humanize_tool_line("> ls -la"), "Running ls -la");
    assert_eq!(humanize_tool_line("Thinking about the plan (esc to interrupt)"), "Thinking about the plan");
    assert_eq!(humanize_tool_line("compiling crate"), "Compiling crate");
}

#[test]
fn phrases_are_one_short_line() {
    let long = format!("$ {}", "word ".repeat(80));
    let phrase = humanize_tool_line(&long);
    assert!(phrase.chars().count() <= MAX_LEN, "{phrase}");
    assert!(phrase.ends_with("..."));
    assert!(!phrase.contains('\n'));
}

const TUI_AGENT: &str = r##"
schema_version = 1

[agent]
id = "doing"
label = "Doing"
experimental = true

[binary]
names = ["doing"]

[tui]
argv = []
activity_regex = ['^\s*[>*]\s*(?P<text>(?:ran|edited|read) .+)$', '^\s*tool: (\w+ .+)$']
"##;

fn declarative() -> DeclarativeProfile {
    DeclarativeProfile::new(parse_definition(TUI_AGENT).unwrap()).unwrap()
}

#[test]
fn declarative_activity_regex_teaches_the_phrase() {
    let p = declarative();
    let screen = "intro\n> edited src/lib.rs\nspinner...\n";
    assert_eq!(p.activity(screen, PaneStatus::Running).unwrap().text, "Editing src/lib.rs");
    // second pattern, group 1; newest matching line wins
    let screen = "> read old.txt\ntool: grep foo src\n";
    assert_eq!(p.activity(screen, PaneStatus::Running).unwrap().text, "Searching foo src");
    // no match: the generic status phrase
    assert_eq!(p.activity("nothing", PaneStatus::Running).unwrap().text, "Working");
    // patterns only describe running work; idle stays idle
    assert_eq!(p.activity(screen, PaneStatus::Idle).unwrap().text, "Idle");
}

#[test]
fn a_bad_activity_regex_is_rejected_at_load() {
    let bad = TUI_AGENT.replace("'^\\s*tool: (\\w+ .+)$'", "'(unclosed'");
    let error = DeclarativeProfile::new(parse_definition(&bad).unwrap()).err().expect("rejected");
    assert_eq!(error.code, "AGENT_DEF_BAD_REGEX");
    assert!(error.path.contains("activity_regex"), "{}", error.path);
}
