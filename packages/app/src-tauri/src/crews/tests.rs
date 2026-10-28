use super::draft::{draft, Env, TreePane};
use super::model::{parse, to_toml, Crew, Role, MAX_ROLES};
use super::start::{run, start_order, StartFailure, Starter, Step};
use super::store;
use super::validate::{validate, AgentState};

fn state(id: &str) -> AgentState {
    match id {
        "alpha" | "beta" | "wizard_made" => AgentState::Ready,
        "plain_shell" => AgentState::NotDispatchable("it is a plain shell".into()),
        _ => AgentState::Unknown,
    }
}

fn role(name: &str, agent: &str, parent: Option<&str>) -> Role {
    Role { name: name.into(), agent_type: agent.into(), task: format!("Do the {name} job in {{folder}}"), parent: parent.map(String::from) }
}

fn crew(roles: Vec<Role>) -> Crew {
    Crew { name: "Review team".into(), roles }
}

fn codes(c: &Crew) -> Vec<&'static str> {
    validate(c, &state).into_iter().map(|e| e.code).collect()
}

const FILE: &str = r#"
name = "Review team"

[[roles]]
name = "Lead"
agent_type = "alpha"
task = "Plan the review of {folder}"

[[roles]]
name = "Checker"
agent_type = "wizard_made"
parent = "Lead"
task = "Run the checks in {folder}"
"#;

// ------------------------------------------------------------ parse and validate

#[test]
fn a_crew_file_parses_validates_and_round_trips() {
    let c = parse(FILE).unwrap();
    assert_eq!(c.roles.len(), 2);
    assert_eq!(c.roles[1].parent.as_deref(), Some("Lead"));
    assert!(validate(&c, &state).is_empty());
    assert_eq!(parse(&to_toml(&c).unwrap()).unwrap(), c);
}

#[test]
fn an_unknown_key_or_a_huge_file_is_refused() {
    assert_eq!(parse("name = \"x\"\nrun = \"rm -rf /\"\n").unwrap_err().code, "CREW_FILE_INVALID");
    assert_eq!(parse(&"#".repeat(70_000)).unwrap_err().code, "CREW_FILE_TOO_BIG");
}

#[test]
fn an_unknown_agent_is_refused_and_a_wizard_made_one_is_not() {
    let bad = crew(vec![role("Lead", "ghost", None)]);
    assert_eq!(codes(&bad), vec!["CREW_AGENT_UNKNOWN"]);
    assert!(codes(&crew(vec![role("Lead", "wizard_made", None)])).is_empty());
    assert_eq!(codes(&crew(vec![role("Lead", "plain_shell", None)])), vec!["CREW_AGENT_NOT_DISPATCHABLE"]);
}

#[test]
fn an_agent_id_with_shell_characters_is_refused() {
    for id in ["alpha; rm -rf /", "alpha && x", "$(alpha)", "a b", "..\\alpha", "alpha\n"] {
        assert!(codes(&crew(vec![role("Lead", id, None)])).contains(&"CREW_AGENT_UNKNOWN"), "{id:?}");
    }
}

#[test]
fn a_cycle_is_refused() {
    let c = crew(vec![role("Lead", "alpha", None), role("A", "alpha", Some("B")), role("B", "alpha", Some("A"))]);
    let found = codes(&c);
    assert!(found.contains(&"CREW_CYCLE"), "{found:?}");
    let own = crew(vec![role("Lead", "alpha", None), role("A", "alpha", Some("A"))]);
    assert!(codes(&own).contains(&"CREW_CYCLE"));
}

#[test]
fn a_loop_with_no_root_is_refused_too() {
    let c = crew(vec![role("A", "alpha", Some("B")), role("B", "alpha", Some("A"))]);
    let found = codes(&c);
    assert!(found.contains(&"CREW_ROOTS") && found.contains(&"CREW_CYCLE"), "{found:?}");
}

#[test]
fn at_most_eight_roles_and_exactly_one_root() {
    let mut roles = vec![role("Lead", "alpha", None)];
    for n in 0..MAX_ROLES {
        roles.push(role(&format!("H{n}"), "beta", Some("Lead")));
    }
    assert_eq!(roles.len(), 9);
    assert!(codes(&crew(roles.clone())).contains(&"CREW_TOO_MANY_ROLES"));
    roles.truncate(MAX_ROLES);
    assert!(codes(&crew(roles)).is_empty());
    assert!(codes(&crew(vec![role("A", "alpha", None), role("B", "alpha", None)])).contains(&"CREW_ROOTS"));
    assert!(codes(&crew(vec![])).contains(&"CREW_NO_ROLES"));
}

