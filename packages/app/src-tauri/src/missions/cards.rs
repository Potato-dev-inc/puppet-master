//! The mission's cards in the one attention list (Inbox, tray count, phone, push): the tamper card
//! ("The checks pass, but the tests were changed"), the stuck card, the setup card and, when it is
//! done, the review item. Raised with `attention::raise` under `mission:<id>` and cleared as soon as
//! the mission leaves that state. The budget card is the budgets module's own.
//!
//! The answers go through the mission routes (`POST /missions/<id>/decide`), never through a pane.

use super::model::{Mission, MissionState, Needs};
use crate::attention::model::{Action, Kind, Raised, Resolution};

pub fn key(m: &Mission) -> String {
    format!("mission:{}", m.id)
}

fn action(id: &str, label: &str, style: &str, needs: &str) -> Action {
    Action { id: id.into(), label: label.into(), style: style.into(), needs: needs.into() }
}

/// The agent that made the first blocking test change, for the tamper card's headline.
fn tamper_headline(m: &Mission) -> String {
    let changed_by: Vec<usize> = m
        .rounds
        .iter()
        .filter(|r| r.verdict.as_ref().is_some_and(|v| v.test_changes.blocks))
        .map(|r| r.agent)
        .collect();
    let first = changed_by.first().copied();
    if first.is_some() && changed_by.iter().all(|a| Some(*a) == first) {
        format!("The checks pass, but {} changed the tests", m.agent_label(first.unwrap_or(0)))
    } else {
        "The checks pass, but the tests were changed".into()
    }
}

pub fn tamper_lines(m: &Mission) -> Vec<String> {
    let all = m.latest_verdict().map(|v| v.test_changes.blocking_sentences()).unwrap_or_default();
    let mut lines: Vec<String> = all.iter().take(3).cloned().collect();
    let rest = all.len().saturating_sub(3);
    if rest > 0 {
        lines.push(if rest == 1 { "and 1 more change".into() } else { format!("and {rest} more changes") });
    }
    lines
}

/// The stuck card's body: "Claude Code and Codex each tried 3 turns. Still 9 of 12 pass."
pub fn stuck_sentence(m: &Mission) -> String {
    let mut names: Vec<String> = Vec::new();
    for r in &m.rounds {
        let label = m.agent_label(r.agent);
        if !names.contains(&label) {
            names.push(label);
        }
    }
    let who = match names.len() {
        0 => "The agents".to_string(),
        1 => names[0].clone(),
        _ => format!("{} and {}", names[..names.len() - 1].join(", "), names[names.len() - 1]),
    };
    let still = m.latest_verdict().map(|v| format!(" Still {} pass.", v.short())).unwrap_or_default();
    let verb = if names.len() > 1 { "each tried" } else { "tried" };
    format!("{who} {verb} without progress.{still}")
}

/// The card's headline while the mission needs the person.
pub fn headline(m: &Mission) -> Option<String> {
    match (m.state, m.needs) {
        (MissionState::NeedsYou, Some(Needs::Tamper)) => Some(tamper_headline(m)),
        (MissionState::NeedsYou, Some(Needs::Stuck)) => Some("The mission isn't getting anywhere".into()),
        (MissionState::NeedsYou, Some(Needs::Setup)) => Some("The mission can't go on".into()),
        (MissionState::NeedsYou, Some(Needs::Budget)) => Some("The mission used its budget".into()),
        _ => None,
    }
}

fn raised(m: &Mission) -> Option<Raised> {
    let title_suffix = super::engine::short(&m.objective);
    match (m.state, m.needs) {
        (MissionState::NeedsYou, Some(Needs::Tamper)) => {
            let mut lines = tamper_lines(m);
            lines.push("The mission isn't done until you decide.".into());
            Some(Raised {
                key: key(m),
                kind: Kind::Tamper,
                pane_id: None,
                title: format!("{} · {title_suffix}", tamper_headline(m)),
                sentence: Some(lines.join("\n")),
                expires_at_ms: None,
                actions: vec![
                    action("undo_tests", "Undo the test changes and keep going", "primary", "answer"),
                    action("accept_tests", "Accept the changes", "secondary", "answer"),
                ],
            })
        }
        (MissionState::NeedsYou, Some(Needs::Stuck)) => Some(Raised {
            key: key(m),
            kind: Kind::Stuck,
            pane_id: None,
            title: format!("The mission isn't getting anywhere · {title_suffix}"),
            sentence: Some(format!("{} Their notes are in MISSION.md.", stuck_sentence(m))),
            expires_at_ms: None,
            actions: vec![
                action("more_turns", "Try 5 more turns", "primary", "answer"),
                action("hand_to_me", "Hand it to me", "secondary", "answer"),
                action("stop", "Stop mission", "plain", "control"),
            ],
        }),
        (MissionState::NeedsYou, Some(Needs::Setup)) => Some(Raised {
            key: key(m),
            kind: Kind::Setup,
            pane_id: None,
            title: format!("The mission can't go on · {title_suffix}"),
            sentence: m.sentence.clone(),
            expires_at_ms: None,
            actions: vec![action("try_again", "Try again", "primary", "answer"), action("stop", "Stop mission", "plain", "control")],
        }),
        (MissionState::Green, _) => Some(Raised {
            key: key(m),
            kind: Kind::Review,
            pane_id: None,
            title: format!("Mission done: {} · {title_suffix}", m.latest_verdict().map(|v| format!("{} pass", v.short())).unwrap_or_default()),
            sentence: Some("Nothing has been merged. The separate copy waits for your review.".into()),
            expires_at_ms: None,
            actions: vec![action("review", "Review changes", "primary", "watch")],
        }),
        _ => None,
    }
}

/// Puts the card that matches the mission's state in the attention list, or takes it away.
/// A quiet mission (Night Shift) has no cards at all: the night report carries its results.
pub fn sync(m: &Mission) {
    if m.quiet {
        return;
    }
    match raised(m) {
        Some(card) => crate::attention::raise(card),
        None => {
            let resolution = Resolution {
                state: if m.state == MissionState::Stopped { "withdrawn".into() } else { "answered".into() },
                by: None,
                choice_label: None,
                reason: None,
                at_ms: super::store::now_ms(),
                // A mission card is decided in the app (mission_decide), never by a phone.
                device_name: None,
                device_id: None,
            };
            crate::attention::clear(&key(m), Some(resolution));
        }
    }
}
