//! Starter crews: they parse and validate, `{default}` takes the default agent when a crew starts, the Start
//! sheet's per-role choices apply, and a copy is the only thing that is ever saved.

use super::agents::{pick_default, resolve_defaults, with_agents, AgentChoice};
use super::model::{Crew, DEFAULT_AGENT};
use super::starters;
use super::store;
use super::validate::{validate, AgentState};
use std::collections::BTreeMap;

fn state(id: &str) -> AgentState {
    match id {
        "alpha" | "beta" => AgentState::Ready,
        _ => AgentState::Unknown,
    }
}

fn choice(id: &str) -> AgentChoice {
    AgentChoice { id: id.into(), label: id.to_uppercase() }
}

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    std::env::temp_dir().join(format!("pm-crews-starter-{tag}-{}-{nanos}", std::process::id()))
}

fn starter(id: &str) -> Crew {
    starters::lookup(&format!("starter:{id}")).expect("a starter")
}

#[test]
fn the_three_starters_are_listed_by_name_and_parse() {
    let all = starters::all();
    let names: Vec<(String, String)> = all.iter().map(|(slug, c)| (slug.clone(), c.name.clone())).collect();
    assert_eq!(
        names,
        vec![
            ("starter:builder-reviewer".to_string(), "Builder + Reviewer".to_string()),
            ("starter:fix-the-tests".to_string(), "Fix the tests".to_string()),
            ("starter:research-write".to_string(), "Research + Write".to_string()),
        ]
    );
}

#[test]
fn every_starter_is_valid_before_and_after_the_default_agent_is_filled_in() {
    for (slug, crew) in starters::all() {
        // Unresolved: `{default}` has no registry entry yet, so it passes.
        assert!(validate(&crew, &state).is_empty(), "{slug} unresolved");
        // Resolved with a fake registry: the same checks as a copied or started crew.
        let resolved = resolve_defaults(&crew, Some("alpha")).unwrap();
        assert!(validate(&resolved, &state).is_empty(), "{slug} resolved");
        assert!(resolved.roles.iter().all(|r| r.agent_type == "alpha"), "{slug}");
    }
}

#[test]
fn every_starter_has_one_lead_and_helpers_under_it() {
    for (slug, crew) in starters::all() {
        let leads: Vec<&str> = crew.roles.iter().filter(|r| r.parent.is_none()).map(|r| r.name.as_str()).collect();
        assert_eq!(leads.len(), 1, "{slug}");
        assert!(crew.roles.len() == 2, "{slug} has two roles");
        assert!(crew.roles.iter().filter(|r| r.parent.is_some()).all(|r| r.parent.as_deref() == Some(leads[0])), "{slug}");
    }
}

#[test]
fn the_starter_roles_use_the_default_placeholder_not_an_agent_name() {
    for (slug, crew) in starters::all() {
        assert!(crew.roles.iter().all(|r| r.agent_type == DEFAULT_AGENT), "{slug}");
    }
}

#[test]
fn a_default_role_with_no_agent_to_give_stops_the_crew_and_names_the_role() {
    let crew = starter("builder-reviewer");
    let err = resolve_defaults(&crew, None).unwrap_err();
    assert_eq!(err.code, "CREW_NO_DEFAULT_AGENT");
    assert_eq!(err.role.as_deref(), Some("Builder"));
}

#[test]
fn the_default_agent_is_the_ask_bar_default_then_the_last_one_then_the_first() {
    let available = vec![choice("alpha"), choice("beta")];
    assert_eq!(pick_default(Some("beta"), Some("alpha"), &available).as_deref(), Some("beta"));
    assert_eq!(pick_default(Some("gone"), Some("alpha"), &available).as_deref(), Some("alpha"));
    assert_eq!(pick_default(None, Some("gone"), &available).as_deref(), Some("alpha"));
    assert_eq!(pick_default(None, None, &available).as_deref(), Some("alpha"));
    // Matching ignores case, and the id of the list is what comes back.
    assert_eq!(pick_default(Some("BETA"), None, &available).as_deref(), Some("beta"));
    assert_eq!(pick_default(Some("alpha"), None, &[]), None);
}

#[test]
fn the_start_sheet_choice_changes_only_the_roles_it_names() {
    let crew = starter("fix-the-tests");
    let mut chosen = BTreeMap::new();
    chosen.insert("fixer".to_string(), " beta ".to_string());
    let changed = with_agents(&crew, &chosen);
    assert_eq!(changed.roles[0].agent_type, "beta");
    assert_eq!(changed.roles[1].agent_type, DEFAULT_AGENT);
    // The rest of the crew is untouched.
    assert_eq!(changed.roles[1].task, crew.roles[1].task);
    assert_eq!(changed.roles[1].parent, crew.roles[1].parent);
}

#[test]
fn a_chosen_agent_that_is_not_set_up_is_refused_by_validation() {
    let mut chosen = BTreeMap::new();
    chosen.insert("Fixer".to_string(), "ghost".to_string());
    let changed = resolve_defaults(&with_agents(&starter("fix-the-tests"), &chosen), Some("alpha")).unwrap();
    let codes: Vec<&str> = validate(&changed, &state).into_iter().map(|e| e.code).collect();
    assert_eq!(codes, vec!["CREW_AGENT_UNKNOWN"]);
}

#[test]
fn a_starter_is_never_written_to_app_data_until_it_is_copied() {
    let dir = temp_dir("never-written");
    // Listing and reading starters writes nothing.
    let _ = starters::summaries(&state);
    let _ = store::load_any(&dir, "starter:builder-reviewer").unwrap();
    assert!(store::list(&dir, &state).is_empty());
    assert!(!dir.exists());
}

#[test]
fn copying_a_starter_saves_an_ordinary_crew_the_person_can_keep() {
    let dir = temp_dir("copy");
    let resolved = resolve_defaults(&starter("research-write"), Some("alpha")).unwrap();
    let slug = store::save(&dir, &resolved, false, &state).expect("the copy saves");
    assert_eq!(slug, "research-write");
    assert_eq!(store::load(&dir, &slug).unwrap(), resolved);
    let listed = store::list(&dir, &state);
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, "Research + Write");
    assert!(listed[0].problem.is_none());
    // The starter itself is still there, unchanged.
    assert_eq!(starter("research-write").roles[0].agent_type, DEFAULT_AGENT);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_starter_slug_is_not_a_file_of_ours_so_it_cannot_be_removed_or_overwritten() {
    let dir = temp_dir("slugs");
    assert_eq!(store::remove(&dir, "starter:builder-reviewer").unwrap_err().code, "CREW_NOT_FOUND");
    assert!(starters::lookup("starter:no-such-starter").is_none());
    assert!(starters::lookup("builder-reviewer").is_none());
    assert_eq!(store::load_any(&dir, "starter:no-such-starter").unwrap_err().code, "CREW_NOT_FOUND");
}

#[test]
fn the_starter_list_shows_each_crew_with_its_roles_and_no_problem() {
    let list = starters::summaries(&state);
    let rows: Vec<(usize, Vec<String>, bool)> = list.iter().map(|s| (s.roles, s.agents.clone(), s.problem.is_none())).collect();
    assert_eq!(rows, vec![(2, vec![DEFAULT_AGENT.to_string()], true), (2, vec![DEFAULT_AGENT.to_string()], true), (2, vec![DEFAULT_AGENT.to_string()], true)]);
}
