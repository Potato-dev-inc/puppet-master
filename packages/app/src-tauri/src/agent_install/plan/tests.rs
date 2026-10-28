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

/// A Python agent with a uv package, a Python version and a vendor script as the last resort.
const UV_AND_SCRIPT: &str = r#"
[install.windows]
source = "https://aider.chat/docs/install.html"
uv = "aider-chat"
python = "3.12"
script_url = "https://aider.chat/install.ps1"
"#;

#[test]
fn uv_step_shows_the_exact_command_with_the_python_version() {
    let plan = plan_install("t", "Aider", Some(&spec(UV_AND_SCRIPT)), &tools(false, &["uv"]));
    assert_eq!(commands(&plan), ["uv tool install --python 3.12 aider-chat"]);
    let only = &plan.steps[0];
    assert_eq!(only.method, Method::Uv);
    assert!(!only.high_risk);
    assert_eq!(only.tool, None);
    assert_eq!(only.source, "https://aider.chat/docs/install.html");
    assert_eq!(
        only.action,
        Action::Argv {
            exe: "uv".into(),
            args: ["tool", "install", "--python", "3.12", "aider-chat"].iter().map(|s| s.to_string()).collect(),
        }
    );
}

#[test]
fn uv_step_without_a_python_version_has_no_python_flag() {
    let no_python = spec("[install.windows]\nsource=\"https://example.com/s\"\nuv=\"mistral-vibe\"\n");
    let plan = plan_install("t", "Vibe", Some(&no_python), &tools(false, &["uv"]));
    assert_eq!(commands(&plan), ["uv tool install mistral-vibe"]);
}

#[test]
fn a_uv_and_script_spec_plans_uv_and_never_the_vendor_script() {
    // The bug this guards: `uv` used to fall into the script branch, so with uv present the
    // vendor script (high risk) was planned instead of the uv command.
    let plan = plan_install("t", "Aider", Some(&spec(UV_AND_SCRIPT)), &tools(true, &["uv"]));
    assert_eq!(plan.steps.len(), 1);
    assert_eq!(plan.steps[0].method, Method::Uv);
    assert!(plan.steps.iter().all(|s| s.method != Method::Script && !s.high_risk));
}

#[test]
fn a_missing_uv_is_installed_through_winget_before_the_uv_step() {
    let plan = plan_install("t", "Aider", Some(&spec(UV_AND_SCRIPT)), &tools(true, &[]));
    assert_eq!(
        commands(&plan),
        ["winget install --id astral-sh.uv -e --source winget", "uv tool install --python 3.12 aider-chat"]
    );
    assert_eq!(plan.steps[0].tool.as_deref(), Some("uv"));
    assert_eq!(plan.steps[0].label, "uv");
    assert_eq!(plan.steps[0].method, Method::Winget);
    assert_eq!(plan.steps[1].tool, None);
    assert_eq!(plan.steps[1].method, Method::Uv);
}

#[test]
fn without_uv_or_winget_the_vendor_script_is_still_the_last_resort() {
    let plan = plan_install("t", "Aider", Some(&spec(UV_AND_SCRIPT)), &tools(false, &[]));
    assert_eq!(commands(&plan), ["irm 'https://aider.chat/install.ps1' | iex"]);
    assert!(plan.steps[0].high_risk);
}

#[test]
fn a_uv_only_spec_without_uv_or_winget_is_blocked_with_a_plain_reason() {
    let uv_only = spec("[install.windows]\nsource=\"https://example.com/s\"\nuv=\"aider-chat\"\n");
    let plan = plan_install("t", "Aider", Some(&uv_only), &tools(false, &[]));
    assert!(plan.steps.is_empty());
    assert!(plan.blocked.as_deref().unwrap().contains("winget is missing"), "{:?}", plan.blocked);
}

#[test]
fn missing_tools_knows_uv() {
    let uv_spec = spec(UV_AND_SCRIPT);
    assert_eq!(missing_tools(&uv_spec, "uv", &tools(true, &[])), ["uv"]);
    assert!(missing_tools(&uv_spec, "uv", &tools(true, &["uv"])).is_empty());
    // Other methods do not ask for uv.
    assert!(missing_tools(&uv_spec, "script", &tools(true, &[])).is_empty());
}

#[test]
fn an_unknown_method_gets_no_step_instead_of_the_script() {
    assert!(step("T", None, &spec(UV_AND_SCRIPT), "pip", &tools(true, &["uv"])).is_none());
    assert!(step("T", None, &spec(UV_AND_SCRIPT), "choco", &tools(true, &[])).is_none());
}

