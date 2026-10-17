//! Listed-surface tests: aliases and advanced tools are hidden from `tools/list` but stay callable.

use super::*;

/// The catalog version before the listed surface existed. It must have moved.
const CATALOG_VERSION_BEFORE_SURFACE: &str = "68-e4687e50f1687299";

fn names_for(mode: McpMode) -> Vec<&'static str> {
    tools_for_mode(mode)
        .into_iter()
        .map(|tool| tool.name)
        .collect()
}

#[test]
fn catalog_version_moved_when_the_listed_surface_was_introduced() {
    let version = catalog_version();
    assert_ne!(version, CATALOG_VERSION_BEFORE_SURFACE);
    // 68 tools before the Team tools were added (team_roster, team_events, team_wait, team_notify): 72 now.
    assert!(version.starts_with("72-"), "{version}");
    assert_eq!(external_mcp_tool_count(), 72);
}

#[test]
fn catalog_version_covers_the_listing_tier() {
    // The tier code is part of the hash, so moving a tool between tiers moves the version.
    use super::surface::tier_code;
    assert_eq!(tier_code("run_agent"), 0);
    assert_eq!(tier_code("create_task"), 1);
    assert_eq!(tier_code("list_workers"), 2);
    assert_eq!(catalog_version(), catalog_version());
}

#[test]
fn aliases_are_hidden_from_tools_list_in_every_mode_but_all() {
    assert_eq!(
        ALIASES.iter().map(|(alias, _)| *alias).collect::<Vec<_>>(),
        [
            "list_workers",
            "send_agent",
            "cancel_agent",
            "wait_for_model",
            "wait_for_task",
            "wait_for_worker"
        ]
    );
    for mode in [McpMode::Agent, McpMode::Shell, McpMode::Both] {
        let listed = names_for(mode);
        for (alias, _) in ALIASES {
            assert!(!listed.contains(alias), "{alias} is listed in {mode:?}");
        }
    }
    let all = names_for(McpMode::All);
    for (alias, canonical) in ALIASES {
        assert!(all.contains(alias), "all mode must still list {alias}");
        assert!(
            all.contains(canonical),
            "{alias} points at unlisted {canonical}"
        );
    }
}

#[test]
fn aliases_stay_callable_exactly_where_they_were_before() {
    // Callable sets are untouched: agent mode keeps its three wrappers, shell keeps two waits,
    // and `both` / `all` can call every alias.
    for alias in ["list_workers", "send_agent", "cancel_agent"] {
        assert!(tool_visible_in_mode(alias, McpMode::Agent), "{alias} agent");
        assert!(!tool_visible_in_mode(alias, McpMode::Shell), "{alias} shell");
    }
    for alias in ["wait_for_model", "wait_for_worker"] {
        assert!(tool_visible_in_mode(alias, McpMode::Shell), "{alias} shell");
        assert!(!tool_visible_in_mode(alias, McpMode::Agent), "{alias} agent");
    }
    assert!(!tool_visible_in_mode("wait_for_task", McpMode::Agent));
    assert!(!tool_visible_in_mode("wait_for_task", McpMode::Shell));
    for (alias, _) in ALIASES {
        assert!(tool_visible_in_mode(alias, McpMode::Both), "{alias} both");
        assert!(tool_visible_in_mode(alias, McpMode::All), "{alias} all");
        assert!(
            callable_tools_for_mode(McpMode::Both)
                .iter()
                .any(|tool| tool.name == *alias),
            "{alias} must be in the callable set"
        );
    }
}

#[test]
fn every_alias_really_duplicates_its_canonical_tool() {
    let registry = tools();
    let find = |name: &str| registry.iter().find(|tool| tool.name == name).unwrap();
    // Same route and method: the bridge cannot tell the two apart.
    for (alias, canonical) in [
        ("list_workers", "list_agents"),
        ("send_agent", "followup_task"),
        ("cancel_agent", "interrupt_agent"),
    ] {
        let (a, c) = (find(alias), find(canonical));
        assert_eq!((a.method, a.path), (c.method, c.path), "{alias} vs {canonical}");
        assert_eq!(a.safety, c.safety, "{alias} vs {canonical}");
    }
    // send_agent is NOT a sibling of send_message: it starts a new turn like followup_task.
    assert_ne!(find("send_agent").path, find("send_message").path);
    assert!(find("send_agent").description.contains("followup_task"));
    // The wait aliases are presets of wait_for_panes on their own sub-routes.
    for alias in ["wait_for_model", "wait_for_task", "wait_for_worker"] {
        assert!(find(alias).path.starts_with("/panes/wait/"), "{alias}");
        assert!(
            find(alias).description.to_lowercase().contains("alias of wait_for_panes"),
            "{alias}"
        );
    }
    assert_eq!(alias_of("send_agent"), Some("followup_task"));
    assert_eq!(alias_of("run_agent"), None);
}

#[test]
fn listed_tool_count_per_mode_is_pinned() {
    assert_eq!(names_for(McpMode::Agent).len(), 22, "{:?}", names_for(McpMode::Agent));
    assert_eq!(names_for(McpMode::Shell).len(), 16, "{:?}", names_for(McpMode::Shell));
    assert_eq!(names_for(McpMode::Both).len(), 32, "{:?}", names_for(McpMode::Both));
    // The old full list, behind mode `all`.
    assert_eq!(names_for(McpMode::All).len(), 72);
    assert_eq!(names_for(McpMode::All).len(), external_mcp_tool_count());
}

