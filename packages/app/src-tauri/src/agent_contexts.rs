use once_cell::sync::Lazy;
use regex::Regex;
use serde::Serialize;

use crate::pty::agents::AgentType;
use crate::pty::registry::{PaneInfo, PaneRegistry};
use parking_lot::Mutex;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
#[allow(dead_code)]
pub enum AgentCapability {
    CodebaseReasoning,
    Implementation,
    Review,
    Debugging,
    Research,
    TerminalOps,
    UiOrchestration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModelDetection {
    CliBanner,
    Configuration,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentContextProfile {
    pub agent_type: AgentType,
    pub label: &'static str,
    pub default_model: Option<&'static str>,
    pub model_detection: ModelDetection,
    pub smartness: u8,
    pub strengths: &'static [AgentCapability],
    pub context_notes: &'static [&'static str],
    pub best_for: &'static [&'static str],
    pub planned_sidebar_actions: &'static [&'static str],
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentModelInspection {
    pub pane_id: String,
    pub agent_type: AgentType,
    pub detected_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    pub source: &'static str,
    pub confidence: &'static str,
    pub smartness: u8,
    pub notes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_model: Option<crate::opencode::client::OpenCodeModelRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_user_model: Option<crate::opencode::client::OpenCodeModelRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub footer_model_source: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buffer_fallback: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PaneAgentContext {
    pub pane: PaneInfo,
    pub context: AgentContextProfile,
    pub model: AgentModelInspection,
    pub recent_buffer_preview: String,
}

struct ModelPattern {
    regex: Regex,
    capture: usize,
}

static MODEL_PATTERNS: Lazy<Vec<ModelPattern>> = Lazy::new(|| {
    vec![
        ModelPattern {
            regex: Regex::new(
                r"(?i)\b(gpt-5(?:\.[\w-]+)?|gpt-4(?:\.[\w-]+)?|o[134](?:-[\w-]+)?)\b",
            )
            .unwrap(),
            capture: 1,
        },
        ModelPattern {
            regex: Regex::new(r"(?i)\b(claude-(?:opus|sonnet|haiku)-[\w.-]+)\b").unwrap(),
            capture: 1,
        },
        ModelPattern {
            regex: Regex::new(r"(?i)\b((?:gemini|qwen|deepseek|llama|mistral|kimi)[\w./:-]*)\b")
                .unwrap(),
            capture: 1,
        },
        ModelPattern {
            regex: Regex::new(r"(?i)\bmodel(?:\s+is|\s*[:=])\s*([A-Za-z0-9_./:-]+)").unwrap(),
            capture: 1,
        },
    ]
});

pub fn list_agent_context_profiles() -> Vec<AgentContextProfile> {
    [
        AgentType::Claude,
        AgentType::Codex,
        AgentType::Opencode,
        AgentType::OpencodeNative,
        AgentType::Cmd,
        AgentType::Powershell,
        AgentType::Bash,
        AgentType::Cursor,
    ]
    .iter()
    .map(|agent_type| get_agent_context_profile(*agent_type))
    .collect()
}

pub fn get_agent_context_profile(agent_type: AgentType) -> AgentContextProfile {
    match agent_type {
        AgentType::Claude => AgentContextProfile {
            agent_type,
            label: "Claude Code",
            default_model: None,
            model_detection: ModelDetection::CliBanner,
            smartness: 9,
            strengths: &[
                AgentCapability::CodebaseReasoning,
                AgentCapability::Implementation,
                AgentCapability::Review,
                AgentCapability::Debugging,
            ],
            context_notes: &[
                "Strong default for broad repository understanding, multi-file edits, and code review.",
                "Usually exposes its active model in the TUI banner or startup text when configured by the CLI.",
            ],
            best_for: &[
                "planning complex changes",
                "reviewing diffs",
                "large refactors",
                "implementation with tests",
            ],
            planned_sidebar_actions: &[
                "delegate task",
                "ask for review",
                "compare plan against Codex",
                "summarize current pane",
            ],
        },
        AgentType::Codex => AgentContextProfile {
            agent_type,
            label: "Codex CLI",
            default_model: None,
            model_detection: ModelDetection::CliBanner,
            smartness: 9,
            strengths: &[
                AgentCapability::Implementation,
                AgentCapability::Debugging,
                AgentCapability::TerminalOps,
                AgentCapability::CodebaseReasoning,
            ],
            context_notes: &[
                "Good default for surgical coding, build fixes, and terminal-native verification loops.",
                "Model may be supplied by the Codex config or surfaced in the TUI; inspect terminal buffer for the best available signal.",
            ],
            best_for: &[
                "coding fixes",
                "running tests",
                "debugging build failures",
                "iterative verification",
            ],
            planned_sidebar_actions: &[
                "delegate implementation",
                "run verification loop",
                "ask for status",
                "handoff focused bug",
            ],
        },
        AgentType::Opencode => AgentContextProfile {
            agent_type,
            label: "OpenCode",
            default_model: None,
            model_detection: ModelDetection::Configuration,
            smartness: 7,
            strengths: &[
                AgentCapability::Implementation,
                AgentCapability::TerminalOps,
                AgentCapability::Debugging,
            ],
            context_notes: &[
                "Useful as an additional coding terminal when you want parallel exploration.",
                "Model routing is provider-config dependent, so treat detection as advisory unless the buffer shows an explicit model.",
            ],
            best_for: &[
                "parallel edits",
                "alternative implementation passes",
                "lighter bug fixes",
            ],
            planned_sidebar_actions: &[
                "delegate parallel attempt",
                "ask for alternative",
                "compare output",
            ],
        },
        AgentType::OpencodeNative => AgentContextProfile {
            agent_type,
            label: "OpenCode (API)",
            default_model: None,
            model_detection: ModelDetection::Configuration,
            smartness: 8,
            strengths: &[
                AgentCapability::Implementation,
                AgentCapability::TerminalOps,
                AgentCapability::Debugging,
            ],
            context_notes: &[
                "Runs opencode serve locally; orchestrator input uses prompt_async instead of PTY typing.",
                "The attach TUI is for visibility; bridge/MCP writes target the API session.",
            ],
            best_for: &[
                "orchestrated workers",
                "API-driven prompts",
                "parallel OpenCode sessions",
            ],
            planned_sidebar_actions: &[
                "delegate via API",
                "inspect session health",
                "reply to permission prompts",
            ],
        },
        AgentType::Cmd => AgentContextProfile {
            agent_type,
            label: "Command Prompt",
            default_model: None,
            model_detection: ModelDetection::Unknown,
            smartness: 1,
            strengths: &[AgentCapability::TerminalOps],
            context_notes: &[
                "Plain cmd.exe-style shell pane. It has no model; use it for deterministic Windows command workflows.",
            ],
            best_for: &[
                "Windows command prompt workflows",
                "batch commands",
                "environment inspection",
            ],
            planned_sidebar_actions: &["run command", "capture output", "prepare environment"],
        },
        AgentType::Powershell => AgentContextProfile {
            agent_type,
            label: "PowerShell",
            default_model: None,
            model_detection: ModelDetection::Unknown,
            smartness: 1,
            strengths: &[AgentCapability::TerminalOps],
            context_notes: &[
                "Plain shell pane. It has no model; use it for deterministic commands and scripts.",
            ],
            best_for: &["build commands", "file inspection", "manual scripts"],
            planned_sidebar_actions: &["run command", "capture output", "prepare environment"],
        },
        AgentType::Bash => AgentContextProfile {
            agent_type,
            label: "Bash",
            default_model: None,
            model_detection: ModelDetection::Unknown,
            smartness: 1,
            strengths: &[AgentCapability::TerminalOps],
            context_notes: &["Plain shell pane. It has no model; use it for POSIX-flavored commands."],
            best_for: &["shell commands", "cross-platform scripts", "log inspection"],
            planned_sidebar_actions: &["run command", "capture output", "prepare environment"],
        },
        AgentType::Cursor => AgentContextProfile {
            agent_type,
            label: "Cursor IDE",
            default_model: None,
            model_detection: ModelDetection::Unknown,
            smartness: 6,
            strengths: &[
                AgentCapability::UiOrchestration,
                AgentCapability::Implementation,
            ],
            context_notes: &[
                "IDE launcher rather than a terminal TUI. Treat model information as unavailable unless a future Cursor bridge reports it.",
            ],
            best_for: &["opening the workspace visually", "manual user-guided edits"],
            planned_sidebar_actions: &["open project", "focus editor", "handoff manual review"],
        },
        AgentType::CursorAgent => AgentContextProfile {
            agent_type,
            label: "Cursor Agent CLI",
            default_model: None,
            model_detection: ModelDetection::CliBanner,
            smartness: 8,
            strengths: &[AgentCapability::CodebaseReasoning, AgentCapability::Implementation, AgentCapability::Review],
            context_notes: &["Headless Cursor Agent CLI. Read-only delegated tasks use its documented plan mode."],
            best_for: &["headless code tasks", "structured result reporting"],
            planned_sidebar_actions: &["delegate task", "inspect result", "review changes"],
        },
    }
}

pub fn detect_model_from_buffer(buffer: &str) -> Option<String> {
    MODEL_PATTERNS.iter().find_map(|pattern| {
        pattern
            .regex
            .captures(buffer)
            .and_then(|captures| captures.get(pattern.capture))
            .map(|capture| capture.as_str().to_string())
    })
}

pub fn inspect_agent_model(
    pane_id: impl Into<String>,
    agent_type: AgentType,
    buffer: &str,
) -> AgentModelInspection {
    inspect_agent_model_with_registry(None, pane_id, agent_type, buffer)
}

pub fn inspect_agent_model_with_registry(
    registry: Option<&Arc<Mutex<PaneRegistry>>>,
    pane_id: impl Into<String>,
    agent_type: AgentType,
    buffer: &str,
) -> AgentModelInspection {
    let pane_id = pane_id.into();
    let profile = get_agent_context_profile(agent_type);

    if agent_type == AgentType::OpencodeNative {
        if let Some(registry) = registry {
            if let Ok(inspection) =
                inspect_opencode_native_model(registry, &pane_id, agent_type, buffer, &profile)
            {
                return inspection;
            }
        }
    }

    let buffer_fallback = detect_model_from_buffer(buffer);
    if let Some(detected_model) = buffer_fallback.clone() {
        return AgentModelInspection {
            pane_id,
            agent_type,
            detected_model: Some(detected_model),
            provider_id: None,
            source: "buffer",
            confidence: "low",
            smartness: profile.smartness,
            notes: vec![
                "Detected from terminal buffer text; prefer read_opencode_worker_status for opencode_native.".into(),
            ],
            session_model: None,
            last_user_model: None,
            footer_model_source: None,
            buffer_fallback,
        };
    }

    AgentModelInspection {
        pane_id,
        agent_type,
        detected_model: profile.default_model.map(str::to_string),
        provider_id: None,
        source: if profile.default_model.is_some() {
            "profile"
        } else {
            "unknown"
        },
        confidence: "low",
        smartness: profile.smartness,
        notes: if profile.default_model.is_some() {
            vec!["Using static agent profile default; no model visible in buffer.".into()]
        } else {
            vec!["No model visible in buffer and this agent has no static default.".into()]
        },
        session_model: None,
        last_user_model: None,
        footer_model_source: None,
        buffer_fallback: None,
    }
}

fn inspect_opencode_native_model(
    registry: &Arc<Mutex<PaneRegistry>>,
    pane_id: &str,
    agent_type: AgentType,
    buffer: &str,
    profile: &AgentContextProfile,
) -> Result<AgentModelInspection, String> {
    let status = crate::opencode::status::worker_status(registry, pane_id)?;
    let session_model =
        status
            .session_model
            .as_ref()
            .map(|model| crate::opencode::client::OpenCodeModelRef {
                provider_id: model.provider_id.clone(),
                model_id: model.model_id.clone(),
            });
    let last_user_model =
        status
            .last_user_model
            .as_ref()
            .map(|model| crate::opencode::client::OpenCodeModelRef {
                provider_id: model.provider_id.clone(),
                model_id: model.model_id.clone(),
            });
    let footer_model_source = status
        .footer_model_source
        .as_ref()
        .map(|source| match source {
            crate::opencode::status::FooterModelSource::Session => "session",
            crate::opencode::status::FooterModelSource::LastUser => "last_user",
            crate::opencode::status::FooterModelSource::Unknown => "unknown",
        });

    let (detected_model, provider_id, source, confidence) =
        if let Some(model) = last_user_model.as_ref() {
            (
                Some(model.model_id.clone()),
                Some(model.provider_id.clone()),
                "last_user",
                "high",
            )
        } else if let Some(model) = session_model.as_ref() {
            (
                Some(model.model_id.clone()),
                Some(model.provider_id.clone()),
                "session",
                "medium",
            )
        } else {
            (None, None, "unknown", "low")
        };

    let buffer_fallback = detect_model_from_buffer(buffer);
    let mut notes =
        vec!["OpenCode session API model (footer follows last user message on attach).".into()];
    if buffer_fallback.is_some() {
        notes.push("Buffer heuristic differs — trust session/last_user fields.".into());
    }

    Ok(AgentModelInspection {
        pane_id: pane_id.to_string(),
        agent_type,
        detected_model,
        provider_id,
        source,
        confidence,
        smartness: profile.smartness,
        notes,
        session_model,
        last_user_model,
        footer_model_source,
        buffer_fallback,
    })
}

pub fn build_pane_agent_context(
    registry: Option<&Arc<Mutex<PaneRegistry>>>,
    pane: PaneInfo,
    buffer: &str,
) -> Option<PaneAgentContext> {
    let agent_type = AgentType::parse(&pane.agent_type)?;
    let context = get_agent_context_profile(agent_type);
    let model = inspect_agent_model_with_registry(registry, &pane.id, agent_type, buffer);
    let recent_buffer_preview = buffer.lines().rev().take(40).collect::<Vec<_>>();
    let recent_buffer_preview = recent_buffer_preview
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n");

    Some(PaneAgentContext {
        pane,
        context,
        model,
        recent_buffer_preview,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_all_agent_profiles() {
        let profiles = list_agent_context_profiles();
        assert_eq!(profiles.len(), 8);
        assert!(profiles
            .iter()
            .any(|profile| profile.agent_type == AgentType::Codex));
        assert!(profiles.iter().all(|profile| !profile.best_for.is_empty()));
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
        assert_eq!(context.context.agent_type, AgentType::Codex);
        assert_eq!(context.model.detected_model.as_deref(), Some("o3"));
        assert_eq!(context.model.source, "buffer");
        assert!(context.recent_buffer_preview.contains("line 3"));
    }
}
