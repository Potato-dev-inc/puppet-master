//! Text Your Agents: policy. Which attention cards may be answered from the chat (D-9), and which
//! capability each command needs. Pure rules, no I/O.
//!
//! The matches here have no wildcard on purpose: a new `attention::model::Kind` (or `Command`)
//! must fail to compile until someone decides what the chat may do with it.

use crate::attention::model::{Kind, Level};
use crate::channels::commands::Command;
use crate::mobile_pairing::{Cap, Need};

/// Shown under a card the chat cannot answer, when the answer must be made on the PC or in the app.
const OPEN_TO_ANSWER: &str = "Open Puppet Master to answer.";
const OPEN_TO_DECIDE: &str = "Open Puppet Master to decide.";
const OPEN_TO_ACT: &str = "Open Puppet Master to act on this.";
const ONLY_YOU_ON_PC: &str = "Only you can pass this, on your PC.";
const DECIDE_ON_PC: &str = "An agent wants more control. Decide on your PC.";

/// What the chat may do with one card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answerable {
    /// The question's choices as buttons (one row each), plus free text by reply when allowed.
    Choices,
    /// **Allow once…** then a confirm tap. Deny is a single tap. Never a "don't ask again" choice.
    AllowOnceWithConfirm,
    /// No buttons. The string is the one sentence the chat shows instead.
    Never(&'static str),
}

/// The D-9 table. `risk` is the approval's risk level; `None` means the profile read no risk.
pub fn answerable(kind: Kind, risk: Option<Level>) -> Answerable {
    match kind {
        Kind::Question => Answerable::Choices,
        Kind::Approval => match risk {
            Some(Level::Low) => Answerable::AllowOnceWithConfirm,
            Some(Level::Medium) | Some(Level::High) | None => Answerable::Never(OPEN_TO_ANSWER),
        },
        Kind::TrustGate => Answerable::Never(ONLY_YOU_ON_PC),
        Kind::Consent => Answerable::Never(DECIDE_ON_PC),
        Kind::Budget | Kind::Limit => Answerable::Never(OPEN_TO_DECIDE),
        // Info only: the chat may show the title, never a button.
        Kind::Tamper
        | Kind::Setup
        | Kind::Error
        | Kind::Stuck
        | Kind::Review
        | Kind::Done
        | Kind::Answer => Answerable::Never(OPEN_TO_ACT),
    }
}

