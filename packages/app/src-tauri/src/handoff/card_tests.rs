//! The Inbox cards of a usage limit, the come-back card and the failure card: their choices and copy,
//! and what answering each one asks for (`card::decision`). Agent names here are fixtures only.

use super::card::{self, Decision};
use super::detect::{LimitEvidence, LimitHit, LimitKind};
use super::keep_going::model::{KeepGoingOffer, Target};
use super::reset::{when_text, FixedClock};
use crate::questions::{NewQuestion, Question, QuestionStore};

const PANE: &str = "pane-a";
const NOW: u64 = 1_700_000_000_000;

fn clock() -> FixedClock {
    FixedClock { now_ms: NOW as i64, offset_secs: 0, zone: None }
}

fn hit(reset_at_ms: Option<u64>, armed: bool) -> LimitHit {
    LimitHit {
        kind: LimitKind::Usage,
        rule_id: "window".into(),
        window: Some("session limit".into()),
        reset_text: reset_at_ms.map(|_| "5pm".to_string()),
        reset_at_ms,
        self_resume_armed: armed,
        menu_open: false,
        evidence: LimitEvidence::Seen,
        stable_ms: 2000,
        screen_sha: "sha".into(),
    }
}

fn target(text: &str) -> Target {
    Target::parse(text).expect("a valid fixture target")
}

fn codex() -> KeepGoingOffer {
    KeepGoingOffer::Offer { target: target("codex"), label: "Codex".into() }
}

fn choices(card: &NewQuestion) -> Vec<(&str, &str)> {
    card.choices.iter().map(|c| (c.id.as_str(), c.label.as_str())).collect()
}

/// Raise `card` in a fresh store and answer `choice`. `decision` reads the resolved question.
fn answered(card: NewQuestion, choice: &str) -> Question {
    let store = QuestionStore::new();
    let created = store.create(card, NOW).expect("the card is valid");
    store.answer(&created.id, Some(choice), None, "desktop", NOW + 1).expect("the choice is on the card").question
}

#[test]
fn an_offer_puts_continue_first_then_remind_and_stop() {
    let reset = NOW + 3 * 3_600_000;
    let at = when_text(reset as i64, &clock());
    let card = card::limit_card(&hit(Some(reset), false), "Claude Code", PANE, true, &clock(), &codex());
    assert_eq!(card.question, format!("Claude Code ran out of usage. It can continue at {at}."));
    let remind = format!("Remind me at {at}");
    assert_eq!(
        choices(&card),
        vec![("continue:codex", "Continue on Codex"), ("remind", remind.as_str()), ("stop", "Stop here")]
    );
    assert_eq!(card.pane_id.as_deref(), Some(PANE));
    assert!(!card.allow_free_text.unwrap_or(true));
    assert!(card.limit.as_ref().unwrap().has_run);
}

#[test]
fn an_offer_says_where_the_work_goes_and_that_the_old_agent_stops_first() {
    let card = card::limit_card(&hit(None, false), "Claude Code", PANE, true, &clock(), &codex());
    assert_eq!(
        card.context.as_deref(),
        Some("Codex gets a short brief and works in the same folder. Claude Code in this pane is stopped first.")
    );
}

#[test]
fn an_offer_with_an_account_names_the_account_in_its_choice_id() {
    let offer = KeepGoingOffer::Offer { target: target("codex@work"), label: "Codex · Work".into() };
    let card = card::limit_card(&hit(None, false), "Claude Code", PANE, true, &clock(), &offer);
    assert_eq!(choices(&card)[0], ("continue:codex@work", "Continue on Codex · Work"));
}

#[test]
fn an_armed_self_resume_is_said_before_the_offer_and_that_the_offer_stops_it() {
    let card = card::limit_card(&hit(None, true), "Claude Code", PANE, true, &clock(), &codex());
    assert_eq!(
        card.context.as_deref(),
        Some(
            "Claude Code is set to continue on its own. Continuing on Codex stops that. \
             Codex gets a short brief and works in the same folder. Claude Code in this pane is stopped first."
        )
    );
    assert!(card.limit.as_ref().unwrap().self_resume_armed);
}

