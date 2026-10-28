use super::super::model::{AttemptEnd, AttemptKind, Event, Review};
use super::super::report;
use super::*;
use crate::missions::model::AgentChoice;

fn ctx() -> RedactContext {
    RedactContext { env_values: Vec::new(), home: Some("C:\\Users\\you".into()) }
}

fn json<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).expect("serialize")
}

#[test]
fn counts_and_line_follow_the_report() {
    let run = morning_fixture();
    let view = view_of(&run, &ctx(), 0);
    assert_eq!((view.total, view.done, view.green, view.needs_you, view.didnt_run), (3, 1, 1, 1, 1));
    assert_eq!(view.line, report::line(&report::summary(&run)));
    assert_eq!(view.line, "1 of 3 done, tests green. 1 needs you. 1 didn't run.");
    assert_eq!(view.state, "running");
    assert_eq!(view.from, "01:00");
    assert_eq!(view.to.as_deref(), Some("02:00"));
    assert_eq!(view.turns, 15);
    assert_eq!(view.duration, "2 h 8 min");
    assert_eq!(view.by_plan, vec![PlanCount { label: "Codex".into(), tasks: 2 }]);
}

#[test]
fn keep_rules_follow_class_review_and_copy() {
    let view = view_of(&morning_fixture(), &ctx(), 0);
    let [ready, needs, skipped] = &view.tasks[..] else { panic!("three tasks") };
    assert!(ready.can_keep && ready.can_throw_away, "a ready copy can be kept or thrown away");
    assert!(needs.can_keep, "a needs-a-look copy can be kept after its confirm line");
    assert!(!skipped.can_keep, "a task that did not run has no copy to keep");
    assert!(skipped.can_throw_away, "a task that did not run can only be thrown away");
    assert_eq!(view.keep_all, 1, "only the ready copy passes Keep all");
    assert_eq!(ready.class.as_deref(), Some("ready"));
    assert_eq!(needs.class.as_deref(), Some("needs_look"));
}

#[test]
fn kept_or_thrown_away_tasks_offer_no_buttons() {
    let mut run = morning_fixture();
    run.tasks[0].review = Review::Kept { at_ms: 5, into: "main".into() };
    run.tasks[1].review = Review::Discarded { at_ms: 6 };
    let view = view_of(&run, &ctx(), 0);
    assert!(!view.tasks[0].can_keep && !view.tasks[0].can_throw_away);
    assert_eq!(view.tasks[0].review, "kept");
    assert_eq!(view.tasks[0].review_text.as_deref(), Some("Kept · merged into main"));
    assert_eq!(view.tasks[0].into.as_deref(), Some("main"));
    assert_eq!(view.tasks[1].review_text.as_deref(), Some("Thrown away"));
    assert!(!view.tasks[1].can_keep);
    assert_eq!(view.keep_all, 0);
}

#[test]
fn keep_all_counts_exactly_what_keep_all_ready_keeps() {
    let mut run = morning_fixture();
    // A second ready copy, one still running, one that conflicted before, and a no-checks copy.
    let mut second = run.tasks[0].clone();
    second.spec.id = "nt-00000004".into();
    let mut running = run.tasks[0].clone();
    running.spec.id = "nt-00000005".into();
    running.attempts[0].end = None;
    let mut conflicted = run.tasks[0].clone();
    conflicted.spec.id = "nt-00000006".into();
    conflicted.review = Review::KeepFailed { at_ms: 1, sentence: "This conflicts.".into() };
    let mut no_checks = run.tasks[1].clone();
    no_checks.spec.id = "nt-00000007".into();
    if let Some(outcome) = no_checks.outcome.as_mut() {
        outcome.class = Class::NoChecks;
    }
    run.tasks.extend([second, running, conflicted, no_checks]);
    let view = view_of(&run, &ctx(), 0);
    let bulk = run.tasks.iter().filter(|task| review::keeps_in_bulk(task)).count();
    assert_eq!(view.keep_all as usize, bulk);
    assert_eq!(view.keep_all, 2, "the first ready copy and the second one; not the needs-a-look or no-checks copies");
}