#[test]
fn a_uv_step_refuses_a_python_version_that_was_not_checked() {
    // Built by hand: the spec validator would already refuse this value; the plan must too.
    let bad = InstallSpec {
        source: "https://example.com/s".into(),
        uv: Some("aider-chat".into()),
        python: Some("3.12; calc".into()),
        ..Default::default()
    };
    assert!(step("T", None, &bad, "uv", &tools(false, &["uv"])).is_none());
}

#[test]
fn uninstall_through_winget_uses_the_recorded_package() {
    let plan = plan_uninstall("t", "Tool", Some(&spec(FULL)), "winget", "Vendor.Tool", &tools(true, &[]));
    assert_eq!(plan.blocked, None);
    assert_eq!(commands(&plan), ["winget uninstall --id Vendor.Tool -e --source winget"]);
    assert_eq!(plan.steps[0].method, Method::Winget);
    assert_eq!(plan.steps[0].tool, None);
    assert!(!plan.steps[0].high_risk);
    assert_eq!(plan.steps[0].source, "https://example.com/setup");
}

#[test]
fn uninstall_through_npm_is_one_step_even_when_node_is_missing() {
    // Node.js is never a prerequisite of a removal, and never removed itself.
    let plan = plan_uninstall("t", "Tool", Some(&spec(FULL)), "npm", "@vendor/tool", &tools(false, &[]));
    assert_eq!(commands(&plan), ["npm uninstall -g @vendor/tool"]);
    assert_eq!(plan.steps[0].method, Method::Npm);
}

#[test]
fn uninstall_through_uv_matches_the_install_command() {
    let plan = plan_uninstall("t", "Aider", Some(&spec(UV_AND_SCRIPT)), "uv", "aider-chat", &tools(false, &[]));
    assert_eq!(commands(&plan), ["uv tool uninstall aider-chat"]);
    assert_eq!(plan.steps[0].method, Method::Uv);
    assert_eq!(plan.steps.len(), 1, "uv is not removed with the agent");
}

#[test]
fn uninstall_is_blocked_when_the_package_differs_from_the_record() {
    let plan = plan_uninstall("t", "Tool", Some(&spec(FULL)), "npm", "@other/tool", &tools(true, &["node"]));
    assert!(plan.steps.is_empty());
    assert!(plan.blocked.as_deref().unwrap().contains("installed another way"), "{:?}", plan.blocked);
}

#[test]
fn uninstall_is_blocked_when_the_spec_no_longer_names_the_recorded_method() {
    // The record says uv, but the current spec has no uv package.
    let plan = plan_uninstall("t", "Tool", Some(&spec(FULL)), "uv", "aider-chat", &tools(true, &[]));
    assert!(plan.steps.is_empty());
    assert!(plan.blocked.as_deref().unwrap().contains("installed another way"));
}

#[test]
fn uninstall_never_handles_a_script_install_or_an_unknown_method() {
    for method in ["script", "pip", ""] {
        let plan = plan_uninstall("t", "Tool", Some(&spec(FULL)), method, "x", &tools(true, &[]));
        assert!(plan.steps.is_empty(), "{method}");
        assert!(plan.blocked.is_some(), "{method}");
    }
}

#[test]
fn uninstall_through_winget_needs_winget_on_the_pc() {
    let plan = plan_uninstall("t", "Tool", Some(&spec(FULL)), "winget", "Vendor.Tool", &tools(false, &[]));
    assert!(plan.steps.is_empty());
    assert!(plan.blocked.as_deref().unwrap().contains("winget is missing"), "{:?}", plan.blocked);
}

#[test]
fn uninstall_without_a_current_spec_is_blocked() {
    let plan = plan_uninstall("t", "Tool", None, "npm", "@vendor/tool", &tools(true, &["node"]));
    assert!(plan.steps.is_empty());
    assert!(plan.blocked.is_some());
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
    assert_eq!(defs.keys().map(String::as_str).collect::<Vec<_>>(), ["git", "node", "uv"]);
    for (id, def) in &defs {
        let spec = def.install.windows.as_ref().unwrap_or_else(|| panic!("{id}"));
        assert!(spec.validate().is_ok(), "{id}");
        assert!(spec.winget.is_some(), "{id} installs through winget");
    }
}