#[test]
fn names_and_parents_must_make_sense() {
    assert!(codes(&crew(vec![role("Lead", "alpha", None), role("lead", "beta", Some("Lead"))])).contains(&"CREW_DUPLICATE_ROLE"));
    assert!(codes(&crew(vec![role("Lead", "alpha", None), role("B", "beta", Some("Nobody"))])).contains(&"CREW_PARENT_UNKNOWN"));
    assert!(codes(&crew(vec![role("  ", "alpha", None)])).contains(&"CREW_ROLE_NAME"));
    let mut c = crew(vec![role("Lead", "alpha", None)]);
    c.name = "x".repeat(61);
    assert!(codes(&c).contains(&"CREW_NAME"));
}

#[test]
fn a_task_is_plain_text_with_folder_as_its_only_placeholder() {
    let with = |task: &str| {
        let mut c = crew(vec![role("Lead", "alpha", None)]);
        c.roles[0].task = task.into();
        codes(&c)
    };
    assert!(with("Check {folder} and print { \"a\": 1 } and {1}").is_empty());
    assert_eq!(with("   "), vec!["CREW_TASK"]);
    assert_eq!(with("Look at {project}"), vec!["CREW_TASK"]);
    assert_eq!(with("bad \u{0}byte"), vec!["CREW_TASK"]);
    assert_eq!(with("bad \u{1b}[31mcolour"), vec!["CREW_TASK"]);
    assert_eq!(with(&"x".repeat(4001)), vec!["CREW_TASK"]);
    assert!(with("line one\nline two\ttabbed").is_empty());
}

// ------------------------------------------------------------ a draft from a live tree

fn pane(id: &str, agent: &str, parent: Option<&str>, title: Option<&str>) -> TreePane {
    TreePane {
        id: id.into(),
        agent_type: agent.into(),
        title: title.map(String::from),
        folders: vec![r"C:\Work\Shop".into()],
        parent: parent.map(String::from),
    }
}

fn first_task(id: &str) -> Option<String> {
    match id {
        "p1" => Some(r"Plan the review of C:\Work\Shop. Token sk-ant-api03-SECRETSECRETSECRET1234 must not leak.".into()),
        "p2" => Some("Run the tests in c:/work/shop/ and in C:\\Work\\Shop2 too".into()),
        "p3" => Some("Summarise the test results".into()),
        _ => None,
    }
}

fn label(agent: &str) -> String {
    format!("Agent {agent}")
}

fn tree() -> Vec<TreePane> {
    vec![
        pane("other", "alpha", None, None),
        pane("p1", "alpha", None, Some("Lead")),
        pane("p2", "beta", Some("p1"), None),
        pane("p3", "beta", Some("p2"), None),
        pane("p4", "alpha", Some("other"), None),
    ]
}

fn real_redact(text: &str) -> String {
    let ctx = crate::redact::RedactContext { env_values: vec![], home: Some(r"C:\Users\you-pc".into()) };
    crate::redact::strict_with(text, &ctx).into_inner()
}

#[test]
fn a_draft_follows_the_tree_and_swaps_the_folder_for_the_placeholder() {
    let env = Env { first_task_of: &first_task, redact: &real_redact, label_of: &label };
    let d = draft(&tree(), "p1", &env).unwrap();
    assert_eq!(d.omitted, 0);
    let roles = &d.crew.roles;
    assert_eq!(roles.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), vec!["Lead", "Agent beta", "Agent beta 2"]);
    assert_eq!(roles.iter().map(|r| r.parent.as_deref()).collect::<Vec<_>>(), vec![None, Some("Lead"), Some("Agent beta")]);
    assert_eq!(roles[0].agent_type, "alpha");
    assert!(roles[0].task.starts_with("Plan the review of {folder}."), "{}", roles[0].task);
    // Both slash styles and any case; a longer folder name that merely starts the same is left alone.
    assert_eq!(roles[1].task, "Run the tests in {folder}/ and in C:\\Work\\Shop2 too");
    assert_eq!(roles[2].task, "Summarise the test results");
    assert_eq!(d.crew.name, "Lead team");
    // The helpers of another tree are not in this one.
    assert!(!roles.iter().any(|r| r.name == "Agent alpha"));
}