#[test]
fn a_conflicted_copy_can_be_thrown_away_but_not_kept_again_from_its_row() {
    let mut run = morning_fixture();
    run.tasks[0].review = Review::KeepFailed { at_ms: 4, sentence: "This conflicts with a task you already kept (src/cart.ts).".into() };
    let view = view_of(&run, &ctx(), 0);
    let row = &view.tasks[0];
    assert_eq!(row.review, "keep_failed");
    assert!(row.can_throw_away, "the copy stays after a conflict, so it can be thrown away");
    assert!(!row.can_keep);
    assert_eq!(row.review_text.as_deref(), Some("This conflicts with a task you already kept (src/cart.ts)."));
    assert_eq!(view.keep_all, 0, "the conflicted task is not counted");
}

#[test]
fn a_pending_copy_names_the_branch_keep_would_merge_into() {
    let run = morning_fixture();
    let view = view_of_with(&run, &ctx(), 0, &|project| {
        assert_eq!(project, "C:\\Users\\you\\work\\app", "the lookup gets the task's project");
        Some("main".into())
    });
    assert_eq!(view.tasks[0].into.as_deref(), Some("main"), "a pending copy with a copy");
    assert_eq!(view.tasks[1].into.as_deref(), Some("main"), "a needs-a-look copy too");
    assert_eq!(view.tasks[2].into, None, "a task that did not run has no copy to merge");
    let none = view_of_with(&run, &ctx(), 0, &|_| None);
    assert_eq!(none.tasks[0].into, None, "a detached project has no branch to name");
    assert_eq!(view_of(&run, &ctx(), 0).tasks[0].into, None, "the bridge view does no lookup");
}

#[test]
fn a_kept_copy_keeps_the_branch_it_merged_into_and_no_lookup_runs() {
    let mut run = morning_fixture();
    run.tasks[0].review = Review::Kept { at_ms: 5, into: "release".into() };
    let view = view_of_with(&run, &ctx(), 0, &|_| Some("main".into()));
    assert_eq!(view.tasks[0].into.as_deref(), Some("release"));
    assert_eq!(view.tasks[0].review_text.as_deref(), Some("Kept · merged into release"));
    let mut thrown = morning_fixture();
    thrown.tasks[0].review = Review::Discarded { at_ms: 6 };
    let view = view_of_with(&thrown, &ctx(), 0, &|_| panic!("a thrown-away copy has no branch to look up"));
    assert_eq!(view.tasks[0].into, None);
}

#[test]
fn an_older_night_is_read_only() {
    let mut view = view_of(&morning_fixture(), &ctx(), 0);
    assert!(view.tasks[0].can_keep && view.tasks[0].can_throw_away && view.keep_all == 1);
    view.read_only();
    assert_eq!(view.keep_all, 0);
    assert!(view.tasks.iter().all(|task| !task.can_keep && !task.can_throw_away));
    assert_eq!(view.tasks[0].title, "Fix the cart total rounding.", "the details stay");
}

#[test]
fn title_is_the_first_line_cut_to_80_characters() {
    assert_eq!(title_of("Rename the config loader.\nSecond line", &ctx()), "Rename the config loader.");
    let long = "word ".repeat(40);
    let title = title_of(&long, &ctx());
    assert_eq!(title.chars().count(), TITLE_CHARS);
    assert!(title.ends_with('…'));
    assert_eq!(title_of("   \n  ", &ctx()), "");
}

#[test]
fn title_and_sentences_have_no_absolute_paths_or_secrets() {
    let ctx = ctx();
    let title = title_of("Fix C:\\Users\\you\\work\\app\\src\\cart.rs with ghp_abcdefghijklmnopqrstuvwxyz0123456789", &ctx);
    assert!(!title.contains("Users"), "{title}");
    assert!(!title.contains("ghp_abc"), "{title}");
    assert!(title.contains("cart.rs"), "{title}");
    let sentence = turn_text("Used sk-ant-api03-abcdefghijklmnopqrstuvwxyz123456 in /home/you/app", &ctx);
    assert!(!sentence.contains("sk-ant-api03"), "{sentence}");
    assert!(!sentence.contains("/home/you/"), "{sentence}");
}

