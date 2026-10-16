//! The local drafter: the prompt goes on standard input, read-only, in the work folder.

use std::cell::RefCell;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::json;

use crate::tui::declarative::wizard::ask::{run_local, without_task, MARKER, MAX_PROMPT_CHARS};
use crate::tui::types::{HeadlessLaunch, LaunchSpec};

use super::support::{bin_dir, ok, start, Tree};

fn exe(name: &str) -> String {
    bin_dir().join(if cfg!(windows) { format!("{name}.exe") } else { name.to_string() }).to_string_lossy().into_owned()
}

/// The command line of a pretend agent that takes its task as the last argument, like `codex exec`.
fn pretend(program: String) -> impl Fn(&HeadlessLaunch) -> Result<LaunchSpec, String> {
    move |launch| Ok(LaunchSpec::new(program.clone(), vec!["exec".into(), "--json".into(), launch.task.into()], PathBuf::from(launch.project), true))
}

fn text(output: &str, code: i32) -> Option<String> {
    (code == 0).then(|| output.trim().to_string())
}

fn probed(wizard: &crate::tui::declarative::wizard::Wizard) -> String {
    let id = start(wizard, "fake-agent");
    ok(wizard, "probe", json!({"session": id}));
    id
}

#[test]
fn a_prompt_far_over_the_command_line_limit_arrives_whole_on_stdin() {
    let tree = Tree::new("ask-stdin");
    let wizard = tree.wizard();
    let id = probed(&wizard);
    // `stdin-agent` answers "stdin-bytes: N" with N the number of bytes it read from stdin
    let prompt = "x".repeat(200_000);
    let answer = run_local(&wizard, &id, &prompt, &pretend(exe("stdin-agent")), &text).unwrap();
    assert_eq!(answer, "stdin-bytes: 200000", "30,000 characters was the old command-line limit");
    assert_eq!(wizard.read(&id, |s| s.busy).unwrap(), None, "the session is free again");
}

#[test]
fn the_launch_is_read_only_without_a_shell_in_the_work_folder_and_the_marker_never_reaches_the_program() {
    let tree = Tree::new("ask-launch");
    let wizard = tree.wizard();
    let id = probed(&wizard);
    let seen: RefCell<Option<(bool, bool, bool, String, String)>> = RefCell::new(None);
    let program = exe("stdin-agent");
    let build = |l: &HeadlessLaunch| {
        *seen.borrow_mut() = Some((l.read_only, l.allow_shell, l.trust_workspace, l.project.to_string(), l.task.to_string()));
        pretend(program.clone())(l)
    };
    run_local(&wizard, &id, "hello", &build, &text).unwrap();
    let work = wizard.read(&id, |s| s.work_dir.to_string_lossy().into_owned()).unwrap();
    let (read_only, shell, trust, project, task) = seen.borrow().clone().unwrap();
    assert!(read_only && !shell && !trust);
    assert_eq!(project, work, "never the person's project");
    assert_eq!(task, MARKER);
}