#[test]
fn a_draft_is_redacted_after_the_folder_is_swapped() {
    let env = Env { first_task_of: &first_task, redact: &real_redact, label_of: &label };
    let d = draft(&tree(), "p1", &env).unwrap();
    assert!(!d.crew.roles[0].task.contains("SECRETSECRET"), "{}", d.crew.roles[0].task);
    assert!(d.crew.roles[0].task.contains("{folder}"));
    // Strict redaction also hides a personal folder name elsewhere in the text.
    let home = |_: &str| Some(r"Read C:\Users\you-pc\notes.txt in C:\Work\Shop".to_string());
    let env = Env { first_task_of: &home, redact: &real_redact, label_of: &label };
    let d = draft(&tree(), "p1", &env).unwrap();
    assert!(!d.crew.roles[0].task.contains("you-pc"), "{}", d.crew.roles[0].task);
    assert!(d.crew.roles[0].task.ends_with("in {folder}"));
}

#[test]
fn a_pane_without_a_run_gets_an_empty_task_the_person_must_fill() {
    let env = Env { first_task_of: &first_task, redact: &real_redact, label_of: &label };
    let d = draft(&[pane("lonely", "alpha", None, None)], "lonely", &env).unwrap();
    assert_eq!(d.crew.roles[0].task, "");
    assert!(validate(&d.crew, &state).iter().any(|e| e.code == "CREW_TASK"));
}

#[test]
fn a_draft_keeps_eight_roles_and_counts_the_rest() {
    let mut panes = vec![pane("root", "alpha", None, Some("Lead"))];
    for n in 0..11 {
        panes.push(pane(&format!("h{n}"), "beta", Some("root"), None));
    }
    let env = Env { first_task_of: &|_| Some("x".into()), redact: &real_redact, label_of: &label };
    let d = draft(&panes, "root", &env).unwrap();
    assert_eq!(d.crew.roles.len(), MAX_ROLES);
    assert_eq!(d.omitted, 12 - MAX_ROLES);
    assert!(validate(&d.crew, &state).is_empty());
}

#[test]
fn a_draft_of_a_closed_pane_says_so() {
    let env = Env { first_task_of: &first_task, redact: &real_redact, label_of: &label };
    assert_eq!(draft(&tree(), "gone", &env).unwrap_err().code, "CREW_PANE_GONE");
}

// ------------------------------------------------------------ starting

#[derive(Default)]
struct Fake {
    steps: Vec<Step>,
    fail_on: Option<&'static str>,
}

impl Starter for Fake {
    fn start(&mut self, step: &Step) -> Result<String, StartFailure> {
        if Some(step.role.name.as_str()) == self.fail_on {
            return Err(StartFailure { code: "AGENT_NOT_INSTALLED".into(), message: "not installed".into() });
        }
        self.steps.push(step.clone());
        Ok(format!("pane-{}", step.role.name))
    }
}

fn three_levels() -> Crew {
    // Declared child-first on purpose: the order comes from the tree, not from the file.
    crew(vec![role("Grandchild", "beta", Some("Child")), role("Child", "beta", Some("Lead")), role("Lead", "alpha", None), role("Sibling", "alpha", Some("Lead"))])
}

#[test]
fn the_root_starts_first_and_each_child_under_the_pane_of_its_parent() {
    let c = three_levels();
    assert!(validate(&c, &state).is_empty());
    let mut fake = Fake::default();
    let out = run(&c, "C:\\Work\\Shop", &mut fake);
    let names: Vec<&str> = fake.steps.iter().map(|s| s.role.name.as_str()).collect();
    assert_eq!(names, vec!["Lead", "Child", "Sibling", "Grandchild"]);
    let parents: Vec<Option<&str>> = fake.steps.iter().map(|s| s.parent_pane.as_deref()).collect();
    assert_eq!(parents, vec![None, Some("pane-Lead"), Some("pane-Lead"), Some("pane-Child")]);
    assert!(out.failed.is_none() && out.not_started.is_empty());
    assert_eq!(out.started.len(), 4);
    assert_eq!(out.started[0].pane_id, "pane-Lead");
    assert_eq!(start_order(&c), vec![2, 1, 3, 0]);
}

#[test]
fn the_folder_is_filled_into_every_task() {
    let mut fake = Fake::default();
    run(&three_levels(), "D:\\Other", &mut fake);
    assert!(fake.steps.iter().all(|s| s.folder == "D:\\Other" && s.task.contains("in D:\\Other") && !s.task.contains("{folder}")));
}

