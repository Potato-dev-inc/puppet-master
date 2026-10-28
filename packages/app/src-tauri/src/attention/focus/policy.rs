//! Tiers for pinged things while Focus is on (SPEC n33 3.2). Pure: no clock, no I/O, no agent names.
//!
//! Every table is a `match` with no wildcard arm, so a new attention or push kind does not compile until someone
//! decides its tier. Blocking cards (`Quiet`) are never held or hidden: they only lose the buzz. Safety and money
//! kinds (`Through`) always go at full strength. `Hold` is for things that can wait.

use super::model::{Class, Settings, Tier};
use crate::attention::model::{Kind as AttentionKind, Level};
use crate::push::triggers::Kind as PushKind;

/// What a hook does right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// Send as today.
    Send,
    /// Send without the buzz.
    Quiet,
    /// Do not send now. The thing is counted and listed in the summary.
    Hold,
}

/// The tier of an attention card. `risk` is the prompt reading; only `Approval` looks at it.
pub fn tier_of_attention(kind: AttentionKind, risk: Option<Level>) -> Tier {
    match kind {
        AttentionKind::Consent => Tier::Through,
        AttentionKind::Approval => match risk {
            Some(Level::High) => Tier::Through,
            Some(Level::Medium) | Some(Level::Low) | None => Tier::Quiet,
        },
        AttentionKind::TrustGate => Tier::Quiet,
        AttentionKind::Question => Tier::Quiet,
        AttentionKind::Limit => Tier::Quiet,
        AttentionKind::Setup => Tier::Quiet,
        AttentionKind::Error => Tier::Quiet,
        AttentionKind::Tamper => Tier::Through,
        AttentionKind::Budget => Tier::Through,
        AttentionKind::Stuck => Tier::Through,
        AttentionKind::Answer => Tier::Through,
        AttentionKind::Review => Tier::Hold,
        AttentionKind::Done => Tier::Hold,
    }
}

/// The tier of a push. Only `WaitingInput` looks at `risk`.
pub fn tier_of_push(kind: PushKind, risk: Option<Level>) -> Tier {
    match kind {
        PushKind::WaitingInput => match risk {
            Some(Level::High) => Tier::Through,
            Some(Level::Medium) | Some(Level::Low) | None => Tier::Quiet,
        },
        PushKind::TrustGate => Tier::Quiet,
        PushKind::Question => Tier::Quiet,
        PushKind::Limit => Tier::Quiet,
        PushKind::Error => Tier::Quiet,
        PushKind::Consent => Tier::Through,
        PushKind::Budget => Tier::Through,
        PushKind::RunFinished => Tier::Hold,
        PushKind::RunFailed => Tier::Hold,
        PushKind::CompanionReply => Tier::Hold,
        PushKind::NightShift => Tier::Hold,
        // The end message and the reminder are built by Focus itself, so they are never gated.
        PushKind::Focus => Tier::Through,
    }
}

/// The held-list class of a push, `Some` exactly for the push kinds whose tier is `Hold`.
pub fn class_of_push(kind: PushKind) -> Option<Class> {
    match kind {
        PushKind::RunFinished => Some(Class::RunDone),
        PushKind::RunFailed => Some(Class::RunFailed),
        PushKind::CompanionReply => Some(Class::Companion),
        PushKind::NightShift => Some(Class::Night),
        PushKind::WaitingInput => None,
        PushKind::Error => None,
        PushKind::Question => None,
        PushKind::TrustGate => None,
        PushKind::Consent => None,
        PushKind::Budget => None,
        PushKind::Limit => None,
        PushKind::Focus => None,
    }
}

