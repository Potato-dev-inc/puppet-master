//! Pure helpers shaping operations for callers: applied context policy, trimmed output
//! baselines and the API projection.

use super::model::*;

/// Context policy actually applied by `delegate_work`. It never builds a context packet, so
/// an unspecified policy is `fresh` for a first turn and `resume` when continuing a worker
/// that already has earlier turns; `packet` is reported only when explicitly requested.
pub fn applied_delegate_context_policy(
    requested: Option<ContextPolicy>,
    turn_index: u32,
    prior_turns_for_handle: usize,
) -> ContextPolicy {
    requested.unwrap_or(if turn_index > 0 || prior_turns_for_handle > 0 {
        ContextPolicy::Resume
    } else {
        ContextPolicy::Fresh
    })
}

/// Trim TUI chrome from dispatch baselines before exposing operations over MCP/HTTP.
pub fn compact_output_baseline(raw: &str) -> Option<String> {
    let mut lines = Vec::new();
    for line in raw.lines() {
        let trimmed: String = line
            .chars()
            .filter(|ch| {
                !matches!(
                    ch,
                    '█' | '▀' | '▐' | '▔' | '░' | '▁' | '▂' | '▃' | '▄' | '▅' | '▆' | '▇' | '▉' | '▊' | '▋' | '▌'
                        | '▍' | '▎' | '▏' | '─' | '│' | '┌' | '┐' | '└' | '┘' | '├' | '┤'
                        | '┬' | '┴' | '┼'
                )
            })
            .collect::<String>()
            .trim()
            .to_string();
        if trimmed.len() >= 4 && trimmed.chars().any(|ch| ch.is_alphanumeric()) {
            lines.push(trimmed);
        }
    }
    if lines.is_empty() {
        return None;
    }
    let start = lines.len().saturating_sub(8);
    let mut tail = lines[start..].join("\n");
    if tail.len() > 480 {
        let mut cut = tail.len() - 479;
        while !tail.is_char_boundary(cut) {
            cut += 1;
        }
        tail = format!("…{}", &tail[cut..]);
    }
    Some(tail)
}

pub fn snapshot_for_api(mut snapshot: OperationSnapshot) -> OperationSnapshot {
    snapshot.output_baseline = None;
    snapshot
}
