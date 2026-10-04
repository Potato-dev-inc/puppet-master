use once_cell::sync::Lazy;
use regex::Regex;
use serde::Serialize;

use crate::pty::agents::AgentType;
use crate::pty::registry::{PaneInfo, PaneRegistry};
use parking_lot::Mutex;
use std::sync::Arc;

mod inspect;
mod profiles;

pub use inspect::*;
pub use profiles::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AgentCapability {
    CodebaseReasoning,
    Implementation,
    Review,
    Debugging,
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

#[cfg(test)]
mod tests;