#[test]
fn no_offer_keeps_remind_and_stop_and_says_why_nothing_is_offered() {
    let none = KeepGoingOffer::None { why: "no other agent is set up in this folder.".into() };
    let card = card::limit_card(&hit(None, false), "Claude Code", PANE, true, &clock(), &none);
    let got = choices(&card);
    assert_eq!(got.iter().map(|(id, _)| *id).collect::<Vec<_>>(), vec!["remind", "stop"]);
    assert_eq!(
        card.context.as_deref(),
        Some("No other agent can take over now: no other agent is set up in this folder. Pick one in Settings > Agents > Keep going.")
    );
}

#[test]
fn a_card_that_is_not_here_has_no_hand_over_line_and_no_other_choice() {
    let card = card::limit_card(&hit(Some(NOW + 3_600_000), false), "Claude Code", PANE, true, &clock(), &KeepGoingOffer::NotHere);
    assert!(card.context.is_none(), "{:?}", card.context);
    assert!(!card.context.clone().unwrap_or_default().contains("later update"));
    assert_eq!(card.choices.len(), 2);
    assert!(card.choices.iter().all(|c| !c.id.starts_with("continue:")));
}

#[test]
fn a_card_that_is_not_here_keeps_only_the_armed_line() {
    let card = card::limit_card(&hit(None, true), "Claude Code", PANE, true, &clock(), &KeepGoingOffer::NotHere);
    assert_eq!(card.context.as_deref(), Some("Claude Code is set to continue on its own."));
}

#[test]
fn without_a_printed_reset_the_card_says_so_and_reminds_in_an_hour() {
    let card = card::limit_card(&hit(None, false), "Claude Code", PANE, true, &clock(), &KeepGoingOffer::NotHere);
    assert_eq!(card.question, "Claude Code ran out of usage. It didn't say when it can continue.");
    assert_eq!(choices(&card), vec![("remind", "Remind me in 1 hour"), ("stop", "Stop here")]);
}

#[test]
fn a_limit_in_a_pane_without_a_run_offers_dismiss_not_stop() {
    let card = card::limit_card(&hit(None, false), "Claude Code", PANE, false, &clock(), &codex());
    assert_eq!(choices(&card)[2], ("stop", "Dismiss"));
    assert!(!card.limit.as_ref().unwrap().has_run);
}

#[test]
fn the_come_back_card_asks_on_the_new_pane_and_names_both_agents() {
    let card = card::come_back_card("Codex", "Claude Code", PANE, "Make the cart total round to cents");
    assert_eq!(card.pane_id.as_deref(), Some(PANE));
    assert_eq!(card.agent.as_deref(), Some("Codex"));
    assert_eq!(
        card.question,
        "Claude Code can work again. Move \"Make the cart total round to cents\" back from Codex?"
    );
    assert_eq!(choices(&card), vec![("back", "Move back to Claude Code"), ("stay", "Stay on Codex")]);
    assert_eq!(card.limit.as_ref().unwrap().kind, card::KIND_COME_BACK);
    assert!(!card.allow_free_text.unwrap_or(true));
}

#[test]
fn the_failure_card_offers_try_again_only_when_a_retry_can_help() {
    let to = target("codex");
    let card = card::keep_going_failed_card(
        "Claude Code",
        "Codex",
        PANE,
        "Claude Code didn't stop, so Codex wasn't started. Close its terminal, then try again.",
        Some(&to),
    );
    assert_eq!(choices(&card), vec![("retry:codex", "Try Codex again"), ("ok", "Got it")]);
    assert_eq!(card.limit.as_ref().unwrap().kind, card::KIND_KEEP_GOING_FAILED);
    assert_eq!(card.question, "Claude Code didn't stop, so Codex wasn't started. Close its terminal, then try again.");

    let no_retry = card::keep_going_failed_card("Claude Code", "Codex", PANE, "Codex couldn't start.", None);
    assert_eq!(choices(&no_retry), vec![("ok", "Got it")]);
}

