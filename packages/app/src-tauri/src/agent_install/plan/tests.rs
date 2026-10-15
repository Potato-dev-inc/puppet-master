use super::*;
use crate::tui::install::parse_install_toml;

fn spec(text: &str) -> InstallSpec {
    parse_install_toml(text).unwrap().unwrap()
}

const FULL: &str = r#"
[install.windows]
source = "https://example.com/setup"
order = ["winget", "npm", "script"]
winget = "Vendor.Tool"
npm = "@vendor/tool"
script_url = "https://example.com/install.ps1"
"#;

fn tools(winget: bool, present: &[&str]) -> Tools {
    Tools {
        winget: winget.then(|| "winget".to_string()),
        present: present.iter().map(|s| s.to_string()).collect(),
    }
}

fn commands(plan: &Plan) -> Vec<&str> {
    plan.steps.iter().map(|s| s.command.as_str()).collect()
}

#[test]
fn winget_is_preferred_and_needs_nothing_else() {
    let plan = plan_install("t", "Tool", Some(&spec(FULL)), &tools(true, &[]));
    assert_eq!(commands(&plan), ["winget install --id Vendor.Tool -e --source winget"]);
    assert_eq!(plan.steps[0].method, Method::Winget);
    assert!(!plan.steps[0].high_risk);
    assert_eq!(plan.steps[0].source, "https://example.com/setup");
}

#[test]
fn node_is_not_offered_when_winget_can_install_the_agent_directly() {
    let plan = plan_install("t", "Tool", Some(&spec(FULL)), &tools(true, &[]));
    assert_eq!(plan.steps.len(), 1, "no Node.js step when the chosen method does not need it");
}

#[test]
fn without_winget_npm_is_used_when_node_is_there() {
    let plan = plan_install("t", "Tool", Some(&spec(FULL)), &tools(false, &["node"]));
    assert_eq!(commands(&plan), ["npm install -g @vendor/tool"]);
}

#[test]
fn without_winget_or_node_the_vendor_script_is_the_last_resort_and_is_high_risk() {
    let plan = plan_install("t", "Tool", Some(&spec(FULL)), &tools(false, &[]));
    assert_eq!(commands(&plan), ["irm 'https://example.com/install.ps1' | iex"]);
    assert!(plan.steps[0].high_risk);
    assert_eq!(plan.steps[0].method, Method::Script);
}

#[test]
fn npm_only_agent_gets_a_node_step_first_when_winget_exists() {
    let npm_only = spec("[install.windows]\nsource=\"https://example.com/s\"\nnpm=\"@vendor/tool\"\n");
    let plan = plan_install("t", "Tool", Some(&npm_only), &tools(true, &[]));
    assert_eq!(
        commands(&plan),
        ["winget install --id OpenJS.NodeJS.LTS -e --source winget", "npm install -g @vendor/tool"]
    );
    assert_eq!(plan.steps[0].tool.as_deref(), Some("node"));
    assert_eq!(plan.steps[0].label, "Node.js");
    assert_eq!(plan.steps[1].tool, None);
}

#[test]
fn npm_only_agent_with_node_present_has_a_single_step() {
    let npm_only = spec("[install.windows]\nsource=\"https://example.com/s\"\nnpm=\"@vendor/tool\"\n");
    let plan = plan_install("t", "Tool", Some(&npm_only), &tools(true, &["node"]));
    assert_eq!(commands(&plan), ["npm install -g @vendor/tool"]);
}

#[test]
fn node_beats_the_script_when_only_a_prerequisite_is_missing() {
    let both = spec(
        "[install.windows]\nsource=\"https://example.com/s\"\norder=[\"npm\",\"script\"]\nnpm=\"pkg\"\nscript_url=\"https://example.com/i.ps1\"\n",
    );
    let plan = plan_install("t", "Tool", Some(&both), &tools(true, &[]));
    assert_eq!(plan.steps.len(), 2);
    assert!(plan.steps.iter().all(|s| !s.high_risk));
}

#[test]
fn a_declared_requirement_adds_its_step() {
    let needs_git =
        spec("[install.windows]\nsource=\"https://example.com/s\"\nwinget=\"Vendor.Tool\"\nrequires=[\"git\"]\n");
    let plan = plan_install("t", "Tool", Some(&needs_git), &tools(true, &[]));
    assert_eq!(
        commands(&plan),
        ["winget install --id Git.Git -e --source winget", "winget install --id Vendor.Tool -e --source winget"]
    );
    let has_git = plan_install("t", "Tool", Some(&needs_git), &tools(true, &["git"]));
    assert_eq!(has_git.steps.len(), 1);
}