#[test]
fn default_surfaces_stay_small_and_inside_the_callable_set() {
    assert!(names_for(McpMode::Agent).len() <= 40);
    assert!(names_for(McpMode::Both).len() <= 40);
    for mode in [McpMode::Agent, McpMode::Shell, McpMode::Both, McpMode::All] {
        let callable: Vec<_> = callable_tools_for_mode(mode)
            .into_iter()
            .map(|tool| tool.name)
            .collect();
        for name in names_for(mode) {
            assert!(callable.contains(&name), "{name} is listed but not callable in {mode:?}");
        }
    }
}

#[test]
fn unlisted_advanced_tools_are_still_callable_where_they_were() {
    for name in [
        "reply_opencode_question",
        "reply_opencode_permission",
        "read_opencode_messages",
        "read_opencode_worker_status",
        "session_identity",
        "attach_agents",
        "release_lease",
        "transfer_agent",
    ] {
        assert_eq!(tool_tier(name), ToolTier::Advanced, "{name}");
        assert!(
            tool_visible_in_mode(name, McpMode::Agent),
            "{name} must stay callable in agent mode"
        );
        assert!(!names_for(McpMode::Agent).contains(&name), "{name} should be unlisted");
        assert!(names_for(McpMode::All).contains(&name), "{name} is in the full list");
    }
    // The task board and friends were only ever callable in `both`; still so, still unlisted.
    for name in ["create_task", "claim_task", "acquire_resource_lock", "set_pane_role"] {
        assert!(tool_visible_in_mode(name, McpMode::Both), "{name}");
        assert!(!names_for(McpMode::Both).contains(&name), "{name}");
    }
}

#[test]
fn nothing_became_callable_or_uncallable_in_agent_and_shell_modes() {
    // The callable sets of agent and shell are the pre-change lists (33 and 18 names, with
    // set_mode counted in agent mode only through its always-visible rule), plus the four Team
    // tools in agent mode only.
    assert_eq!(callable_tools_for_mode(McpMode::Agent).len(), 37);
    assert_eq!(callable_tools_for_mode(McpMode::Shell).len(), 19);
    assert_eq!(callable_tools_for_mode(McpMode::Both).len(), 72);
    assert_eq!(callable_tools_for_mode(McpMode::All).len(), 72);
}

#[test]
fn the_five_tool_happy_path_is_listed_in_agent_mode() {
    let listed = names_for(McpMode::Agent);
    for name in ["quick_ask", "run_agent", "wait_agents", "followup_task", "list_agents", "set_mode"] {
        assert!(listed.contains(&name), "{name}");
        assert_eq!(tool_tier(name), ToolTier::Core);
    }
}

#[test]
fn set_mode_accepts_all_and_it_is_never_hidden() {
    assert_eq!(McpMode::parse("all"), Some(McpMode::All));
    assert_eq!(McpMode::All.as_str(), "all");
    let set_mode = tools()
        .into_iter()
        .find(|tool| tool.name == "set_mode")
        .unwrap();
    let modes = set_mode.input_schema["properties"]["mode"]["enum"]
        .as_array()
        .unwrap();
    assert!(modes.iter().any(|v| v == "all"));
    for mode in [McpMode::Agent, McpMode::Shell, McpMode::Both, McpMode::All] {
        assert!(names_for(mode).contains(&"set_mode"));
    }
}

#[test]
fn instructions_lead_with_the_five_tool_happy_path_in_order() {
    let text = mcp_instructions();
    let positions: Vec<usize> = [
        "quick_ask(",
        "run_agent(",
        "wait_agents(",
        "followup_task(",
        "list_agents()",
    ]
    .iter()
    .map(|name| text.find(name).unwrap_or_else(|| panic!("{name} missing")))
    .collect();
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]), "{positions:?}");
    assert!(text.starts_with("Puppet Master MCP"));
    assert!(text.contains("Five tools"));
    assert!(text.contains("SCHEMA_DRIFT"));
    assert!(text.contains("Rebuild or reinstall the Puppet Master MCP"));
    for (alias, _) in ALIASES {
        assert!(text.contains(alias), "instructions should name the hidden alias {alias}");
    }
}

#[test]
fn the_team_tools_are_callable_by_name_in_agent_mode_but_not_listed_by_default() {
    for name in ["team_roster", "team_events", "team_wait", "team_notify"] {
        assert_eq!(tool_tier(name), ToolTier::Advanced, "{name}");
        assert!(tool_visible_in_mode(name, McpMode::Agent), "{name} agent");
        assert!(tool_visible_in_mode(name, McpMode::Both), "{name} both");
        assert!(tool_visible_in_mode(name, McpMode::All), "{name} all");
        assert!(!tool_visible_in_mode(name, McpMode::Shell), "{name} shell: a terminal-control session has no business with the team");
        assert!(!names_for(McpMode::Agent).contains(&name), "{name} listed in agent mode");
        assert!(!names_for(McpMode::Both).contains(&name), "{name} listed in both");
        assert!(names_for(McpMode::All).contains(&name), "{name} missing from all");
    }
    // The instructions say they exist, so an orchestrator on a team can find them.
    for name in ["team_roster", "team_events", "team_wait", "team_notify"] {
        assert!(mcp_instructions().contains(name), "{name}");
    }
    assert!(mcp_instructions().contains("untrusted data"));
}
