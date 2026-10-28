use super::*;
use crate::agent_install::plan::{Action, Method};

/// A store entry `id` (its definition names one npm package), loaded the way the shipped catalog is.
fn store_entry(id: &str) -> CatalogEntry {
    let entry = format!(
        "schema_version = 1\n\n[store]\nid = \"{id}\"\ntagline = \"A test agent.\"\ntier = \"starter\"\n\
         builtin = false\ncategories = [\"general\"]\nstatus = \"active\"\nfirst_listed = \"2026-10-28\"\n\n\
         [vendor]\nname = \"Test Vendor\"\nhomepage = \"https://example.com/{id}\"\nlicense = \"Apache-2.0\"\n\
         license_kind = \"open_source\"\n\n[sign_in]\nkind = \"none\"\nsummary = \"No sign-in needed.\"\n\n\
         [checked]\ndate = \"2026-10-28\"\nversion = \"1.0.0\"\nhow = \"vendor_docs\"\nwindows_tested = false\n"
    );
    let definition = format!(
        "schema_version = 1\n\n[agent]\nid = \"{id}\"\nlabel = \"Test {id}\"\n\n[binary]\nnames = [\"{id}\"]\n\n\
         [install.windows]\nsource = \"https://example.com/{id}\"\nnpm = \"test-{id}-cli\"\n"
    );
    let files = [(format!("agents/{id}/entry.toml"), entry), (format!("agents/{id}/definition.toml"), definition)];
    let refs: Vec<(&str, &str)> = files.iter().map(|(p, t)| (p.as_str(), t.as_str())).collect();
    let loaded = catalog::load(&refs);
    assert!(loaded.problems.is_empty(), "{:?}", loaded.problems);
    loaded.entries.into_iter().next().expect("one entry")
}

fn winget_plan() -> Plan {
    Plan {
        agent_id: "t".into(),
        label: "Tool".into(),
        steps: vec![Step {
            tool: None,
            label: "Tool".into(),
            method: Method::Winget,
            action: Action::Argv { exe: "winget".into(), args: vec!["install".into()] },
            command: "winget install --id A.B -e --source winget".into(),
            source: "https://example.com/setup".into(),
            high_risk: false,
            note: String::new(),
        }],
        blocked: None,
    }
}

#[test]
fn the_shown_command_is_accepted() {
    let plan = winget_plan();
    assert!(check_request(&plan, 0, "winget install --id A.B -e --source winget", false).is_ok());
}

#[test]
fn a_different_command_is_refused_before_anything_runs() {
    let plan = winget_plan();
    let err = check_request(&plan, 0, "winget install --id Evil.Thing -e", false).unwrap_err();
    assert!(err.starts_with("INSTALL_COMMAND_CHANGED"), "{err}");
}

#[test]
fn a_missing_step_and_a_blocked_plan_are_refused() {
    let plan = winget_plan();
    assert!(check_request(&plan, 3, "x", false).unwrap_err().starts_with("INSTALL_NO_SUCH_STEP"));
    let blocked = Plan { steps: vec![], blocked: Some("no".into()), ..winget_plan() };
    assert!(check_request(&blocked, 0, "x", false).unwrap_err().starts_with("INSTALL_UNAVAILABLE"));
}

#[test]
fn a_script_step_needs_the_second_confirmation() {
    let mut plan = winget_plan();
    plan.steps[0].high_risk = true;
    plan.steps[0].method = Method::Script;
    let shown = "winget install --id A.B -e --source winget";
    assert!(check_request(&plan, 0, shown, false).unwrap_err().starts_with("INSTALL_NEEDS_CONFIRMATION"));
    assert!(check_request(&plan, 0, shown, true).is_ok());
}

#[test]
fn the_sheet_gets_the_command_its_source_and_the_risk() {
    let view = plan_view(&winget_plan(), Some("npm i -g tool"));
    assert_eq!(view["available"], true);
    assert_eq!(view["hint"], "npm i -g tool");
    let step = &view["steps"][0];
    assert_eq!(step["command"], "winget install --id A.B -e --source winget");
    assert_eq!(step["source_host"], "example.com");
    assert_eq!(step["high_risk"], false);
    assert_eq!(step["method"], "winget");
}

#[test]
fn a_blocked_plan_says_why_and_still_carries_the_hint() {
    let blocked = Plan { steps: vec![], blocked: Some("winget is missing".into()), ..winget_plan() };
    let view = plan_view(&blocked, Some("npm i -g tool"));
    assert_eq!(view["available"], false);
    assert_eq!(view["reason"], "winget is missing");
    assert_eq!(view["hint"], "npm i -g tool");
}

#[test]
fn source_host_is_the_site_only() {
    assert_eq!(source_host("https://code.example.com/docs/en/setup?x=1"), "code.example.com");
}

#[test]
fn a_pane_this_module_did_not_start_is_not_readable() {
    let registry = Mutex::new(crate::pty::PaneRegistry::default());
    let view = status(&registry, "some-other-pane");
    assert_eq!(view["known"], false);
    assert_eq!(view["done"], false);
}

