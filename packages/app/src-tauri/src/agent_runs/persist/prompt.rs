//! Turn prompts: what a handle's earlier turns contribute (conversation facts) and how a turn's
//! prompt is rendered for each context policy.

use super::{resolve_continuity, PARENT_SUMMARY_LIMIT};
use crate::operations::{self, ContextContinuity, ContextPolicy, OperationStatus};

#[derive(Debug, Clone)]
pub struct TurnPromptArgs<'a> {
    pub policy: ContextPolicy,
    pub task: &'a str,
    pub scope: Option<&'a str>,
    pub prior_result: Option<&'a str>,
    pub prior_user: Option<&'a str>,
    pub selected_history: &'a [String],
    pub can_resume: bool,
    pub is_followup: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnPrompt {
    pub prompt: String,
    pub policy: ContextPolicy,
    pub continuity: ContextContinuity,
}

pub fn conversation_facts(project: &str, handle: &str) -> Option<String> {
    let turns = operations::turns_for_handle(project, handle).ok()?;
    let mut parts = Vec::new();
    for turn in turns {
        let unanswered = matches!(
            turn.status,
            OperationStatus::Failed | OperationStatus::Cancelled
        ) && turn
            .result
            .as_deref()
            .map_or(true, |text| text.trim().is_empty());
        if unanswered {
            continue;
        }
        let task = original_task(&turn.task);
        if !task.is_empty() {
            parts.push(format!("User: {task}"));
        }
        if let Some(result) = turn
            .result
            .as_deref()
            .and_then(|text| crate::agent_runs::completion::clean_extracted_result(text, ""))
        {
            parts.push(format!("Assistant: {result}"));
        }
    }
    let text = parts.join("\n");
    if text.is_empty() {
        None
    } else {
        Some(owned_bound_summary(&text))
    }
}

/// Stored turn tasks are the rendered prompt; cut the blocks `render_turn_prompt` and the
/// acceptance wrapper appended so replayed history does not nest earlier packets.
fn original_task(stored: &str) -> &str {
    const MARKERS: [&str; 5] = [
        "

Scope: ",
        "

Prior context:
",
        "

Parent summary:
",
        "
Selected history:",
        "

Acceptance criteria:
",
    ];
    let end = MARKERS
        .iter()
        .filter_map(|marker| stored.find(marker))
        .min()
        .unwrap_or(stored.len());
    stored[..end].trim()
}

pub fn render_turn_prompt(args: TurnPromptArgs<'_>) -> TurnPrompt {
    let continuity = resolve_continuity(args.policy, args.can_resume, args.is_followup);
    let cleaned_prior = args
        .prior_result
        .and_then(|text| crate::agent_runs::completion::clean_extracted_result(text, ""));
    let reconstructed = if args.is_followup && !args.can_resume {
        Some(reconstruct_facts(args.prior_user, cleaned_prior.as_deref()))
            .filter(|text| !text.is_empty())
    } else {
        None
    };
    let reconstructed_facts = reconstructed.as_deref();
    let prompt = match args.policy {
        ContextPolicy::Fresh => args.task.to_string(),
        ContextPolicy::Packet => context_packet(args.task, args.scope, reconstructed_facts),
        ContextPolicy::ParentSummary => {
            let summary = if args.is_followup {
                reconstructed_facts
                    .or(cleaned_prior.as_deref())
                    .unwrap_or("(none)")
            } else {
                "(none)"
            };
            format!(
                "{}\n\nParent summary:\n{}",
                args.task,
                bound_summary(summary)
            )
        }
        ContextPolicy::Resume => context_packet(args.task, args.scope, reconstructed_facts),
        ContextPolicy::SelectedHistory => {
            let mut prompt = context_packet(args.task, args.scope, reconstructed_facts);
            if !args.selected_history.is_empty() {
                prompt.push_str("\nSelected history:");
                for message in args.selected_history {
                    prompt.push_str("\n- ");
                    prompt.push_str(message);
                }
            }
            prompt
        }
    };
    TurnPrompt {
        prompt,
        policy: args.policy,
        continuity,
    }
}

fn context_packet(task: &str, scope: Option<&str>, facts: Option<&str>) -> String {
    let scope = scope.map(str::trim).filter(|value| !value.is_empty());
    let facts = facts.map(bound_summary).filter(|value| !value.is_empty());
    if scope.is_none() && facts.is_none() {
        return task.to_string();
    }
    let mut prompt = task.to_string();
    if let Some(scope) = scope {
        prompt.push_str("\n\nScope: ");
        prompt.push_str(scope);
    }
    if let Some(facts) = facts {
        prompt.push_str("\n\nPrior context:\n");
        prompt.push_str(facts);
    }
    prompt
}

fn reconstruct_facts(prior_user: Option<&str>, prior_result: Option<&str>) -> String {
    let mut parts = Vec::new();
    if let Some(user) = prior_user.map(str::trim).filter(|value| !value.is_empty()) {
        parts.push(user.to_string());
    }
    if let Some(result) = prior_result
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if !parts.iter().any(|part| part.contains(result)) {
            parts.push(format!("Assistant: {result}"));
        }
    }
    parts.join("\n")
}

fn bound_summary(text: &str) -> &str {
    if text.len() <= PARENT_SUMMARY_LIMIT {
        return text;
    }
    let end = text
        .char_indices()
        .map(|(index, ch)| index + ch.len_utf8())
        .take_while(|end| *end <= PARENT_SUMMARY_LIMIT)
        .last()
        .unwrap_or(0);
    &text[..end]
}

fn owned_bound_summary(text: &str) -> String {
    bound_summary(text).to_string()
}

#[cfg(test)]
pub(in crate::agent_runs) fn original_task_for_test(stored: &str) -> &str {
    original_task(stored)
}
