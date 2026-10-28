use super::super::model::{sample_task, Class, Counts, NightRun, Outcome, TaskState};
use super::super::report;
use super::super::view::morning_fixture;
use super::*;

fn ctx() -> RedactContext {
    RedactContext { env_values: Vec::new(), home: Some("C:\\Users\\you".into()) }
}

#[test]
fn page_has_the_summary_line_and_one_block_per_task() {
    let run = morning_fixture();
    let html = page(&run, &ctx());
    let line = report::line(&report::summary(&run));
    assert!(html.contains(&escape(&line)), "summary line missing");
    assert!(html.starts_with("<!doctype html>"));
    assert_eq!(html.matches("<section class=\"task\">").count(), 3);
    assert!(html.contains("<h2>Fix the cart total rounding.</h2>"));
    assert!(html.contains("Tests: 9 of 12 pass (was 7 of 12)"));
    assert!(html.contains("5 files · +120 -34"));
    assert!(html.contains("Didn&#39;t run: no time left before 07:00."), "the skipped task's headline is missing");
}

#[test]
fn page_escapes_script_in_a_turn_sentence() {
    let html = page(&morning_fixture(), &ctx());
    assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt; at lib.rs"), "{html}");
    assert!(!html.contains("<script>"), "a script tag must never reach the page");
}

#[test]
fn page_has_no_absolute_path_pane_id_or_home_dir() {
    let html = page(&morning_fixture(), &ctx());
    assert!(!html.contains("Users"), "home folder leaked");
    assert!(!html.contains('\\'), "a windows path leaked");
    assert!(!html.contains("/home/"), "a unix home path leaked");
    assert!(!html.contains("pane-42") && !html.contains("pane-43"), "a pane id leaked");
    assert!(!html.contains("ns/nt-00000001"), "a copy branch leaked");
    // The folder name stays; the full path does not.
    assert!(html.contains("<span>app</span>"));
    assert!(html.contains("<span>docs</span>"));
}

#[test]
fn page_redacts_secrets_in_agent_text() {
    let mut run = morning_fixture();
    if let Some(outcome) = run.tasks[1].outcome.as_mut() {
        outcome.summaries.push("Set token ghp_abcdefghijklmnopqrstuvwxyz0123456789 for the build".into());
    }
    let html = page(&run, &ctx());
    assert!(!html.contains("ghp_abc"), "a token leaked into the page");
}

#[test]
fn page_stays_under_1_mb_for_ten_tasks() {
    let mut run = NightRun { tasks: Vec::new(), ..morning_fixture() };
    let long = "x".repeat(2_000);
    for i in 0..10 {
        let mut task = sample_task(&format!("nt-0000000{i}"));
        task.spec.text = long.clone();
        task.state = TaskState::Finished;
        task.outcome = Some(Outcome {
            class: Class::NeedsLook,
            headline: "Tests fail: 9 of 12 pass".into(),
            reason: None,
            tests: Some(Counts { passed: 9, total: 12 }),
            tests_before: None,
            checks_declared: true,
            test_change_lines: vec![long.clone(); 30],
            files: Some((1, 1, 1)),
            turns: 12,
            minutes: 90,
            summaries: vec![long.clone(); 20],
            copy: None,
        });
        run.tasks.push(task);
    }
    let html = page(&run, &ctx());
    assert!(html.len() < 1_000_000, "the page is {} bytes", html.len());
    assert_eq!(html.matches("<section class=\"task\">").count(), 10);
}

#[test]
fn page_for_a_run_with_no_tasks_says_zero() {
    let run = NightRun { tasks: Vec::new(), ..morning_fixture() };
    let html = page(&run, &ctx());
    assert!(html.contains("0 of 0 done"), "{html}");
    assert!(!html.contains("<section"));
}
