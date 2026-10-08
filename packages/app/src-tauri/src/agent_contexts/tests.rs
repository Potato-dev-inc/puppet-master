use super::*;

#[test]
fn lists_all_agent_profiles() {
    let profiles = list_agent_context_profiles();
    assert_eq!(profiles.len(), 9);
    assert!(profiles.iter().any(|profile| profile.agent_type == "codex"));
    assert!(profiles.iter().all(|profile| !profile.best_for.is_empty()));
}

/// The list is the registry snapshot (registry order), not a hand-kept array.
#[test]
fn list_follows_registry_order() {
    let _serial = crate::tui::registry::test_serial();
    let listed: Vec<String> = list_agent_context_profiles()
        .iter()
        .map(|profile| profile.agent_type.as_str().to_string())
        .collect();
    let registered: Vec<String> = crate::tui::registry::snapshot()
        .iter()
        .filter(|profile| profile.context().is_some())
        .map(|profile| profile.id().to_string())
        .collect();
    assert_eq!(listed, registered);
    assert_eq!(listed, CODE_IDS);
}

#[test]
fn detects_model_from_buffer_text() {
    assert_eq!(
        detect_model_from_buffer("current model: claude-sonnet-4-6").as_deref(),
        Some("claude-sonnet-4-6")
    );
    assert_eq!(
        detect_model_from_buffer("using GPT-5.1 for this session").as_deref(),
        Some("GPT-5.1")
    );
}

#[test]
fn detects_cursor_model_from_footer() {
    let inspected = inspect_agent_model("p1", AgentType::CursorAgent, "hi\nComposer 2.5 · 7.2%");
    assert_eq!(inspected.detected_model.as_deref(), Some("Composer 2.5"));
}

#[test]
fn builds_complete_pane_agent_context() {
    let pane = PaneInfo {
        id: "pane-1".to_string(),
        agent_type: "codex".to_string(),
        pid: 42,
        status: "idle".to_string(),
        created_at: 1,
        last_output_at: Some(2),
        cwd: "/tmp".to_string(),
        cols: 120,
        rows: 30,
    };
    let context = build_pane_agent_context(None, pane, "line 1\nmodel: o3\nline 3").unwrap();
    assert_eq!(context.context.agent_type, "codex");
    assert_eq!(context.model.detected_model.as_deref(), Some("o3"));
    assert_eq!(context.model.source, "buffer");
    assert!(context.recent_buffer_preview.contains("line 3"));
}

/// The nine code profiles, in registry order.
const CODE_IDS: [&str; 9] = [
    "claude",
    "codex",
    "opencode",
    "opencode_native",
    "cursor_agent",
    "cursor",
    "cmd",
    "powershell",
    "bash",
];