#[test]
fn npm_only_agent_without_winget_or_node_is_blocked_with_a_plain_reason() {
    let npm_only = spec("[install.windows]\nsource=\"https://example.com/s\"\nnpm=\"pkg\"\n");
    let plan = plan_install("t", "Tool", Some(&npm_only), &tools(false, &[]));
    assert!(plan.steps.is_empty());
    assert!(plan.blocked.as_deref().unwrap().contains("winget is missing"), "{:?}", plan.blocked);
}

#[test]
fn no_spec_means_no_steps() {
    let plan = plan_install("t", "Tool", None, &tools(true, &["node"]));
    assert!(plan.steps.is_empty());
    assert!(plan.blocked.is_some());
}

#[test]
fn a_winget_found_only_as_an_app_alias_runs_by_full_path_but_shows_its_name() {
    let t = Tools {
        winget: Some(r"C:\Users\a\AppData\Local\Microsoft\WindowsApps\winget.exe".into()),
        present: BTreeSet::new(),
    };
    let plan = plan_install("t", "Tool", Some(&spec(FULL)), &t);
    match &plan.steps[0].action {
        Action::Argv { exe, .. } => assert!(exe.ends_with("winget.exe")),
        other => panic!("{other:?}"),
    }
    assert!(plan.steps[0].command.starts_with("winget install"), "{}", plan.steps[0].command);
}

#[test]
fn argv_never_holds_shell_text() {
    let plan = plan_install("t", "Tool", Some(&spec(FULL)), &tools(true, &[]));
    let Action::Argv { exe, args } = &plan.steps[0].action else { panic!() };
    assert_eq!(exe, "winget");
    for token in args {
        assert!(!token.contains([' ', ';', '|', '&', '$', '`', '"', '\'']), "{token}");
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pm-install-plan-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn detect_finds_tools_on_a_given_path() {
    let bin = scratch("bin");
    for f in ["winget.exe", "node.exe", "npm.cmd"] {
        std::fs::write(bin.join(f), "").unwrap();
    }
    let path = std::env::join_paths([&bin]).unwrap();
    let t = Tools::detect_in(&path, None);
    assert_eq!(t.winget.as_deref(), Some("winget"));
    assert!(t.present.contains("node"));
    assert!(!t.present.contains("git"));
    let _ = std::fs::remove_dir_all(&bin);
}

#[test]
fn node_needs_both_node_and_npm() {
    let bin = scratch("half");
    std::fs::write(bin.join("node.exe"), "").unwrap();
    let t = Tools::detect_in(&std::env::join_paths([&bin]).unwrap(), None);
    assert!(!t.present.contains("node"));
    let _ = std::fs::remove_dir_all(&bin);
}

#[test]
fn detect_falls_back_to_the_windows_apps_alias_when_path_lacks_winget() {
    let local = scratch("local");
    let apps = local.join("Microsoft").join("WindowsApps");
    std::fs::create_dir_all(&apps).unwrap();
    std::fs::write(apps.join("winget.exe"), "").unwrap();
    let empty = scratch("empty");
    let t = Tools::detect_in(&std::env::join_paths([&empty]).unwrap(), Some(&local));
    let found = t.winget.expect("alias found");
    assert!(found.ends_with("winget.exe") && found.contains("WindowsApps"), "{found}");
    let none = Tools::detect_in(&std::env::join_paths([&empty]).unwrap(), Some(&empty));
    assert_eq!(none.winget, None);
    let _ = std::fs::remove_dir_all(&local);
    let _ = std::fs::remove_dir_all(&empty);
}

#[test]
fn the_bundled_tool_definitions_parse_with_winget_ids() {
    let defs = tool_defs();
    assert_eq!(defs.keys().map(String::as_str).collect::<Vec<_>>(), ["git", "node"]);
    for (id, def) in &defs {
        let spec = def.install.windows.as_ref().unwrap_or_else(|| panic!("{id}"));
        assert!(spec.validate().is_ok(), "{id}");
        assert!(spec.winget.is_some(), "{id} installs through winget");
    }
}
