//! The pure `build` with fixtures (SC-10): notes wording, project choice by id and by path, a
//! copy's folder mapped to its origin, unknown projects, the window and the `days` check. The
//! roll-up itself is tested in `aggregate`; here only what `build` adds around it.

use super::*;
use crate::scores::model::{CopyState, End, ScoreRun, ScoreTurn};
use chrono::Utc;

const NOW: u64 = 1_800_000_000_000; // 2027-01-15 08:00 UTC
const DAY: u64 = 86_400_000;

fn agent_label(id: &str) -> String {
    match id {
        "agent-a" => "Agent A".to_string(),
        other => other.to_string(),
    }
}

fn account_label(_id: &str) -> Option<String> {
    None
}

fn labels() -> Labels<'static> {
    Labels { agent: &agent_label, account: &account_label }
}

fn shop() -> String {
    key_of_path("C:/Users/you/shop")
}

fn blog() -> String {
    key_of_path("C:/Users/you/blog")
}

fn turn(project: &str, started_ms: u64) -> ScoreTurn {
    ScoreTurn {
        project: project.to_string(),
        agent_id: "agent-a".to_string(),
        account: None,
        pane_id: None,
        started_ms,
        ended_ms: started_ms + 60_000,
        end: End::Ok,
        files_changed: None,
        lines: None,
        asked_you: None,
        checks_green: None,
        undone: None,
    }
}

fn run(project: &str, last_started_ms: u64) -> ScoreRun {
    ScoreRun {
        project: project.to_string(),
        agent_id: "agent-a".to_string(),
        account: None,
        turns: 1,
        busy_ms: 60_000,
        last_started_ms,
        copy: CopyState::Merged,
    }
}

fn inputs() -> Inputs {
    let mut inputs = Inputs::default();
    inputs.turns = (0..6).map(|i| turn(&shop(), NOW - DAY * 2 - i * 60_000)).collect();
    inputs.turns.extend((0..3).map(|i| turn(&blog(), NOW - DAY * 40 - i * 60_000)));
    inputs.project_names.insert(shop(), "shop".to_string());
    inputs.project_names.insert(blog(), "blog".to_string());
    inputs
}

fn report(inputs: &Inputs, project: Option<&str>, days: Option<u32>) -> ScoresReport {
    build_in(inputs, project, days, NOW, &Utc, &labels())
}

fn turns_of(report: &ScoresReport) -> u32 {
    report.rows.iter().map(|r| r.turns).sum()
}

#[test]
fn all_projects_counts_everything_and_lists_projects_newest_first() {
    let r = report(&inputs(), None, None);
    assert_eq!(r.scope, "all");
    assert!(r.project.is_none());
    assert_eq!(turns_of(&r), 9);
    let names: Vec<&str> = r.projects.iter().map(|p| p.label.as_str()).collect();
    assert_eq!(names, ["shop", "blog"]);
    assert_eq!(r.projects[0].turns, 6);
    assert_eq!(r.projects[1].turns, 3);
    assert_eq!(r.min_sample, 5);
    assert_eq!(r.days, None);
    assert_eq!(r.since_ms, 0);
}

#[test]
fn choose_a_project_by_the_id_of_an_earlier_answer() {
    let first = report(&inputs(), None, None);
    let id = first.projects.iter().find(|p| p.label == "blog").unwrap().id.clone();
    assert_eq!(id.len(), 12);
    assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    let r = report(&inputs(), Some(&id), None);
    assert_eq!(r.scope, "project");
    assert_eq!(r.project.as_ref().map(|p| p.label.as_str()), Some("blog"));
    assert_eq!(turns_of(&r), 3);
}

#[test]
fn choose_a_project_by_folder_path() {
    let r = report(&inputs(), Some("C:/Users/you/shop"), None);
    assert_eq!(r.scope, "project");
    assert_eq!(r.project.as_ref().map(|p| p.label.as_str()), Some("shop"));
    assert_eq!(turns_of(&r), 6);
}