/// Exact JSON of the nine code context profiles as of T09 (before the move to owned strings),
/// with one deliberate fix: `AgentType`'s `rename_all = "lowercase"` serialised OpenCode (API) as
/// `"opencodenative"`; the id string is `"opencode_native"` everywhere else (and is what callers
/// look contexts up by), so the owned id serialises that.
const NINE_PROFILES_JSON: &str = r#"[
  {
    "agent_type": "claude",
    "label": "Claude Code",
    "default_model": null,
    "model_detection": "cli-banner",
    "smartness": 9,
    "strengths": [
      "codebase-reasoning",
      "implementation",
      "review",
      "debugging"
    ],
    "context_notes": [
      "Strong default for broad repository understanding, multi-file edits, and code review.",
      "Usually exposes its active model in the TUI banner or startup text when configured by the CLI."
    ],
    "best_for": [
      "planning complex changes",
      "reviewing diffs",
      "large refactors",
      "implementation with tests"
    ],
    "planned_sidebar_actions": [
      "delegate task",
      "ask for review",
      "compare plan against Codex",
      "summarize current pane"
    ]
  },
  {
    "agent_type": "codex",
    "label": "Codex CLI",
    "default_model": null,
    "model_detection": "cli-banner",
    "smartness": 9,
    "strengths": [
      "implementation",
      "debugging",
      "terminal-ops",
      "codebase-reasoning"
    ],
    "context_notes": [
      "Good default for surgical coding, build fixes, and terminal-native verification loops.",
      "Model may be supplied by the Codex config or surfaced in the TUI; inspect terminal buffer for the best available signal."
    ],
    "best_for": [
      "coding fixes",
      "running tests",
      "debugging build failures",
      "iterative verification"
    ],
    "planned_sidebar_actions": [
      "delegate implementation",
      "run verification loop",
      "ask for status",
      "handoff focused bug"
    ]
  },
  {
    "agent_type": "opencode",
    "label": "OpenCode",
    "default_model": null,
    "model_detection": "configuration",
    "smartness": 7,
    "strengths": [
      "implementation",
      "terminal-ops",
      "debugging"
    ],
    "context_notes": [
      "Useful as an additional coding terminal when you want parallel exploration.",
      "Model routing is provider-config dependent, so treat detection as advisory unless the buffer shows an explicit model."
    ],
    "best_for": [
      "parallel edits",
      "alternative implementation passes",
      "lighter bug fixes"
    ],
    "planned_sidebar_actions": [
      "delegate parallel attempt",
      "ask for alternative",
      "compare output"
    ]
  },
  {
    "agent_type": "opencode_native",
    "label": "OpenCode (API)",
    "default_model": null,
    "model_detection": "configuration",
    "smartness": 8,
    "strengths": [
      "implementation",
      "terminal-ops",
      "debugging"
    ],
    "context_notes": [
      "Runs opencode serve locally; orchestrator input uses prompt_async instead of PTY typing.",
      "The attach TUI is for visibility; bridge/MCP writes target the API session."
    ],
    "best_for": [
      "orchestrated workers",
      "API-driven prompts",
      "parallel OpenCode sessions"
    ],
    "planned_sidebar_actions": [
      "delegate via API",
      "inspect session health",
      "reply to permission prompts"
    ]
  },
  {
    "agent_type": "cursor_agent",
    "label": "Cursor Agent CLI",
    "default_model": null,
    "model_detection": "cli-banner",
    "smartness": 8,
    "strengths": [
      "codebase-reasoning",
      "implementation",
      "review"
    ],
    "context_notes": [
      "Headless Cursor Agent CLI. Read-only delegated tasks use its documented plan mode."
    ],
    "best_for": [
      "headless code tasks",
      "structured result reporting"
    ],
    "planned_sidebar_actions": [
      "delegate task",
      "inspect result",
      "review changes"
    ]
  },
  {
    "agent_type": "cursor",
    "label": "Cursor IDE",
    "default_model": null,
    "model_detection": "unknown",
    "smartness": 6,
    "strengths": [
      "ui-orchestration",
      "implementation"
    ],
    "context_notes": [
      "IDE launcher rather than a terminal TUI. Treat model information as unavailable unless a future Cursor bridge reports it."
    ],
    "best_for": [
      "opening the workspace visually",
      "manual user-guided edits"
    ],
    "planned_sidebar_actions": [
      "open project",
      "focus editor",
      "handoff manual review"
    ]
  },
  {
    "agent_type": "cmd",
    "label": "Command Prompt",
    "default_model": null,
    "model_detection": "unknown",
    "smartness": 1,
    "strengths": [
      "terminal-ops"
    ],
    "context_notes": [
      "Plain cmd.exe-style shell pane. It has no model; use it for deterministic Windows command workflows."
    ],
    "best_for": [
      "Windows command prompt workflows",
      "batch commands",
      "environment inspection"
    ],
    "planned_sidebar_actions": [
      "run command",
      "capture output",
      "prepare environment"
    ]
  },
  {
    "agent_type": "powershell",
    "label": "PowerShell",
    "default_model": null,
    "model_detection": "unknown",
    "smartness": 1,
    "strengths": [
      "terminal-ops"
    ],
    "context_notes": [
      "Plain shell pane. It has no model; use it for deterministic commands and scripts."
    ],
    "best_for": [
      "build commands",
      "file inspection",
      "manual scripts"
    ],
    "planned_sidebar_actions": [
      "run command",
      "capture output",
      "prepare environment"
    ]
  },
  {
    "agent_type": "bash",
    "label": "Bash",
    "default_model": null,
    "model_detection": "unknown",
    "smartness": 1,
    "strengths": [
      "terminal-ops"
    ],
    "context_notes": [
      "Plain shell pane. It has no model; use it for POSIX-flavored commands."
    ],
    "best_for": [
      "shell commands",
      "cross-platform scripts",
      "log inspection"
    ],
    "planned_sidebar_actions": [
      "run command",
      "capture output",
      "prepare environment"
    ]
  }
]"#;

#[test]
fn json_snapshot_unchanged_for_nine_profiles() {
    let all: Vec<_> = CODE_IDS
        .iter()
        .map(|id| context_profile(id).unwrap())
        .collect();
    let actual = serde_json::to_string_pretty(&all).unwrap();
    assert_eq!(actual, NINE_PROFILES_JSON);
}