#[test]
fn desktop_view_keeps_the_folder_name_and_the_pane_for_the_terminal() {
    let view = view_of(&morning_fixture(), &ctx(), 0);
    assert_eq!(view.tasks[0].project.as_deref(), Some("app"));
    assert_eq!(view.tasks[0].pane_id.as_deref(), Some("pane-42"));
    let sentence = &view.tasks[1].summaries[0];
    assert!(sentence.starts_with("<script>alert(1)</script> at "), "{sentence}");
    assert!(sentence.ends_with("lib.rs"), "{sentence}");
}

#[test]
fn bridge_view_has_no_paths_pane_ids_or_turn_sentences() {
    let view = view_of(&morning_fixture(), &ctx(), 0).redacted_for_bridge();
    let text = json(&view);
    assert!(!text.contains('\\'), "a backslash leaked: {text}");
    assert!(!text.contains("/home/"), "{text}");
    assert!(!text.contains("pane-4"), "a pane id leaked: {text}");
    assert!(!text.contains("Users"), "{text}");
    for task in &view.tasks {
        assert_eq!(task.project, None);
        assert_eq!(task.pane_id, None);
        assert!(task.summaries.is_empty());
        assert!(task.test_change_lines.is_empty());
        assert!(task.attempts.iter().all(|a| a.sentence.is_none()));
    }
    assert_eq!(view.tasks[0].title, "Fix the cart total rounding.");
    assert_eq!(view.line, "1 of 3 done, tests green. 1 needs you. 1 didn't run.");
}

#[test]
fn bridge_view_cuts_event_text_to_80_characters() {
    let mut run = morning_fixture();
    run.events.push(Event { at_ms: 1, kind: "note".into(), glyph: ":".into(), text: "word ".repeat(60), task: None });
    let view = view_of(&run, &ctx(), 0).redacted_for_bridge();
    let last = view.events.last().expect("event");
    assert_eq!(last.text.chars().count(), EVENT_BRIDGE_CHARS);
}

#[test]
fn json_uses_camel_case_names() {
    let text = json(&view_of(&morning_fixture(), &ctx(), 0));
    for name in ["\"needsYou\"", "\"keepAll\"", "\"canKeep\"", "\"stateText\"", "\"testsBefore\"", "\"paneId\"", "\"byPlan\""] {
        assert!(text.contains(name), "{name} missing");
    }
    for name in ["needs_you", "can_keep", "state_text", "pane_id"] {
        assert!(!text.contains(name), "{name} should not be in the JSON");
    }
}

#[test]
fn plan_view_serializes_in_camel_case() {
    let plan = PlanView {
        key: "codex".into(),
        label: "Codex".into(),
        ready: false,
        why_not: Some("Out of usage".into()),
        out_until: Some("17:00".into()),
        tried_here: [("app".to_string(), true)].into_iter().collect(),
    };
    assert_eq!(
        json(&plan),
        r#"{"key":"codex","label":"Codex","ready":false,"whyNot":"Out of usage","outUntil":"17:00","triedHere":{"app":true}}"#
    );
}

#[test]
fn attempt_ends_and_outcome_facts_come_through() {
    let view = view_of(&morning_fixture(), &ctx(), 0);
    assert_eq!(view.tasks[1].attempts[0].end, "gate");
    assert_eq!(view.tasks[1].attempts[0].sentence.as_deref(), Some("Codex asked to trust the folder."));
    assert_eq!(view.tasks[0].attempts[0].end, "completed");
    assert_eq!(view.tasks[0].files, Some(FilesView { count: 5, added: 120, removed: 34 }));
    assert_eq!(view.tasks[0].duration.as_deref(), Some("38 min"));
    assert_eq!(view.tasks[2].class.as_deref(), Some("didnt_run"));
    assert_eq!(view.tasks[2].state, "finished");

    let mut run = morning_fixture();
    run.tasks[2].attempts = vec![Attempt {
        n: 1,
        plan: AgentChoice { agent_type: "codex".into(), account: None, model: None },
        label: "Codex".into(),
        kind: AttemptKind::Single,
        mission_id: None,
        handle: None,
        pane_ids: Vec::new(),
        started_ms: 3_600_000,
        ended_ms: None,
        end: Some(AttemptEnd::OutOfUsage { reset_at_ms: None }),
    }];
    let view = view_of(&run, &ctx(), 0);
    assert_eq!(view.tasks[2].attempts[0].end, "out_of_usage");
    assert_eq!(view.tasks[2].attempts[0].ended, None);
}
