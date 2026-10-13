use super::*;
use std::cell::RefCell;
use std::path::PathBuf;

// ------------------------------------------------------------ window

#[test]
fn window_is_a_small_frameless_always_on_top_hidden_window() {
    let spec = window_spec();
    assert_eq!(spec.label, "askbar");
    assert_eq!(spec.url, "index.html?askbar=1");
    assert_eq!((spec.width, spec.height, spec.expanded_height), (640.0, 64.0, 420.0));
    assert!(!spec.decorations && spec.always_on_top && spec.transparent && spec.skip_taskbar);
    // It must never appear or take focus by itself (only the user's shortcut shows it).
    assert!(!spec.visible && !spec.focused);
    assert_eq!(height_for(false), 64.0);
    assert_eq!(height_for(true), 420.0);
}

#[test]
fn the_window_is_allowed_in_the_capabilities_file() {
    let caps = include_str!("../../capabilities/default.json");
    let value: serde_json::Value = serde_json::from_str(caps).unwrap();
    let windows = value["windows"].as_array().unwrap();
    assert!(windows.iter().any(|w| w == WINDOW_LABEL), "askbar missing from capabilities");
}

#[test]
fn position_is_centred_a_fifth_down_on_the_monitor() {
    // 1920x1080 monitor whose origin is at x=1920 (a second monitor), window 640 wide.
    assert_eq!(spotlight_position((1920, 0, 1920, 1080), 640), (1920 + 640, 216));
    // Negative origin (monitor left of the primary).
    assert_eq!(spotlight_position((-1280, 100, 1280, 720), 640), (-1280 + 320, 100 + 144));
}

#[test]
fn a_blur_during_the_focus_handover_does_not_hide_the_bar() {
    assert!(!should_hide_on_blur(1_000 + 50, 1_000));
    assert!(should_hide_on_blur(1_000 + BLUR_GRACE_MS, 1_000));
    assert!(should_hide_on_blur(10_000, 1_000));
}

// ------------------------------------------------------------ settings and shortcut

#[derive(Default)]
struct FakeOs {
    log: Vec<String>,
    fail: bool,
}
impl ShortcutRegistrar for FakeOs {
    fn register(&mut self, shortcut: &str) -> Result<(), String> {
        if self.fail {
            return Err("taken".into());
        }
        self.log.push(format!("+{shortcut}"));
        Ok(())
    }
    fn unregister(&mut self, shortcut: &str) {
        self.log.push(format!("-{shortcut}"));
    }
}

fn enabled(shortcut: &str) -> Settings {
    Settings { enabled: true, shortcut: shortcut.into(), ..Settings::default() }
}

#[test]
fn it_is_off_by_default_and_registers_nothing() {
    let settings = Settings::default();
    assert!(!settings.enabled);
    assert_eq!(settings.shortcut, "Ctrl+Alt+Space");
    let mut os = FakeOs::default();
    let mut current = None;
    sync_shortcut(&mut os, &mut current, &settings).unwrap();
    assert!(os.log.is_empty(), "disabled bar must not grab a global shortcut");
    assert_eq!(current, None);
}

#[test]
fn the_shortcut_is_registered_only_when_enabled_and_follows_changes() {
    let mut os = FakeOs::default();
    let mut current = None;
    sync_shortcut(&mut os, &mut current, &enabled("Ctrl+Alt+Space")).unwrap();
    assert_eq!(os.log, vec!["+Ctrl+Alt+Space"]);
    // Same settings again: nothing to do.
    sync_shortcut(&mut os, &mut current, &enabled("Ctrl+Alt+Space")).unwrap();
    assert_eq!(os.log.len(), 1);
    // A different choice: the old one is released first.
    sync_shortcut(&mut os, &mut current, &enabled("Alt+Space")).unwrap();
    assert_eq!(os.log, vec!["+Ctrl+Alt+Space", "-Ctrl+Alt+Space", "+Alt+Space"]);
    // Turning it off releases it.
    sync_shortcut(&mut os, &mut current, &Settings::default()).unwrap();
    assert_eq!(os.log.last().unwrap(), "-Alt+Space");
    assert_eq!(current, None);
}