#[test]
fn a_copy_folder_maps_to_its_origin_project() {
    let r = report(&inputs(), Some("C:\\Users\\you\\shop\\.puppet-master\\worktrees\\try-1"), None);
    assert_eq!(r.project.as_ref().map(|p| p.label.as_str()), Some("shop"));
    assert_eq!(turns_of(&r), 6);
}

#[test]
fn an_unknown_path_gives_the_project_scope_with_no_rows_and_a_note() {
    let r = report(&inputs(), Some("C:/Users/you/other"), None);
    assert_eq!(r.scope, "project");
    assert!(r.rows.is_empty());
    let info = r.project.expect("the project is named");
    assert_eq!(info.label, "other");
    assert_eq!(info.turns, 0);
    assert!(r.notes.iter().any(|n| n == "No turns have been recorded for this project yet."));
    // The other projects stay selectable.
    assert_eq!(r.projects.len(), 2);
}

#[test]
fn an_unknown_id_counts_nothing_even_for_turns_with_no_folder() {
    let mut i = inputs();
    i.turns.push(turn("", NOW - 1_000));
    let r = report(&i, Some("0123456789ab"), None);
    assert_eq!(r.scope, "project");
    assert!(r.rows.is_empty());
    assert_eq!(r.project.map(|p| p.id), Some("0123456789ab".to_string()));
    // A turn with no known folder still counts for all projects, but is not listed as a project.
    let all = report(&i, None, None);
    assert_eq!(turns_of(&all), 10);
    assert_eq!(all.projects.len(), 2);
}

#[test]
fn blank_project_means_all_projects() {
    let r = report(&inputs(), Some("  "), None);
    assert_eq!(r.scope, "all");
}

#[test]
fn the_window_leaves_out_older_turns_and_counts_project_turns_inside_it() {
    let r = report(&inputs(), None, Some(30));
    assert_eq!(turns_of(&r), 6);
    assert_eq!(r.days, Some(30));
    assert_eq!(r.since_ms, NOW - 30 * DAY);
    // The blog is still listed (it has history), with no turns in the window.
    let blog = r.projects.iter().find(|p| p.label == "blog").unwrap();
    assert_eq!(blog.turns, 0);
    assert_eq!(report(&inputs(), None, Some(90)).rows.iter().map(|r| r.turns).sum::<u32>(), 9);
}

#[test]
fn runs_count_in_the_project_list_and_order_it() {
    let mut i = Inputs::default();
    i.runs.push(run(&blog(), NOW - DAY));
    i.turns.push(turn(&shop(), NOW - 5 * DAY));
    let r = report(&i, None, None);
    assert_eq!(r.projects.len(), 2);
    assert_eq!(r.projects[0].label, "blog");
}

#[test]
fn two_folders_with_one_name_get_a_number() {
    let mut i = Inputs::default();
    let (a, b) = (key_of_path("C:/Users/you/a/app"), key_of_path("C:/Users/you/b/app"));
    i.turns.push(turn(&a, NOW - DAY));
    i.turns.push(turn(&b, NOW - 2 * DAY));
    i.project_names.insert(a, "app".to_string());
    i.project_names.insert(b, "app".to_string());
    let labels: Vec<String> = report(&i, None, None).projects.into_iter().map(|p| p.label).collect();
    assert_eq!(labels, ["app", "app (2)"]);
}

#[test]
fn a_project_with_no_stored_name_is_named_from_its_key_and_the_list_is_capped() {
    let mut i = Inputs::default();
    for n in 0..60u64 {
        i.turns.push(turn(&key_of_path(&format!("C:/Users/you/p{n}")), NOW - n * 1_000));
    }
    let r = report(&i, None, None);
    assert_eq!(r.projects.len(), 50);
    assert_eq!(r.projects[0].label.to_lowercase(), "p0");
    // A project past the cap can still be chosen by path.
    let far = report(&i, Some("C:/Users/you/p59"), None);
    assert_eq!(turns_of(&far), 1);
}

