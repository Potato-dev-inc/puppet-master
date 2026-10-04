//! Detecting and inspecting the model a pane's agent is running.

use super::*;

pub(super) struct ModelPattern {
    regex: Regex,
    capture: usize,
}

pub(super) static MODEL_PATTERNS: Lazy<Vec<ModelPattern>> = Lazy::new(|| {
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

pub fn detect_model_from_buffer(buffer: &str) -> Option<String> {
    MODEL_PATTERNS.iter().find_map(|pattern| {
        pattern
            .regex
            .captures(buffer)
            .and_then(|captures| captures.get(pattern.capture))
            .map(|capture| capture.as_str().to_string())
    })
}

#[cfg(test)]
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

    let buffer_fallback = crate::tui::profile_for(agent_type.as_str())
        .detect_footer_model(buffer)
        .or_else(|| detect_model_from_buffer(buffer));
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

pub(super) fn inspect_opencode_native_model(
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