#[test]
fn the_first_failure_stops_the_crew_and_names_the_role() {
    let mut fake = Fake { fail_on: Some("Child"), ..Fake::default() };
    let out = run(&three_levels(), "D:\\Other", &mut fake);
    assert_eq!(out.started.iter().map(|s| s.role.as_str()).collect::<Vec<_>>(), vec!["Lead"]);
    let failed = out.failed.unwrap();
    assert_eq!((failed.role.as_str(), failed.code.as_str()), ("Child", "AGENT_NOT_INSTALLED"));
    assert_eq!(out.not_started, vec!["Sibling", "Grandchild"]);
    assert_eq!(fake.steps.len(), 1);
}

#[test]
fn a_failing_root_starts_nothing_else() {
    let mut fake = Fake { fail_on: Some("Lead"), ..Fake::default() };
    let out = run(&three_levels(), "D:\\Other", &mut fake);
    assert!(out.started.is_empty());
    assert_eq!(out.not_started.len(), 3);
}

// ------------------------------------------------------------ the folder of files

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    std::env::temp_dir().join(format!("pm-crews-{tag}-{}-{nanos}", std::process::id()))
}

#[test]
fn saving_listing_loading_and_removing_a_crew() {
    let dir = temp_dir("store");
    let c = parse(FILE).unwrap();
    let slug = store::save(&dir, &c, &state).unwrap();
    assert_eq!(slug, "review-team");
    // Same name again: the same file is replaced.
    assert_eq!(store::save(&dir, &c, &state).unwrap(), "review-team");
    // Another crew whose name gives the same slug does not overwrite it.
    let mut other = c.clone();
    other.name = "Review team!".into();
    assert_eq!(store::save(&dir, &other, &state).unwrap(), "review-team-2");
    let listed = store::list(&dir, &state);
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].roles, 2);
    assert_eq!(listed[0].agents, vec!["alpha", "wizard_made"]);
    assert!(listed.iter().all(|s| s.problem.is_none()));
    assert_eq!(store::load(&dir, "review-team").unwrap(), c);
    store::remove(&dir, "review-team-2").unwrap();
    assert_eq!(store::list(&dir, &state).len(), 1);
}

#[test]
fn an_invalid_crew_is_not_written_and_a_gone_agent_flags_the_row() {
    let dir = temp_dir("invalid");
    let bad = crew(vec![role("Lead", "ghost", None)]);
    let errors = store::save(&dir, &bad, &state).unwrap_err();
    assert_eq!(errors[0].code, "CREW_AGENT_UNKNOWN");
    assert!(store::list(&dir, &state).is_empty());
    let slug = store::save(&dir, &parse(FILE).unwrap(), &state).unwrap();
    let after_removal = |id: &str| if id == "alpha" { AgentState::Unknown } else { AgentState::Ready };
    let listed = store::list(&dir, &after_removal);
    assert_eq!(listed[0].slug, slug);
    assert!(listed[0].problem.as_deref().unwrap().contains("alpha"));
}

#[test]
fn only_slugs_of_ours_name_a_file() {
    let dir = temp_dir("slug");
    for bad in ["../x", "a/b", "a\\b", "", "UPPER", "x.toml", "C:"] {
        assert_eq!(store::load(&dir, bad).unwrap_err().code, "CREW_NOT_FOUND", "{bad:?}");
        assert!(store::remove(&dir, bad).is_err());
    }
    assert_eq!(store::slug_of("  Caf\u{e9} ## Team!! "), "caf-team");
    assert_eq!(store::slug_of("???"), "crew");
}

#[test]
fn a_file_that_cannot_be_read_is_listed_with_its_problem() {
    let dir = temp_dir("broken");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("oops.toml"), "name = ").unwrap();
    let listed = store::list(&dir, &state);
    assert_eq!(listed.len(), 1);
    assert!(listed[0].problem.is_some());
}

#[test]
fn a_crew_start_key_asks_the_dispatcher_for_a_pane_of_its_own() {
    assert!(super::wants_new_pane(&super::start_key("1f", 2)));
    assert_ne!(super::start_key("1f", 1), super::start_key("1f", 2));
    assert!(!super::wants_new_pane("mission-1f-2"));
    assert!(!super::wants_new_pane(""));
}