#[test]
fn project_ids_are_stable_and_twelve_hex_characters() {
    assert_eq!(project_id("c:/users/you/shop"), project_id("c:/users/you/shop"));
    assert_ne!(project_id("c:/users/you/shop"), project_id("c:/users/you/blog"));
    assert_eq!(project_id("c:/users/you/shop").len(), 12);
}

#[test]
fn undo_note_says_since_when_or_from_now_on() {
    let none = report(&inputs(), None, None);
    assert_eq!(none.notes[0], "Counting Undo from now on.");

    let mut i = inputs();
    i.undo_since_ms = Some(NOW - 3 * DAY);
    // All time: earlier undos were not recorded.
    let all = report(&i, None, None);
    assert_eq!(all.notes[0], "Counting Undo since 12 January 2027. Earlier undos were not recorded.");
    // The window starts after the log began: nothing earlier is missing.
    let recent = report(&i, None, Some(1));
    assert_eq!(recent.notes[0], "Counting Undo since 12 January 2027.");
}

#[test]
fn notes_say_what_is_left_out() {
    let mut i = inputs();
    i.turns[0].files_changed = Some(2);
    i.turns[1].checks_green = Some(true);
    i.unreadable_records = 2;
    let notes = report(&i, Some("C:/Users/you/shop"), None).notes;
    assert!(notes.contains(&"5 turns had no file information, so they are left out of Undone and Asked you.".to_string()), "{notes:?}");
    assert!(notes.contains(&"1 of 6 turns had checks. Checks count only when a run is started with them.".to_string()), "{notes:?}");
    assert!(notes.contains(
        &"Tokens are not counted. Only usage an agent reports itself could show here, and it is not saved per turn.".to_string()
    ));
    assert!(notes.contains(&"2 records could not be read. Their turns are not counted.".to_string()), "{notes:?}");

    i.unreadable_records = 1;
    i.turns.truncate(1);
    let one = report(&i, None, None).notes;
    assert!(one.contains(&"1 record could not be read. Its turns are not counted.".to_string()), "{one:?}");
    assert!(one.contains(&"0 of 1 turns had checks. Checks count only when a run is started with them.".to_string()), "{one:?}");
}

#[test]
fn no_turns_means_no_file_or_check_notes() {
    let notes = report(&Inputs::default(), None, None).notes;
    assert_eq!(notes.len(), 2, "{notes:?}");
    assert!(notes[0].starts_with("Counting Undo"));
    assert!(notes[1].starts_with("Tokens are not counted"));
}

#[test]
fn days_must_be_one_to_3650() {
    assert!(check_days(None).is_ok());
    assert!(check_days(Some(1)).is_ok());
    assert!(check_days(Some(3650)).is_ok());
    for bad in [0u32, 3651, 4000] {
        let err = agent_scores(None, Some(bad)).unwrap_err();
        assert!(err.starts_with("INVALID_ARGUMENT: days"), "{err}");
    }
}

#[test]
fn the_report_names_projects_by_last_folder_name_only() {
    let mut i = Inputs::default();
    let key = key_of_path("C:/Users/maya/secret-client/shop");
    i.turns.push(turn(&key, NOW - DAY));
    // The stored name is a full path by mistake: still only the last name leaves.
    i.project_names.insert(key, "C:\\Users\\maya\\secret-client\\shop".to_string());
    let json = serde_json::to_string(&report(&i, Some("C:/Users/maya/secret-client/shop"), None)).unwrap().to_lowercase();
    assert!(json.contains("\"label\":\"shop\""), "{json}");
    assert!(!json.contains("maya"), "{json}");
    assert!(!json.contains("secret-client"), "{json}");
    assert!(!json.contains("c:/"), "{json}");
}
