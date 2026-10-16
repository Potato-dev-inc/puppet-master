//! `exec`: timeout, output cap, cancel, early stop, cleaning.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::tui::declarative::wizard::exec::{clean, cut_bytes, cut_chars, plain, run, RunSpec};

use super::support::{bin_dir, Tree};

fn exe(name: &str) -> std::path::PathBuf {
    bin_dir().join(if cfg!(windows) { format!("{name}.exe") } else { name.to_string() })
}

#[test]
fn a_program_that_never_ends_is_killed_at_the_timeout() {
    let tree = Tree::new("exec-timeout");
    let program = exe("hang-agent");
    let args: Vec<String> = vec![];
    let mut spec = RunSpec::new(&program, &args, &tree.temp);
    spec.timeout = Duration::from_millis(300);
    let started = Instant::now();
    let out = run(&spec);
    assert!(out.timed_out);
    assert_eq!(out.exit, None);
    assert!(started.elapsed() < Duration::from_secs(50), "the child sleeps 60 s; it must have been killed: {:?}", started.elapsed());
}

#[test]
fn output_is_capped_but_the_child_is_read_to_the_end() {
    let tree = Tree::new("exec-cap");
    let program = exe("flood-agent");
    let args: Vec<String> = vec![];
    let mut spec = RunSpec::new(&program, &args, &tree.temp);
    spec.max_bytes = 4096;
    spec.timeout = Duration::from_secs(20);
    let out = run(&spec);
    assert_eq!(out.exit, Some(0), "the child must not block on a full pipe");
    assert!(out.stdout.len() <= 4096, "{}", out.stdout.len());
    assert!(out.truncated);
    assert!(!out.timed_out);
}

#[test]
fn cancel_stops_a_running_child_quickly() {
    let tree = Tree::new("exec-cancel");
    let program = exe("hang-agent");
    let args: Vec<String> = vec![];
    let cancel = AtomicBool::new(false);
    let started = Instant::now();
    let out = std::thread::scope(|scope| {
        let handle = scope.spawn(|| {
            let mut spec = RunSpec::new(&program, &args, &tree.temp);
            spec.timeout = Duration::from_secs(30);
            spec.cancel = Some(&cancel);
            run(&spec)
        });
        std::thread::sleep(Duration::from_millis(200));
        cancel.store(true, Ordering::SeqCst);
        handle.join().unwrap()
    });
    assert!(out.cancelled);
    assert!(!out.timed_out);
    assert!(started.elapsed() < Duration::from_secs(25));
}

#[test]
fn a_program_that_cannot_start_reports_why() {
    let tree = Tree::new("exec-spawn");
    let missing = tree.temp.join("no-such-program");
    let args: Vec<String> = vec![];
    let out = run(&RunSpec::new(&missing, &args, &tree.temp));
    assert!(out.spawn_error.is_some());
    assert_eq!(out.exit, None);
}

#[test]
fn a_normal_run_returns_output_and_exit_code() {
    let tree = Tree::new("exec-ok");
    let program = exe("fake-agent");
    let args = vec!["--version".to_string()];
    let out = run(&RunSpec::new(&program, &args, &tree.temp));
    assert_eq!(out.exit, Some(0));
    assert_eq!(out.stdout.trim(), "fake-agent 1.4.2");
    assert!(!out.truncated && !out.timed_out);
}

#[test]
fn early_stop_kills_a_child_that_lingers_after_its_last_event() {
    let tree = Tree::new("exec-early");
    let program = exe("linger-agent");
    let args: Vec<String> = ["run", "--format", "json", "--", "Reply with exactly the word: pong"].iter().map(|s| s.to_string()).collect();
    let rule = |text: &str| text.contains("\"event\":\"end\"");
    let mut spec = RunSpec::new(&program, &args, &tree.temp);
    spec.timeout = Duration::from_secs(30);
    spec.early_stop = Some(&rule);
    spec.grace = Duration::from_millis(200);
    let started = Instant::now();
    let out = run(&spec);
    assert!(out.stopped_early, "{out:?}");
    assert!(!out.timed_out);
    assert!(out.stdout.contains("\"text\":\"pong\""));
    assert!(started.elapsed() < Duration::from_secs(50));
}

#[test]
fn plain_removes_escape_codes_and_keeps_indentation() {
    assert_eq!(plain("\u{1b}[1mCommands:\u{1b}[0m\r\n  run   Run\r\n"), "Commands:\n  run   Run\n");
    assert_eq!(plain("a\u{1b}]0;title\u{7}b"), "ab");
}

#[test]
fn clean_masks_secrets_and_the_user_folder_name() {
    let text = clean("key sk-ant-api03-ABCDEFGHIJKLMNOP and C:\\Users\\SomeOne\\proj and /home/someone/x");
    assert!(!text.contains("ABCDEFGHIJ"), "{text}");
    assert!(!text.contains("SomeOne") && !text.contains("someone"), "{text}");
    assert!(text.contains("C:\\Users\\user\\proj") && text.contains("/home/user/x"), "{text}");
}

#[test]
fn cuts_respect_character_boundaries() {
    assert_eq!(cut_bytes("aéb", 2), "a");
    assert_eq!(cut_bytes("aéb", 3), "aé");
    assert_eq!(cut_chars("aéb", 2), "aé");
    assert_eq!(cut_chars("ab", 5), "ab");
}
