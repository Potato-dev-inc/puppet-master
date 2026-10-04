//! MCP resources and prompts advertised alongside the tools.

use super::types::{PromptArgument, PromptDefinition, ResourceDefinition};

pub fn resources() -> Vec<ResourceDefinition> {
    vec![
        ResourceDefinition {
            uri: "puppet-master://session",
            name: "Current session",
            description: "Current Puppet Master session state and orchestration context.",
            mime_type: "application/json",
        },
        ResourceDefinition {
            uri: "puppet-master://panes",
            name: "Live panes",
            description: "Live terminal panes known to the Rust bridge.",
            mime_type: "application/json",
        },
        ResourceDefinition {
            uri: "puppet-master://panes/{id}/digest",
            name: "Pane digest",
            description: "Latest pane digest supplied through the Rust session context event log.",
            mime_type: "application/json",
        },
        ResourceDefinition {
            uri: "puppet-master://tasks",
            name: "Tasks",
            description: "Task board projection rebuilt from the Rust event log.",
            mime_type: "application/json",
        },
        ResourceDefinition {
            uri: "puppet-master://locks",
            name: "Locks",
            description: "Resource lock projection rebuilt from the Rust event log.",
            mime_type: "application/json",
        },
        ResourceDefinition {
            uri: "puppet-master://audit",
            name: "Audit",
            description: "Recent coordination and MCP audit entries.",
            mime_type: "application/json",
        },
    ]
}

pub fn prompts() -> Vec<PromptDefinition> {
    vec![
        PromptDefinition {
            name: "status_check",
            description: "Inspect bridge health, panes, tasks, and locks before choosing next action.",
            arguments: vec![],
        },
        PromptDefinition {
            name: "summarize_session",
            description: "Summarize current session state, active work, blockers, and recommended next steps.",
            arguments: vec![],
        },
        PromptDefinition {
            name: "handoff_to_worker",
            description: "Prepare a concise handoff prompt for a worker pane.",
            arguments: vec![PromptArgument {
                name: "pane_id",
                description: "Target worker pane id.",
                required: true,
            }],
        },
        PromptDefinition {
            name: "delegate_refactor",
            description: "Render a structured refactor delegation prompt with acceptance criteria and evidence requirements.",
            arguments: vec![],
        },
        PromptDefinition {
            name: "implement_with_review",
            description: "Render a two-step implementation prompt that asks for verification and reviewer evidence.",
            arguments: vec![],
        },
        PromptDefinition {
            name: "fix_ci",
            description: "Render a CI-fix prompt focused on reproducing failures and reporting command output.",
            arguments: vec![],
        },
    ]
}