#[test]
fn the_marker_is_taken_out_of_the_command_line_and_must_be_exactly_one_argument() {
    let claude_shape: Vec<String> = ["-p", MARKER, "--output-format", "json", "--permission-prompts", "none", "--permission-mode", "plan"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(
        without_task(&claude_shape).unwrap(),
        ["-p", "--output-format", "json", "--permission-prompts", "none", "--permission-mode", "plan"]
    );
    let codex_shape: Vec<String> = ["--ask-for-approval", "never", "exec", "--json", "--sandbox", "read-only", "--skip-git-repo-check", MARKER]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(without_task(&codex_shape).unwrap().last().map(String::as_str), Some("--skip-git-repo-check"));
    let none: Vec<String> = vec!["run".into()];
    assert!(without_task(&none).is_none());
    let embedded: Vec<String> = vec![format!("--prompt={MARKER}")];
    assert!(without_task(&embedded).is_none(), "an argument that only contains the marker cannot be split");
    let twice: Vec<String> = vec![MARKER.into(), MARKER.into()];
    assert!(without_task(&twice).is_none());
}

#[test]
fn an_agent_that_cannot_take_stdin_is_refused_clearly() {
    let tree = Tree::new("ask-nostdin");
    let wizard = tree.wizard();
    let id = probed(&wizard);
    let build = |_: &HeadlessLaunch| Ok(LaunchSpec::new(exe("stdin-agent"), vec!["run".into()], PathBuf::from("."), true));
    let err = run_local(&wizard, &id, "hello", &build, &text).unwrap_err();
    assert_eq!((err.status, err.code), (409, "WIZARD_DRAFTER_NO_STDIN"));
    let unavailable = |_: &HeadlessLaunch| Err("claude was not found".to_string());
    assert_eq!(run_local(&wizard, &id, "hello", &unavailable, &text).unwrap_err().code, "WIZARD_DRAFTER_UNAVAILABLE");
    assert_eq!(wizard.read(&id, |s| s.busy).unwrap(), None);
}

#[test]
fn huge_blank_and_unknown_requests_are_refused_before_anything_starts() {
    let tree = Tree::new("ask-refuse");
    let wizard = tree.wizard();
    let id = probed(&wizard);
    let build = pretend(exe("stdin-agent"));
    let long = "x".repeat(MAX_PROMPT_CHARS + 1);
    assert_eq!(run_local(&wizard, &id, &long, &build, &text).unwrap_err().code, "WIZARD_PROMPT_TOO_LONG");
    assert_eq!(run_local(&wizard, &id, "  ", &build, &text).unwrap_err().code, "INVALID_ARGUMENT");
    assert_eq!(run_local(&wizard, "wz_nope", "hi", &build, &text).unwrap_err().code, "WIZARD_SESSION_NOT_FOUND");
    // not yet probed: there is nothing to write a profile from
    let fresh = start(&wizard, "fake-agent");
    assert_eq!(run_local(&wizard, &fresh, "hi", &build, &text).unwrap_err().code, "WIZARD_BAD_STATE");
    // a prompt of exactly the limit is accepted
    assert!(run_local(&wizard, &id, &"x".repeat(MAX_PROMPT_CHARS), &build, &text).is_ok());
}

#[test]
fn an_agent_that_prints_nothing_usable_is_an_empty_answer() {
    let tree = Tree::new("ask-empty");
    let wizard = tree.wizard();
    let id = probed(&wizard);
    let err = run_local(&wizard, &id, "hi", &pretend(exe("silent-agent")), &text).unwrap_err();
    assert_eq!(err.code, "DRAFTER_EMPTY");
    let missing = pretend(tree.root.join("no-such-program").to_string_lossy().into_owned());
    assert_eq!(run_local(&wizard, &id, "hi", &missing, &text).unwrap_err().code, "DRAFTER_FAILED");
}

#[test]
fn an_agent_that_never_answers_times_out_and_cancel_stops_it() {
    let tree = Tree::new("ask-hang");
    let wizard = std::sync::Arc::new(tree.wizard_with(|env| env.limits.draft_timeout = Duration::from_millis(400)));
    let id = probed(&wizard);
    let err = run_local(&wizard, &id, "hi", &pretend(exe("hang-agent")), &text).unwrap_err();
    assert_eq!((err.status, err.code), (504, "DRAFTER_TIMEOUT"));

    let tree = Tree::new("ask-cancel");
    let wizard = std::sync::Arc::new(tree.wizard_with(|env| env.limits.draft_timeout = Duration::from_secs(120)));
    let id = probed(&wizard);
    let runner = {
        let wizard = wizard.clone();
        let id = id.clone();
        std::thread::spawn(move || run_local(&wizard, &id, "hi", &pretend(exe("hang-agent")), &text))
    };
    let started = Instant::now();
    while wizard.read(&id, |s| s.busy).unwrap().is_none() {
        assert!(started.elapsed() < Duration::from_secs(60), "the drafter never started");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(ok(&wizard, "cancel", json!({"session": id}))["state"], "cancelled");
    assert_eq!(runner.join().unwrap().unwrap_err().code, "WIZARD_CANCELLED");
    assert!(started.elapsed() < Duration::from_secs(90), "cancel must not wait for the 120 s timeout");
}