#[test]
fn a_shortcut_the_os_refuses_is_reported_and_not_remembered() {
    let mut os = FakeOs { fail: true, ..FakeOs::default() };
    let mut current = None;
    assert_eq!(sync_shortcut(&mut os, &mut current, &enabled("Ctrl+Alt+K")), Err("taken".into()));
    assert_eq!(current, None);
}

#[test]
fn settings_only_accept_the_choice_list_and_trim_agents() {
    let odd = Settings { enabled: true, shortcut: "Ctrl+Q".into(), default_agent: Some("  ".into()), last_agent: Some(" codex ".into()) }
        .sanitized();
    assert_eq!(odd.shortcut, DEFAULT_SHORTCUT);
    assert_eq!(odd.default_agent, None);
    assert_eq!(odd.last_agent.as_deref(), Some("codex"));
    for choice in SHORTCUT_CHOICES {
        assert_eq!(enabled(choice).sanitized().shortcut, choice);
    }
    // Old or partial files still load.
    let partial: Settings = serde_json::from_str(r#"{"enabled": true}"#).unwrap();
    assert!(partial.enabled && partial.shortcut == DEFAULT_SHORTCUT);
}

// ------------------------------------------------------------ clipboard

#[test]
fn short_clipboard_text_is_kept_whole() {
    let clip = clip_text("fn main() {}").unwrap();
    assert_eq!((clip.text.as_str(), clip.chars, clip.truncated), ("fn main() {}", 12, false));
}

#[test]
fn long_clipboard_text_is_cut_to_8000_characters_not_bytes() {
    // Multi-byte characters: byte length is far above the limit even at the limit in characters.
    let text = "é".repeat(CLIPBOARD_LIMIT + 500);
    let clip = clip_text(&text).unwrap();
    assert_eq!(clip.text.chars().count(), CLIPBOARD_LIMIT);
    assert_eq!(clip.chars, CLIPBOARD_LIMIT + 500);
    assert!(clip.truncated);
    let exact = "a".repeat(CLIPBOARD_LIMIT);
    assert!(!clip_text(&exact).unwrap().truncated);
}

#[test]
fn an_empty_or_blank_clipboard_attaches_nothing() {
    assert_eq!(clip_text(""), None);
    assert_eq!(clip_text(" \n\t "), None);
    assert_eq!(compose_task(" why? ", Some("  ")), "why?");
    assert_eq!(compose_task("why?", None), "why?");
}

#[test]
fn the_task_quotes_the_clipboard_as_context_and_never_exceeds_the_limit() {
    let task = compose_task("What does this do?", Some("let x = 1;"));
    assert!(task.starts_with("What does this do?\n\nContext from my clipboard:"));
    assert!(task.contains("```\nlet x = 1;\n```"));
    let big = compose_task("q", Some(&"x".repeat(50_000)));
    assert!(big.contains("(cut short)"));
    assert!(big.chars().count() < CLIPBOARD_LIMIT + 200);
}

// ------------------------------------------------------------ agents

fn capable() -> Vec<(String, String)> {
    vec![("alpha".into(), "Alpha".into()), ("beta".into(), "Beta".into()), ("gamma".into(), "Gamma".into())]
}

#[test]
fn only_installed_capable_agents_are_listed_with_ready_marks() {
    let list = list_agents(&capable(), &|id| id != "beta", &["GAMMA".to_string()]);
    assert_eq!(
        list,
        vec![
            AgentEntry { id: "alpha".into(), label: "Alpha".into(), ready: false },
            AgentEntry { id: "gamma".into(), label: "Gamma".into(), ready: true },
        ]
    );
    assert!(list_agents(&capable(), &|_| false, &[]).is_empty());
}

#[test]
fn the_real_registry_offers_exactly_the_read_only_headless_agents() {
    // Capability comes from the registry, never from names in this module.
    let capable_ids = crate::tui::read_only_backends();
    assert!(!capable_ids.is_empty());
    for id in capable_ids {
        assert!(crate::tui::is_agent_backend(id), "{id} must be a real agent, not a shell");
    }
}

// ------------------------------------------------------------ asking (quick_ask wiring)

fn ask(question: &str) -> AskRequest {
    AskRequest { ask_id: "a1".into(), question: question.into(), agent: "alpha".into(), clipboard: None }
}

#[test]
fn an_ask_becomes_a_read_only_headless_quick_ask() {
    let request = AskRequest { clipboard: Some("ctx".into()), ..ask("why?") };
    let quick = quick_request(&request, Some("C:\\work".into()));
    assert_eq!(quick.backend.as_deref(), Some("alpha"));
    assert_eq!(quick.read_only, Some(true));
    assert_eq!(quick.wait_ms, Some(ANSWER_WAIT_MS));
    assert_eq!(quick.project_path.as_deref(), Some("C:\\work"));
    assert!(quick.task.starts_with("why?") && quick.task.contains("ctx"));
    // And the real quick_ask turns it into the conservative run (read-only, no shell, headless).
    let run = crate::agent_runs::quick::build_run_request(&quick, "claude").unwrap();
    assert!(run.headless && run.read_only && !run.allow_shell && !run.keep_pane);
}

#[test]
fn a_finished_answer_comes_back_with_the_agent_label() {
    let seen = RefCell::new(None);
    let result = run_ask(&ask("  2+2? "), "Alpha", None, &|quick| {
        *seen.borrow_mut() = Some(quick);
        Ok(json!({"answer": "  4\n", "receipt": {"outcome": "completed"}}))
    });
    assert!(result.ok);
    assert_eq!(result.answer.as_deref(), Some("4"));
    assert_eq!((result.agent.as_str(), result.agent_label.as_str(), result.question.as_str()), ("alpha", "Alpha", "2+2?"));
    assert_eq!(seen.borrow().as_ref().unwrap().task, "2+2?");
}

#[test]
fn a_blank_question_never_reaches_the_agent() {
    let result = run_ask(&ask("   "), "Alpha", None, &|_| panic!("must not run"));
    assert!(!result.ok);
    assert_eq!(result.error.unwrap().code, "EMPTY_QUESTION");
}

#[test]
fn every_failure_is_a_short_human_sentence() {
    let code = |value: Result<Value, OperationError>| {
        let result = run_ask(&ask("q"), "Alpha", None, &|_| value.clone());
        assert!(!result.ok && result.answer.is_none());
        result.error.unwrap()
    };
    assert_eq!(code(Ok(json!({"status": "running", "handle": "h"}))).code, "ASK_TIMEOUT");
    assert_eq!(code(Ok(json!({"status": "needs_input", "handle": "h"}))).code, "ASK_NEEDS_INPUT");
    assert_eq!(code(Ok(json!({"answer": "   "}))).code, "ASK_EMPTY");
    let missing = code(Err(OperationError::new("BACKEND_UNAVAILABLE", "backend `alpha` was not found on PATH", false)));
    assert_eq!(missing.code, "BACKEND_UNAVAILABLE");
    assert!(!missing.message.is_empty() && !missing.message.contains("PATH`"));
}

// A fake agent CLI: a throwaway script, the same pattern the agent doctor tests use.
struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn fake_agent(name: &str, unix: &str, windows: &str) -> (TempDir, PathBuf) {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!("pm-askbar-test-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    std::fs::create_dir_all(&dir).unwrap();
    let path;
    #[cfg(windows)]
    {
        let _ = unix;
        path = dir.join(format!("{name}.cmd"));
        std::fs::write(&path, format!("@echo off\r\n{windows}\r\n")).unwrap();
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = windows;
        path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{unix}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    (TempDir(dir), path)
}

/// A runner that really starts the fake CLI with the task as its argument, the way `quick_ask`
/// would start a headless CLI, and shapes stdout into the `{answer}` / error the real one returns.
fn process_runner(program: PathBuf) -> impl Fn(QuickAskRequest) -> Result<Value, OperationError> {
    move |quick| {
        let output = std::process::Command::new(&program)
            .arg(quick.task.lines().next().unwrap_or(""))
            .output()
            .map_err(|e| OperationError::new("BACKEND_UNAVAILABLE", e.to_string(), false))?;
        if !output.status.success() {
            return Err(OperationError::new(
                "AGENT_FAILED",
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
                false,
            ));
        }
        Ok(json!({"answer": String::from_utf8_lossy(&output.stdout).trim()}))
    }
}

#[test]
fn a_fake_agent_process_answers_through_the_bar_path() {
    let (_dir, program) = fake_agent("answers", "echo 'the answer is 42'", "echo the answer is 42");
    let result = run_ask(&ask("meaning of life?"), "Fake", None, &process_runner(program));
    assert!(result.ok, "{:?}", result.error);
    assert_eq!(result.answer.as_deref(), Some("the answer is 42"));
}

#[test]
fn a_fake_agent_process_that_fails_gives_the_error_path() {
    let (_dir, program) = fake_agent("breaks", "echo 'not logged in' >&2; exit 3", "echo not logged in 1>&2\r\nexit /b 3");
    let result = run_ask(&ask("anything"), "Fake", None, &process_runner(program));
    assert!(!result.ok && result.answer.is_none());
    let error = result.error.unwrap();
    assert_eq!(error.code, "AGENT_FAILED");
    assert!(!error.message.is_empty());
}

// ------------------------------------------------------------ continue in a pane

#[test]
fn continue_in_a_pane_is_a_visible_kept_pane_with_the_question_as_first_task() {
    let body = continue_request("alpha", "why?", Some("ctx"), Some("C:\\work".into()));
    let run: crate::agent_runs::AgentRunRequest = serde_json::from_value(body).unwrap();
    assert_eq!(run.agent_type, "alpha");
    assert!(!run.headless && run.background && run.keep_pane && !run.read_only);
    assert!(run.task.starts_with("why?") && run.task.contains("ctx"));
    assert_eq!(run.project_path.as_deref(), Some("C:\\work"));
}

// ------------------------------------------------------------ notices

fn result(id: &str) -> AskResult {
    run_ask(&AskRequest { ask_id: id.into(), ..ask("q") }, "Alpha", None, &|_| Ok(json!({"answer": "a"})))
}

#[test]
fn a_notice_is_needed_only_when_the_bar_was_hidden() {
    assert!(needs_notice(false));
    assert!(!needs_notice(true));
}

#[test]
fn notices_keep_newest_first_replace_by_ask_and_can_be_taken() {
    let mut notices = Notices::default();
    notices.push(&result("1"), 10);
    notices.push(&result("2"), 20);
    notices.push(&result("1"), 30);
    let ids: Vec<String> = notices.list().into_iter().map(|n| n.id).collect();
    assert_eq!(ids, vec!["ask-1", "ask-2"]);
    let taken = notices.take("ask-2").unwrap();
    assert_eq!((taken.agent_label.as_str(), taken.result.answer.as_deref()), ("Alpha", Some("a")));
    assert!(notices.take("ask-2").is_none());
    assert_eq!(notices.list().len(), 1);
}

#[test]
fn notices_are_capped() {
    let mut notices = Notices::default();
    for i in 0..(MAX_NOTICES + 5) {
        notices.push(&result(&i.to_string()), i as u64);
    }
    let list = notices.list();
    assert_eq!(list.len(), MAX_NOTICES);
    assert_eq!(list[0].id, format!("ask-{}", MAX_NOTICES + 4));
}

#[test]
fn the_wire_shape_matches_the_frontend_contract() {
    let value = serde_json::to_value(result("w")).unwrap();
    for key in ["ask_id", "agent", "agent_label", "question", "ok", "answer", "error"] {
        assert!(value.get(key).is_some(), "missing {key}");
    }
    let clip = serde_json::to_value(clip_text("abc").unwrap()).unwrap();
    assert_eq!(clip, json!({"text": "abc", "chars": 3, "truncated": false}));
    let agent: AskRequest = serde_json::from_value(json!({"ask_id": "x", "question": "q", "agent": "a", "clipboard": null})).unwrap();
    assert!(agent.clipboard.is_none());
}
