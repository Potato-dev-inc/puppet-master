//! The words attention uses, in one place: the pane heading, the risk line and the sentence of each
//! kind. Plain sentence case, no raw ids. The push notifications and the cards both read these, so a
//! prompt cannot be "Check first" on the desktop and "Medium risk" on the phone.

use super::model::{Kind, Level};
use std::collections::HashMap;

/// A pane heading is cut to this many characters.
pub const MAX_HEADING_CHARS: usize = 40;
/// The first line of a push body is at most this long (design decision D5).
pub const MAX_FIRST_LINE_CHARS: usize = 56;

/// What `heading` needs to know about a pane.
#[derive(Debug, Clone)]
pub struct PaneName {
    pub id: String,
    pub agent_type: String,
    pub created_at: i64,
    pub title: Option<String>,
}

/// The heading every surface shows for each pane: the person's title (or the auto title), else
/// the agent's label numbered when several panes run the same agent ("Claude Code 2"), oldest first.
/// This is `tabLabels` / `paneHeading` of the front end.
pub fn headings(panes: &[PaneName], label_of: impl Fn(&str) -> String) -> HashMap<String, String> {
    let mut ordered: Vec<&PaneName> = panes.iter().collect();
    ordered.sort_by(|a, b| a.created_at.cmp(&b.created_at).then_with(|| a.id.cmp(&b.id)));
    let mut totals: HashMap<&str, usize> = HashMap::new();
    for pane in &ordered {
        *totals.entry(pane.agent_type.as_str()).or_default() += 1;
    }
    let mut seen: HashMap<&str, usize> = HashMap::new();
    let mut out = HashMap::new();
    for pane in ordered {
        let n = {
            let entry = seen.entry(pane.agent_type.as_str()).or_default();
            *entry += 1;
            *entry
        };
        let label = label_of(&pane.agent_type);
        let title = pane.title.as_deref().map(str::trim).filter(|t| !t.is_empty());
        let heading = match title {
            Some(title) => title.to_string(),
            None if totals.get(pane.agent_type.as_str()).copied().unwrap_or(0) > 1 => format!("{label} {n}"),
            None => label,
        };
        out.insert(pane.id.clone(), cut(&heading, MAX_HEADING_CHARS));
    }
    out
}

/// Cut to `max` characters (not bytes), ending in an ellipsis when cut; whitespace collapses.
pub fn cut(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        return flat;
    }
    let mut cut: String = flat.chars().take(max.saturating_sub(1)).collect();
    cut = cut.trim_end().to_string();
    cut.push('…');
    cut
}

/// "High risk · Delete a folder outside this project", or just "High risk" without a headline.
pub fn risk_line(level: Level, headline: Option<&str>) -> String {
    match headline.map(str::trim).filter(|h| !h.is_empty()) {
        Some(headline) => format!("{} · {headline}", level.word()),
        None => level.word().to_string(),
    }
}

/// The card title of a kind that has no text of its own.
pub fn title_for(kind: Kind, agent: &str) -> String {
    match kind {
        Kind::Approval => format!("{agent} is waiting for you"),
        Kind::TrustGate => "Trust this folder?".to_string(),
        Kind::Error => format!("{agent} hit an error"),
        Kind::Stuck => format!("{agent} seems stuck"),
        Kind::Question => format!("{agent} has a question"),
        Kind::Consent => format!("{agent} wants to control another agent"),
        Kind::Answer => format!("{agent} answered your question"),
        Kind::Done => format!("{agent} finished"),
        Kind::Review => format!("{agent} has something to review"),
        Kind::Budget => "Budget used up".to_string(),
        Kind::Limit => "Usage limit reached".to_string(),
        Kind::Tamper => "Tests were changed".to_string(),
        Kind::Setup => "Setup failed".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane(id: &str, agent: &str, created: i64, title: Option<&str>) -> PaneName {
        PaneName { id: id.into(), agent_type: agent.into(), created_at: created, title: title.map(str::to_string) }
    }

    fn label(agent: &str) -> String {
        match agent {
            "claude" => "Claude Code".into(),
            other => other.to_string(),
        }
    }

    #[test]
    fn headings_prefer_the_title_then_number_repeated_agents_oldest_first() {
        let panes = vec![
            pane("b", "claude", 20, None),
            pane("a", "claude", 10, None),
            pane("c", "codex", 5, None),
            pane("d", "claude", 30, Some("  auth refactor ")),
        ];
        let h = headings(&panes, label);
        assert_eq!(h["a"], "Claude Code 1");
        assert_eq!(h["b"], "Claude Code 2");
        assert_eq!(h["c"], "codex", "a lone agent is not numbered");
        assert_eq!(h["d"], "auth refactor");
    }

    #[test]
    fn a_heading_is_cut_to_forty_characters() {
        let h = headings(&[pane("a", "claude", 1, Some(&"t".repeat(90)))], label);
        assert_eq!(h["a"].chars().count(), MAX_HEADING_CHARS);
        assert!(h["a"].ends_with('…'));
    }

    #[test]
    fn the_risk_line_uses_the_card_words() {
        assert_eq!(risk_line(Level::Medium, Some("Install packages")), "Check first · Install packages");
        assert_eq!(risk_line(Level::High, None), "High risk");
        assert_eq!(risk_line(Level::Low, Some("  ")), "Low risk");
    }
}