/// The capability a command needs. `None` means being paired is enough (help, stop, greetings).
pub fn command_need(cmd: &Command) -> Option<Need> {
    match cmd {
        Command::Start(_) | Command::Help | Command::Stop | Command::Unknown => None,
        Command::Status => Some(Need::AnyOf(Cap::Watch, Cap::Answer)),
        Command::Panes => Some(Need::Cap(Cap::Watch)),
        // Anything that writes into a pane or the companion. A too-long message is refused the
        // same way, so it needs `type` too.
        Command::To { .. } | Command::Companion { .. } | Command::Text(_) | Command::TooLong => {
            Some(Need::Cap(Cap::Type))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RISKS: [Option<Level>; 4] = [
        None,
        Some(Level::Low),
        Some(Level::Medium),
        Some(Level::High),
    ];

    /// Every kind, in a fixed order. `position` has no wildcard, so adding a `Kind` breaks this
    /// test file until the list (and the table below) is updated.
    const ALL_KINDS: [Kind; 13] = [
        Kind::Consent,
        Kind::Approval,
        Kind::TrustGate,
        Kind::Question,
        Kind::Tamper,
        Kind::Budget,
        Kind::Limit,
        Kind::Setup,
        Kind::Error,
        Kind::Stuck,
        Kind::Review,
        Kind::Done,
        Kind::Answer,
    ];

    fn position(kind: Kind) -> usize {
        match kind {
            Kind::Consent => 0,
            Kind::Approval => 1,
            Kind::TrustGate => 2,
            Kind::Question => 3,
            Kind::Tamper => 4,
            Kind::Budget => 5,
            Kind::Limit => 6,
            Kind::Setup => 7,
            Kind::Error => 8,
            Kind::Stuck => 9,
            Kind::Review => 10,
            Kind::Done => 11,
            Kind::Answer => 12,
        }
    }

    /// The D-9 table written out again on purpose, per kind and risk (no wildcard).
    fn expected(kind: Kind, risk: Option<Level>) -> Answerable {
        match kind {
            Kind::Question => Answerable::Choices,
            Kind::Approval => match risk {
                Some(Level::Low) => Answerable::AllowOnceWithConfirm,
                Some(Level::Medium) | Some(Level::High) | None => Answerable::Never(OPEN_TO_ANSWER),
            },
            Kind::TrustGate => Answerable::Never(ONLY_YOU_ON_PC),
            Kind::Consent => Answerable::Never(DECIDE_ON_PC),
            Kind::Budget | Kind::Limit => Answerable::Never(OPEN_TO_DECIDE),
            Kind::Tamper
            | Kind::Setup
            | Kind::Error
            | Kind::Stuck
            | Kind::Review
            | Kind::Done
            | Kind::Answer => Answerable::Never(OPEN_TO_ACT),
        }
    }

    #[test]
    fn kind_list_covers_every_variant() {
        for (i, kind) in ALL_KINDS.iter().enumerate() {
            assert_eq!(position(*kind), i);
        }
        assert_eq!(ALL_KINDS.len(), 13);
    }

    #[test]
    fn d9_table_is_exhaustive_over_kind_and_risk() {
        for kind in ALL_KINDS {
            for risk in RISKS {
                assert_eq!(
                    answerable(kind, risk),
                    expected(kind, risk),
                    "kind {kind:?} risk {risk:?}"
                );
            }
        }
    }

    #[test]
    fn question_is_choices_at_every_risk() {
        for risk in RISKS {
            assert_eq!(answerable(Kind::Question, risk), Answerable::Choices);
        }
    }

    #[test]
    fn low_risk_approval_is_allow_once_with_confirm() {
        assert_eq!(
            answerable(Kind::Approval, Some(Level::Low)),
            Answerable::AllowOnceWithConfirm
        );
    }

    #[test]
    fn medium_high_or_unrated_approval_has_no_buttons() {
        for risk in [Some(Level::Medium), Some(Level::High), None] {
            assert_eq!(
                answerable(Kind::Approval, risk),
                Answerable::Never(OPEN_TO_ANSWER)
            );
        }
    }

    #[test]
    fn trust_consent_budget_limit_are_never_answerable_from_chat() {
        for risk in RISKS {
            assert_eq!(
                answerable(Kind::TrustGate, risk),
                Answerable::Never(ONLY_YOU_ON_PC)
            );
            assert_eq!(
                answerable(Kind::Consent, risk),
                Answerable::Never(DECIDE_ON_PC)
            );
            assert_eq!(
                answerable(Kind::Budget, risk),
                Answerable::Never(OPEN_TO_DECIDE)
            );
            assert_eq!(
                answerable(Kind::Limit, risk),
                Answerable::Never(OPEN_TO_DECIDE)
            );
        }
    }

    #[test]
    fn info_only_kinds_never_have_buttons() {
        for kind in [
            Kind::Tamper,
            Kind::Setup,
            Kind::Error,
            Kind::Stuck,
            Kind::Review,
            Kind::Done,
            Kind::Answer,
        ] {
            for risk in RISKS {
                assert!(
                    matches!(answerable(kind, risk), Answerable::Never(_)),
                    "kind {kind:?}"
                );
            }
        }
    }

    #[test]
    fn every_refusal_sentence_is_non_empty() {
        for kind in ALL_KINDS {
            for risk in RISKS {
                if let Answerable::Never(text) = answerable(kind, risk) {
                    assert!(!text.is_empty(), "kind {kind:?}");
                }
            }
        }
    }

    #[test]
    fn command_need_for_every_variant() {
        let cases: Vec<(Command, Option<Need>)> = vec![
            (Command::Start(None), None),
            (Command::Start(Some("PM7K2QX9".into())), None),
            (Command::Help, None),
            (Command::Stop, None),
            (Command::Unknown, None),
            (Command::Status, Some(Need::AnyOf(Cap::Watch, Cap::Answer))),
            (Command::Panes, Some(Need::Cap(Cap::Watch))),
            (
                Command::To {
                    n: 2,
                    text: "run the tests".into(),
                },
                Some(Need::Cap(Cap::Type)),
            ),
            (
                Command::Companion {
                    name: "Mochi".into(),
                    text: "hi".into(),
                },
                Some(Need::Cap(Cap::Type)),
            ),
            (
                Command::Text("is the build green?".into()),
                Some(Need::Cap(Cap::Type)),
            ),
            (Command::TooLong, Some(Need::Cap(Cap::Type))),
        ];
        for (cmd, want) in cases {
            assert_eq!(command_need(&cmd), want, "{cmd:?}");
        }
    }

    #[test]
    fn control_is_never_needed_by_a_chat_command() {
        let all = [
            Command::Start(None),
            Command::Help,
            Command::Status,
            Command::Panes,
            Command::To {
                n: 1,
                text: String::new(),
            },
            Command::Companion {
                name: String::new(),
                text: String::new(),
            },
            Command::Stop,
            Command::Text(String::new()),
            Command::TooLong,
            Command::Unknown,
        ];
        for cmd in all {
            assert_ne!(command_need(&cmd), Some(Need::Cap(Cap::Control)), "{cmd:?}");
            assert_ne!(
                command_need(&cmd),
                Some(Need::AnyOf(Cap::Control, Cap::Control)),
                "{cmd:?}"
            );
        }
    }
}