// ---- the shipped profiles ------------------------------------------------------------------

#[test]
fn every_shipped_agent_install_spec_parses_and_names_its_sources() {
    for id in ["claude", "codex", "cursor_agent", "opencode"] {
        let spec = crate::tui::profile_for(id).install_spec().unwrap_or_else(|| panic!("{id} has no install spec"));
        assert!(spec.validate().is_ok(), "{id}");
        assert!(spec.source.starts_with("https://"), "{id}");
    }
}

#[test]
fn the_shipped_specs_say_what_the_research_found() {
    let spec = |id: &str| crate::tui::profile_for(id).install_spec().unwrap();
    assert_eq!(spec("claude").winget.as_deref(), Some("Anthropic.ClaudeCode"));
    assert_eq!(spec("claude").npm.as_deref(), Some("@anthropic-ai/claude-code"));
    assert_eq!(spec("codex").winget.as_deref(), Some("OpenAI.Codex"));
    assert_eq!(spec("codex").npm.as_deref(), Some("@openai/codex"));
    assert_eq!(spec("opencode").npm.as_deref(), Some("opencode-ai"));
    // Cursor Agent has only the vendor script: no package-manager method exists.
    assert_eq!(spec("cursor_agent").ordered_methods(), ["script"]);
}

#[test]
fn a_native_install_does_not_pull_in_node_or_git_for_the_big_agents() {
    // With winget present, these install in one step and need no prerequisites.
    let tools = Tools { winget: Some("winget".into()), present: Default::default() };
    for id in ["claude", "codex", "opencode"] {
        let (plan, _) = plan_for(id, &tools).unwrap();
        assert_eq!(plan.steps.len(), 1, "{id}: {:?}", plan.steps.iter().map(|s| &s.command).collect::<Vec<_>>());
        assert_eq!(plan.steps[0].method, Method::Winget, "{id}");
    }
}

#[test]
fn cursor_agent_offers_only_the_confirmed_script() {
    let tools = Tools { winget: Some("winget".into()), present: Default::default() };
    let (plan, hint) = plan_for("cursor_agent", &tools).unwrap();
    assert_eq!(plan.steps.len(), 1);
    assert!(plan.steps[0].high_risk);
    assert_eq!(plan.steps[0].command, "irm 'https://cursor.com/install?win32=true' | iex");
    assert!(hint.is_some());
}

#[test]
fn an_unknown_agent_is_an_error_not_a_guess() {
    assert!(plan_for("no_such_agent", &Tools::default()).unwrap_err().starts_with("INSTALL_UNKNOWN_AGENT"));
}

#[test]
fn an_unregistered_store_id_gets_a_plan_from_its_catalog_entry() {
    let entry = store_entry("acme");
    let spec = InstallSpec { source: "https://example.com/acme".into(), npm: Some("test-acme-cli".into()), ..Default::default() };
    let tools = Tools { winget: None, present: ["node".to_string()].into_iter().collect() };
    let plan = store_plan(&entry, Some(&spec), &tools);
    assert!(plan.blocked.is_none(), "{:?}", plan.blocked);
    assert_eq!(plan.agent_id, "acme");
    assert_eq!(plan.label, "Test acme", "the definition's name, not the vendor's");
    assert_eq!(plan.steps.len(), 1);
    assert_eq!(plan.steps[0].command, "npm install -g test-acme-cli");
    assert_eq!(plan_view(&plan, None)["available"], true);
}

#[test]
fn a_store_id_without_a_windows_spec_is_blocked_with_a_reason() {
    let entry = store_entry("acme");
    let plan = store_plan(&entry, None, &Tools::default());
    assert!(plan.steps.is_empty());
    assert!(plan.blocked.is_some());
}

#[test]
fn the_store_probe_finds_the_program_from_its_definition_and_says_when_it_is_missing() {
    let entry = store_entry("nimbus");
    let probe = store_probe(&entry, Path::new(".")).expect("the definition names a binary");
    assert_eq!(probe.id, "nimbus");
    assert_eq!(probe.label, "Test nimbus");
    assert!(!probe.installed, "no nimbus program is on PATH in a test");
    assert!(probe.version_command.is_none());
    let health = crate::agent_health::check_probe(&probe, crate::agent_health::VERSION_TIMEOUT);
    assert!(!health.installed);
    assert_eq!(health.id, "nimbus");
}

#[test]
fn the_install_code_never_matches_an_agent_name() {
    for (name, text) in [
        ("mod.rs", include_str!("mod.rs")),
        ("plan.rs", include_str!("plan.rs")),
        ("command.rs", include_str!("command.rs")),
        ("path_refresh.rs", include_str!("path_refresh.rs")),
    ] {
        let code = text.split("#[cfg(test)]").next().unwrap().to_ascii_lowercase();
        for agent in ["claude", "codex", "cursor", "opencode", "gemini", "kilo"] {
            assert!(!code.contains(agent), "{name} names an agent: {agent}");
        }
    }
}