#[test]
fn a_continue_answer_asks_for_that_hand_over() {
    let card = card::limit_card(&hit(None, false), "Claude Code", PANE, true, &clock(), &codex());
    assert_eq!(
        card::decision(&answered(card, "continue:codex"), NOW),
        Decision::Continue { pane_id: PANE.into(), target: target("codex") }
    );
    let work = card::limit_card(&hit(None, false), "Claude Code", PANE, true, &clock(), &KeepGoingOffer::Offer {
        target: target("codex@work"),
        label: "Codex · Work".into(),
    });
    assert_eq!(
        card::decision(&answered(work, "continue:codex@work"), NOW),
        Decision::Continue { pane_id: PANE.into(), target: target("codex@work") }
    );
}

#[test]
fn a_continue_answer_with_a_bad_target_asks_for_nothing() {
    // A padded id is not tested here: the store trims choice ids on create, so it never reaches
    // `decision`. A leading space survives the store and must still be refused by the strict key check.
    for bad in ["continue:", "continue:../x", "continue:a@b@c", "continue: codex", "continue:x y"] {
        let mut card = card::limit_card(&hit(None, false), "Claude Code", PANE, true, &clock(), &codex());
        card.choices[0].id = bad.to_string();
        assert_eq!(card::decision(&answered(card, bad), NOW), Decision::Nothing, "{bad}");
    }
}

#[test]
fn remind_and_stop_keep_their_meaning() {
    let reset = NOW + 3_600_000;
    let card = card::limit_card(&hit(Some(reset), false), "Claude Code", PANE, true, &clock(), &codex());
    assert_eq!(
        card::decision(&answered(card, "remind"), NOW),
        Decision::Remind { pane_id: PANE.into(), agent_label: "Claude Code".into(), window: Some("session limit".into()), due_ms: reset }
    );
    let card = card::limit_card(&hit(None, false), "Claude Code", PANE, true, &clock(), &codex());
    assert_eq!(card::decision(&answered(card, "stop"), NOW), Decision::StopRun { pane_id: PANE.into() });
}

#[test]
fn stop_without_a_run_asks_for_nothing() {
    let card = card::limit_card(&hit(None, false), "Claude Code", PANE, false, &clock(), &KeepGoingOffer::NotHere);
    assert_eq!(card::decision(&answered(card, "stop"), NOW), Decision::Nothing);
}

#[test]
fn the_come_back_answers_move_back_or_stay() {
    assert_eq!(
        card::decision(&answered(card::come_back_card("Codex", "Claude Code", PANE, "goal"), "back"), NOW),
        Decision::ComeBack { pane_id: PANE.into() }
    );
    assert_eq!(
        card::decision(&answered(card::come_back_card("Codex", "Claude Code", PANE, "goal"), "stay"), NOW),
        Decision::Stay { pane_id: PANE.into() }
    );
}

#[test]
fn a_hand_over_choice_on_a_come_back_card_asks_for_nothing() {
    let mut card = card::come_back_card("Codex", "Claude Code", PANE, "goal");
    card.choices.push(crate::questions::Choice { id: "continue:codex".into(), label: "x".into() });
    assert_eq!(card::decision(&answered(card, "continue:codex"), NOW), Decision::Nothing);
}

#[test]
fn the_retry_answer_asks_for_the_same_hand_over_and_a_bad_one_asks_for_nothing() {
    let card = card::keep_going_failed_card("Claude Code", "Codex", PANE, "failed", Some(&target("codex")));
    assert_eq!(
        card::decision(&answered(card, "retry:codex"), NOW),
        Decision::Retry { pane_id: PANE.into(), target: target("codex") }
    );
    let mut card = card::keep_going_failed_card("Claude Code", "Codex", PANE, "failed", Some(&target("codex")));
    card.choices[0].id = "retry:../x".into();
    assert_eq!(card::decision(&answered(card, "retry:../x"), NOW), Decision::Nothing);
    let card = card::keep_going_failed_card("Claude Code", "Codex", PANE, "failed", None);
    assert_eq!(card::decision(&answered(card, "ok"), NOW), Decision::Nothing);
}

#[test]
fn a_reminder_card_still_only_says_got_it() {
    let card = card::reminder_card("Claude Code", PANE, None);
    assert_eq!(card::decision(&answered(card, "ok"), NOW), Decision::Nothing);
}