/// What a hook does for a tier right now. `Through` always sends. `Quiet` sends quietly, or at full strength
/// when the person turned `quiet_blocking` off. `Hold` holds in every case.
pub fn decide(tier: Tier, settings: &Settings) -> Gate {
    match tier {
        Tier::Hold => Gate::Hold,
        Tier::Quiet => {
            if settings.quiet_blocking {
                Gate::Quiet
            } else {
                Gate::Send
            }
        }
        Tier::Through => Gate::Send,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attention::model::{lane_of, Lane};
    use std::collections::BTreeSet;

    const RISKS: [Option<Level>; 4] = [
        Some(Level::High),
        Some(Level::Medium),
        Some(Level::Low),
        None,
    ];

    /// Every attention kind, in a fixed order.
    const ATTENTION_ALL: [AttentionKind; 13] = [
        AttentionKind::Consent,
        AttentionKind::Approval,
        AttentionKind::TrustGate,
        AttentionKind::Question,
        AttentionKind::Tamper,
        AttentionKind::Budget,
        AttentionKind::Limit,
        AttentionKind::Setup,
        AttentionKind::Error,
        AttentionKind::Stuck,
        AttentionKind::Review,
        AttentionKind::Done,
        AttentionKind::Answer,
    ];

    /// Every push kind, in a fixed order.
    const PUSH_ALL: [PushKind; 12] = [
        PushKind::WaitingInput,
        PushKind::Error,
        PushKind::RunFinished,
        PushKind::RunFailed,
        PushKind::Question,
        PushKind::TrustGate,
        PushKind::Consent,
        PushKind::Budget,
        PushKind::Limit,
        PushKind::CompanionReply,
        PushKind::NightShift,
        PushKind::Focus,
    ];

    /// Exhaustive on purpose (no wildcard): a new attention kind breaks this test file until it is listed.
    fn attention_slot(kind: AttentionKind) -> usize {
        match kind {
            AttentionKind::Consent => 0,
            AttentionKind::Approval => 1,
            AttentionKind::TrustGate => 2,
            AttentionKind::Question => 3,
            AttentionKind::Tamper => 4,
            AttentionKind::Budget => 5,
            AttentionKind::Limit => 6,
            AttentionKind::Setup => 7,
            AttentionKind::Error => 8,
            AttentionKind::Stuck => 9,
            AttentionKind::Review => 10,
            AttentionKind::Done => 11,
            AttentionKind::Answer => 12,
        }
    }

    /// Exhaustive on purpose (no wildcard): a new push kind breaks this test file until it is listed.
    fn push_slot(kind: PushKind) -> usize {
        match kind {
            PushKind::WaitingInput => 0,
            PushKind::Error => 1,
            PushKind::RunFinished => 2,
            PushKind::RunFailed => 3,
            PushKind::Question => 4,
            PushKind::TrustGate => 5,
            PushKind::Consent => 6,
            PushKind::Budget => 7,
            PushKind::Limit => 8,
            PushKind::CompanionReply => 9,
            PushKind::NightShift => 10,
            PushKind::Focus => 11,
        }
    }

    fn settings(quiet_blocking: bool) -> Settings {
        Settings {
            quiet_blocking,
            ..Settings::default()
        }
    }

    #[test]
    fn attention_list_is_complete_and_in_order() {
        assert_eq!(ATTENTION_ALL.len(), 13);
        for (i, kind) in ATTENTION_ALL.iter().enumerate() {
            assert_eq!(attention_slot(*kind), i, "{}", kind.as_str());
        }
    }

    #[test]
    fn push_list_is_complete_and_in_order() {
        assert_eq!(PUSH_ALL.len(), 12);
        for (i, kind) in PUSH_ALL.iter().enumerate() {
            assert_eq!(push_slot(*kind), i, "{}", kind.as_str());
        }
    }

    #[test]
    fn attention_table_full() {
        // (kind, risk, tier). Only Approval reads the risk, so it has four rows; every other kind has one.
        let table: [(AttentionKind, Option<Level>, Tier); 16] = [
            (AttentionKind::Consent, None, Tier::Through),
            (AttentionKind::Approval, Some(Level::High), Tier::Through),
            (AttentionKind::Approval, Some(Level::Medium), Tier::Quiet),
            (AttentionKind::Approval, Some(Level::Low), Tier::Quiet),
            (AttentionKind::Approval, None, Tier::Quiet),
            (AttentionKind::TrustGate, None, Tier::Quiet),
            (AttentionKind::Question, None, Tier::Quiet),
            (AttentionKind::Tamper, None, Tier::Through),
            (AttentionKind::Budget, None, Tier::Through),
            (AttentionKind::Limit, None, Tier::Quiet),
            (AttentionKind::Setup, None, Tier::Quiet),
            (AttentionKind::Error, None, Tier::Quiet),
            (AttentionKind::Stuck, None, Tier::Through),
            (AttentionKind::Review, None, Tier::Hold),
            (AttentionKind::Done, None, Tier::Hold),
            (AttentionKind::Answer, None, Tier::Through),
        ];
        for (kind, risk, want) in table {
            assert_eq!(
                tier_of_attention(kind, risk),
                want,
                "{} risk {:?}",
                kind.as_str(),
                risk
            );
        }
        // Every kind has at least one row.
        for kind in ATTENTION_ALL {
            assert!(
                table.iter().any(|(k, _, _)| *k == kind),
                "{} has no row",
                kind.as_str()
            );
        }
    }

    #[test]
    fn push_table_full() {
        // (kind, risk, tier). Only WaitingInput reads the risk.
        let table: [(PushKind, Option<Level>, Tier); 15] = [
            (PushKind::WaitingInput, Some(Level::High), Tier::Through),
            (PushKind::WaitingInput, Some(Level::Medium), Tier::Quiet),
            (PushKind::WaitingInput, Some(Level::Low), Tier::Quiet),
            (PushKind::WaitingInput, None, Tier::Quiet),
            (PushKind::Error, None, Tier::Quiet),
            (PushKind::TrustGate, None, Tier::Quiet),
            (PushKind::Question, None, Tier::Quiet),
            (PushKind::Limit, None, Tier::Quiet),
            (PushKind::Consent, None, Tier::Through),
            (PushKind::Budget, None, Tier::Through),
            (PushKind::RunFinished, None, Tier::Hold),
            (PushKind::RunFailed, None, Tier::Hold),
            (PushKind::CompanionReply, None, Tier::Hold),
            (PushKind::NightShift, None, Tier::Hold),
            (PushKind::Focus, None, Tier::Through),
        ];
        for (kind, risk, want) in table {
            assert_eq!(
                tier_of_push(kind, risk),
                want,
                "{} risk {:?}",
                kind.as_str(),
                risk
            );
        }
        for kind in PUSH_ALL {
            assert!(
                table.iter().any(|(k, _, _)| *k == kind),
                "{} has no row",
                kind.as_str()
            );
        }
    }

    #[test]
    fn risk_only_changes_approval_and_waiting_input() {
        for kind in ATTENTION_ALL {
            if kind == AttentionKind::Approval {
                continue;
            }
            let base = tier_of_attention(kind, None);
            for risk in RISKS {
                assert_eq!(tier_of_attention(kind, risk), base, "{}", kind.as_str());
            }
        }
        for kind in PUSH_ALL {
            if kind == PushKind::WaitingInput {
                continue;
            }
            let base = tier_of_push(kind, None);
            for risk in RISKS {
                assert_eq!(tier_of_push(kind, risk), base, "{}", kind.as_str());
            }
        }
    }

    #[test]
    fn high_risk_approval_breaks_through() {
        assert_eq!(
            tier_of_attention(AttentionKind::Approval, Some(Level::High)),
            Tier::Through
        );
        assert_eq!(
            decide(
                tier_of_attention(AttentionKind::Approval, Some(Level::High)),
                &settings(true)
            ),
            Gate::Send
        );
    }

    #[test]
    fn low_medium_and_unrated_approval_is_quiet() {
        for risk in [Some(Level::Low), Some(Level::Medium), None] {
            assert_eq!(
                tier_of_attention(AttentionKind::Approval, risk),
                Tier::Quiet,
                "risk {:?}",
                risk
            );
            assert_eq!(
                decide(
                    tier_of_attention(AttentionKind::Approval, risk),
                    &settings(true)
                ),
                Gate::Quiet
            );
        }
    }

    #[test]
    fn invariant_a_blocking_attention_kinds_are_never_held() {
        for kind in ATTENTION_ALL {
            if lane_of(kind) == Lane::Blocking {
                for risk in RISKS {
                    assert_ne!(
                        tier_of_attention(kind, risk),
                        Tier::Hold,
                        "{}",
                        kind.as_str()
                    );
                }
            }
        }
    }

    #[test]
    fn invariant_b_urgent_push_kinds_are_never_held() {
        for kind in PUSH_ALL {
            if kind.is_urgent() {
                for risk in RISKS {
                    assert_ne!(tier_of_push(kind, risk), Tier::Hold, "{}", kind.as_str());
                }
            }
        }
    }

    #[test]
    fn invariant_c_hold_is_exactly_the_ready_and_finished_set() {
        let mut held: BTreeSet<&'static str> = BTreeSet::new();
        for kind in ATTENTION_ALL {
            for risk in RISKS {
                if tier_of_attention(kind, risk) == Tier::Hold {
                    held.insert(kind.as_str());
                }
            }
        }
        for kind in PUSH_ALL {
            for risk in RISKS {
                if tier_of_push(kind, risk) == Tier::Hold {
                    held.insert(kind.as_str());
                }
            }
        }
        let want: BTreeSet<&'static str> = [
            "review",
            "done",
            "run_finished",
            "run_failed",
            "companion_reply",
            "night_shift",
        ]
        .into_iter()
        .collect();
        assert_eq!(held, want);
    }

    #[test]
    fn invariant_d_through_always_sends() {
        for quiet_blocking in [true, false] {
            for default_minutes in [5, 25, 45, 90, 240] {
                let s = Settings {
                    default_minutes,
                    quiet_blocking,
                };
                assert_eq!(decide(Tier::Through, &s), Gate::Send);
            }
        }
    }

    #[test]
    fn decide_with_quiet_blocking_on() {
        let s = settings(true);
        assert_eq!(decide(Tier::Hold, &s), Gate::Hold);
        assert_eq!(decide(Tier::Quiet, &s), Gate::Quiet);
        assert_eq!(decide(Tier::Through, &s), Gate::Send);
    }

    #[test]
    fn quiet_blocking_off_turns_quiet_into_send() {
        let s = settings(false);
        assert_eq!(decide(Tier::Quiet, &s), Gate::Send);
        // Hold and Through do not change with the switch.
        assert_eq!(decide(Tier::Hold, &s), Gate::Hold);
        assert_eq!(decide(Tier::Through, &s), Gate::Send);
    }

    #[test]
    fn quiet_blocking_off_sends_every_blocking_card_at_full_strength() {
        let s = settings(false);
        for kind in ATTENTION_ALL {
            if lane_of(kind) == Lane::Blocking {
                for risk in RISKS {
                    assert_eq!(
                        decide(tier_of_attention(kind, risk), &s),
                        Gate::Send,
                        "{}",
                        kind.as_str()
                    );
                }
            }
        }
        for kind in PUSH_ALL {
            if kind.is_urgent() {
                for risk in RISKS {
                    assert_eq!(
                        decide(tier_of_push(kind, risk), &s),
                        Gate::Send,
                        "{}",
                        kind.as_str()
                    );
                }
            }
        }
    }

    #[test]
    fn class_of_push_is_some_exactly_for_held_push_kinds() {
        for kind in PUSH_ALL {
            let held = tier_of_push(kind, None) == Tier::Hold;
            assert_eq!(class_of_push(kind).is_some(), held, "{}", kind.as_str());
        }
        assert_eq!(class_of_push(PushKind::RunFinished), Some(Class::RunDone));
        assert_eq!(class_of_push(PushKind::RunFailed), Some(Class::RunFailed));
        assert_eq!(
            class_of_push(PushKind::CompanionReply),
            Some(Class::Companion)
        );
        assert_eq!(class_of_push(PushKind::NightShift), Some(Class::Night));
    }

    /// The non-test source of every `.rs` file in `attention/focus/` (files named `*tests.rs` are skipped).
    fn module_sources() -> Vec<(String, String)> {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src/attention/focus");
        let mut out = Vec::new();
        for entry in std::fs::read_dir(dir).expect("focus folder") {
            let path = entry.expect("entry").path();
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            if !name.ends_with(".rs") || name.ends_with("tests.rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("read source");
            let production = text.split("#[cfg(test)]").next().unwrap_or("").to_string();
            out.push((name, production));
        }
        out.sort();
        out
    }

    #[test]
    fn s9_focus_module_never_names_an_agent() {
        let sources = module_sources();
        assert!(
            sources.iter().any(|(name, _)| name == "policy.rs"),
            "policy.rs must be scanned"
        );
        for (name, text) in &sources {
            let lower = text.to_lowercase();
            for word in ["claude", "codex", "cursor", "opencode", "agy"] {
                assert!(!lower.contains(word), "{name} names an agent: {word}");
            }
        }
    }

    #[test]
    fn s4_focus_module_never_writes_to_a_pane() {
        const FORBIDDEN: [&str; 8] = [
            "write_input",
            "send_keys",
            "press_key",
            "TypingPermit",
            "GatePermit",
            "AnswerPermit",
            "write_all",
            "answer",
        ];
        let sources = module_sources();
        assert!(
            sources.len() >= 2,
            "the focus folder has several source files"
        );
        for (name, text) in &sources {
            let tokens: BTreeSet<&str> = text
                .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .filter(|t| !t.is_empty())
                .collect();
            for word in FORBIDDEN {
                assert!(
                    !tokens.contains(word),
                    "{name} has a pane write or answer path: {word}"
                );
            }
        }
    }

    #[test]
    fn s4_scan_sees_the_real_words_it_forbids() {
        // Guard against the scan going blind: the production text of this file does contain ordinary tokens.
        let sources = module_sources();
        let policy = sources
            .iter()
            .find(|(name, _)| name == "policy.rs")
            .expect("policy.rs")
            .1
            .clone();
        assert!(policy.contains("tier_of_push"));
        assert!(policy.contains("Gate::Hold"));
    }
}
