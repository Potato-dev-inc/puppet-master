//! The screen lease: the cases of `packages/potato-brain/test/lifted-screen-lease.test.ts` (the rules must
//! agree; the first test here repeats them), then the sandbox's use of them: expiry, run binding, person take-over, "Needs you".

use super::lease::*;

const T0: i64 = 1_000_000;

#[test]
fn the_brain_test_cases_give_the_same_answers() {
    assert_eq!(lease_id("boot:abc", 3), "boot:abc:3");
    assert_eq!(parse_lease_id("boot:abc:3"), ParsedId { owner: "boot:abc".into(), fence: 3 });
    assert_eq!(parse_lease_id("run-1"), ParsedId { owner: "run-1".into(), fence: 0 });
    for bad in ["run:", "run:-1", "run:1.5", "run:1e3", "run: 3", "run:99999999999999999999", ":3"] {
        assert_eq!(parse_lease_id(bad), ParsedId { owner: bad.into(), fence: 0 }, "{bad}");
    }
    assert_eq!(parse_lease_id("run:007"), ParsedId { owner: "run".into(), fence: 7 });
    assert!(can_take(Some("run-1:1"), Some("run-2:2")));
    assert!(can_take(Some("run-1:1"), Some("run-1:8")));
    assert!(!can_take(Some("run-2:2"), Some("run-1:1")));
    assert!(!can_take(Some("run-1:8"), Some("run-1:1")));
    assert!(!can_take(Some("run-2:2"), None));
    assert!(!can_take(Some("run-1:2"), Some("run-2:2")));
    assert!(!can_take(Some("run-1:4"), Some("run-2:4")));
    assert!(can_take(None, Some("run-1:1")));
    assert!(can_take(Some("run-1:1"), Some("run-1:1")));
    assert!(!can_take(None, None));
    assert!(!can_take(Some("run-1:1"), Some("")));
    assert!(can_release(Some("run-1:1"), Some("run-1:8")));
    assert!(can_release(Some("run-1:1"), Some("run-1:1")));
    assert!(!can_release(Some("run-2:2"), Some("run-1:1")));
    assert!(!can_release(Some("run-1:1"), Some("run-2:5")));
    assert!(lease_held(Some(1_000), 999));
    assert!(!lease_held(Some(1_000), 1_000));
    assert!(!lease_held(None, 0));
    // an expired holder's fence is still the floor for a take
    assert!(!can_take(Some("run-2:5"), Some("run-1:4")));
    assert!(can_take(Some("run-2:5"), Some("run-3:6")));
}

#[test]
fn ids_parse_like_the_brain_does() {
    let p = |id: &str| parse_lease_id(id);
    assert_eq!(p("run-1:3"), ParsedId { owner: "run-1".into(), fence: 3 });
    // the owner may hold colons: the fence is after the LAST one
    assert_eq!(p("boot:abc:12"), ParsedId { owner: "boot:abc".into(), fence: 12 });
    // no colon, a leading colon, an empty or non-digit fence: the whole id is the owner, fence 0
    for bad in ["run-1", ":5", "run-1:", "run-1:x", "run-1: 3", "run-1:3e0", "run-1:-3", "run-1:+3", "run-1:3.0", "run-1:3\n"] {
        assert_eq!(p(bad), ParsedId { owner: bad.into(), fence: 0 }, "{bad:?}");
    }
    // leading zeros are plain decimal; the largest safe integer is allowed, one more is not
    assert_eq!(p("a:007").fence, 7);
    assert_eq!(p("a:9007199254740991").fence, 9_007_199_254_740_991);
    assert_eq!(p("a:9007199254740992"), ParsedId { owner: "a:9007199254740992".into(), fence: 0 });
    assert_eq!(p("a:99999999999999999999999"), ParsedId { owner: "a:99999999999999999999999".into(), fence: 0 });
    assert_eq!(lease_id("run-1", 4), "run-1:4");
    assert_eq!(p(&lease_id("a:b", 9)), ParsedId { owner: "a:b".into(), fence: 9 });
}

#[test]
fn take_follows_rule_2() {
    // no id is always refused
    assert!(!can_take(None, None));
    assert!(!can_take(Some("a:1"), None));
    assert!(!can_take(None, Some("")));
    // nobody holds
    assert!(can_take(None, Some("a:1")));
    assert!(can_take(Some(""), Some("a:1")));
    // the holder's own id is a renewal
    assert!(can_take(Some("a:5"), Some("a:5")));
    // a strictly higher fence wins, whatever the owner
    assert!(can_take(Some("a:5"), Some("b:6")));
    assert!(can_take(Some("a:5"), Some("a:6")));
    // equal or lower is a delayed older run
    assert!(!can_take(Some("a:5"), Some("b:5")));
    assert!(!can_take(Some("a:5"), Some("b:4")));
    assert!(!can_take(Some("a:5"), Some("a:4")));
    // a holder with a bad id has fence 0
    assert!(can_take(Some("weird"), Some("a:1")));
    assert!(!can_take(Some("weird"), Some("a:x")));
}

#[test]
fn release_follows_rule_3() {
    assert!(can_release(None, Some("a:1")));
    assert!(can_release(Some("a:1"), None));
    assert!(can_release(Some("a:1"), Some("a:1")));
    // the same owner with a fence that is not behind
    assert!(can_release(Some("a:3"), Some("a:4")));
    assert!(can_release(Some("a:3"), Some("a:3")));
    // an older finalizer of the same owner cannot release a newer lease
    assert!(!can_release(Some("a:3"), Some("a:2")));
    // another owner cannot release, whatever its fence
    assert!(!can_release(Some("a:3"), Some("b:9")));
}

#[test]
fn expiry_follows_rule_4() {
    assert!(lease_held(Some(100), 99));
    assert!(!lease_held(Some(100), 100));
    assert!(!lease_held(Some(100), 101));
    assert!(!lease_held(None, 0));
}

#[test]
fn a_bot_run_takes_the_screen_and_keeps_it_by_using_it() {
    let mut screen = ScreenLease::new();
    assert_eq!(screen.banner(T0), Banner::Free);
    let id = screen.take_for_bot("fish", "run-1", T0).unwrap();
    assert_eq!(id, "run-1:1");
    assert_eq!(screen.banner(T0), Banner::BotUsing { bot: "fish".into() });
    assert!(screen.check(&id, T0 + 1).is_ok());
    // using it keeps it for 15 more minutes from the use
    assert!(screen.renew(&id, T0 + LEASE_TTL_MS - 1).is_ok());
    assert!(screen.check(&id, T0 + 2 * LEASE_TTL_MS - 3).is_ok());
    // a take by the same run gives the same lease back, renewed, no new fence
    assert_eq!(screen.take_for_bot("fish", "run-1", T0 + 10).unwrap(), id);
    assert_eq!(screen.highest_fence(), 1);
}

#[test]
fn a_lease_ends_after_15_minutes_without_use_and_the_action_is_lease_lost() {
    let mut screen = ScreenLease::new();
    let id = screen.take_for_bot("fish", "run-1", T0).unwrap();
    assert!(screen.check(&id, T0 + LEASE_TTL_MS - 1).is_ok());
    let err = screen.check(&id, T0 + LEASE_TTL_MS).unwrap_err();
    assert_eq!(err, LeaseError::LeaseLost);
    assert_eq!(err.code(), "LEASE_LOST");
    assert_eq!(screen.renew(&id, T0 + LEASE_TTL_MS), Err(LeaseError::LeaseLost));
    // an expired lease blocks nobody
    assert_eq!(screen.banner(T0 + LEASE_TTL_MS), Banner::Free);
    let events = screen.tick(T0 + LEASE_TTL_MS);
    assert_eq!(events, vec![LeaseEvent::LeaseExpired { lease_id: id.clone(), holder: Holder::Bot("fish".into()) }]);
    assert!(screen.tick(T0 + LEASE_TTL_MS + 1).is_empty());
    // but its fence is not forgotten: the next run gets a higher one, the old id never works again
    let next = screen.take_for_bot("fish", "run-2", T0 + LEASE_TTL_MS + 5).unwrap();
    assert_eq!(next, "run-2:2");
    assert_eq!(screen.check(&id, T0 + LEASE_TTL_MS + 6), Err(LeaseError::LeaseLost));
    assert!(!can_take(Some(&next), Some(&id)), "an old execution cannot come back");
}

#[test]
fn a_second_bot_run_cannot_take_a_live_lease_but_can_after_it_expires_or_ends() {
    let mut screen = ScreenLease::new();
    let first = screen.take_for_bot("fish", "run-1", T0).unwrap();
    assert_eq!(screen.take_for_bot("rival", "run-2", T0 + 5), Err(LeaseError::BusyWithBot("fish".into())));
    assert_eq!(screen.take_for_bot("fish", "run-3", T0 + 5), Err(LeaseError::BusyWithBot("fish".into())));
    assert!(screen.check(&first, T0 + 6).is_ok(), "the working run keeps its lease");
    assert_eq!(screen.highest_fence(), 1, "no new fence was made");
    assert!(LeaseError::BusyWithBot("fish".into()).message().contains("busy with fish"));
    // when its lease ran out, the next run takes over
    assert!(screen.take_for_bot("rival", "run-2", T0 + LEASE_TTL_MS + 1).is_ok());
    // or when the run ended
    screen.end_run("run-2", T0 + LEASE_TTL_MS + 2);
    assert!(screen.take_for_bot("fish", "run-4", T0 + LEASE_TTL_MS + 3).is_ok());
}

#[test]
fn an_older_run_whose_lease_ran_out_is_lease_lost_when_a_newer_run_takes_over() {
    let mut screen = ScreenLease::new();
    let old = screen.take_for_bot("fish", "run-1", T0).unwrap();
    let new = screen.take_for_bot("fish", "run-2", T0 + LEASE_TTL_MS + 5).unwrap();
    let (old_check, new_check) = (T0 + LEASE_TTL_MS + 6, T0 + LEASE_TTL_MS + 6);
    assert_eq!((old.as_str(), new.as_str()), ("run-1:1", "run-2:2"));
    assert_eq!(screen.check(&old, old_check), Err(LeaseError::LeaseLost));
    assert!(screen.check(&new, new_check).is_ok());
    // the older run's finalizer cannot release the newer run's lease
    assert!(!screen.release(&old));
    assert!(screen.check(&new, new_check + 1).is_ok());
    // the owner of the live lease can
    assert!(screen.release(&new));
    assert_eq!(screen.banner(new_check + 2), Banner::Free);
    // releasing when nobody holds is fine
    assert!(screen.release("run-9:1"));
}

#[test]
fn the_person_takes_over_with_a_higher_fence_and_the_bot_loses_its_lease() {
    let mut screen = ScreenLease::new();
    let bot = screen.take_for_bot("fish", "run-1", T0).unwrap();
    let (person, displaced) = screen.take_over(T0 + 10);
    assert_eq!(person, "person:2");
    assert_eq!(displaced, Some(Displaced { bot: "fish".into(), run_id: "run-1".into() }));
    assert!(parse_lease_id(&person).fence > parse_lease_id(&bot).fence);
    assert_eq!(screen.banner(T0 + 11), Banner::PersonHasControl);
    assert_eq!(screen.check(&bot, T0 + 11), Err(LeaseError::LeaseLost));
    assert_eq!(screen.renew(&bot, T0 + 11), Err(LeaseError::LeaseLost));
    // a bot never takes the screen from the person
    assert_eq!(screen.take_for_bot("fish", "run-1", T0 + 12), Err(LeaseError::PersonHasControl));
    assert_eq!(screen.take_for_bot("rival", "run-7", T0 + 12), Err(LeaseError::PersonHasControl));
    // taking over again just renews with a new fence
    let (again, again_displaced) = screen.take_over(T0 + 13);
    assert_eq!(again, "person:3");
    assert_eq!(again_displaced, displaced, "the displaced run is remembered across the person's own renewals");
}

#[test]
fn handing_back_frees_the_screen_and_names_the_run_that_was_using_it() {
    let mut screen = ScreenLease::new();
    screen.take_for_bot("fish", "run-1", T0).unwrap();
    let (_, _) = screen.take_over(T0 + 10);
    let back = screen.hand_back(T0 + 20);
    assert_eq!(back, Some(Displaced { bot: "fish".into(), run_id: "run-1".into() }));
    assert_eq!(screen.banner(T0 + 21), Banner::Free);
    // the run takes a new lease, with a higher fence than the person's
    let again = screen.take_for_bot("fish", "run-1", T0 + 22).unwrap();
    assert_eq!(again, "run-1:3");
    // handing back when the person has nothing is a no-op
    assert_eq!(screen.hand_back(T0 + 23), None);
}

#[test]
fn the_person_lease_also_expires() {
    let mut screen = ScreenLease::new();
    let (person, _) = screen.take_over(T0);
    assert!(screen.check(&person, T0 + LEASE_TTL_MS - 1).is_ok());
    assert_eq!(screen.banner(T0 + LEASE_TTL_MS), Banner::Free);
    assert!(screen.take_for_bot("fish", "run-1", T0 + LEASE_TTL_MS).is_ok());
}

#[test]
fn needs_you_is_a_card_that_lasts_30_minutes_and_keeps_the_lease_alive() {
    let mut screen = ScreenLease::new();
    let id = screen.take_for_bot("fish", "run-1", T0).unwrap();
    let card = screen.ask_for_help(&id, "The shop wants a code from your phone.\u{7}", T0 + 1000).unwrap();
    assert_eq!(card.expires_ms, T0 + 1000 + NEEDS_YOU_TTL_MS);
    assert_eq!(card.reason, "The shop wants a code from your phone.");
    assert_eq!(screen.banner(T0 + 2000), Banner::NeedsYou { bot: "fish".into(), card_id: card.card_id.clone() });
    // the lease outlives its 15 minutes while the card is open
    assert!(screen.check(&id, T0 + LEASE_TTL_MS + 5000).is_ok());
    // a stale lease cannot ask
    assert_eq!(screen.ask_for_help("run-0:0", "x", T0 + 2000), Err(LeaseError::LeaseLost));
    // the person's take-over answers the card
    let (_, displaced) = screen.take_over(T0 + 3000);
    assert_eq!(displaced.unwrap().run_id, "run-1");
    assert!(screen.card(T0 + 3001).is_none());
    assert_eq!(screen.banner(T0 + 3001), Banner::PersonHasControl);
}

#[test]
fn skip_answers_the_card_and_the_bot_keeps_its_lease() {
    let mut screen = ScreenLease::new();
    let id = screen.take_for_bot("fish", "run-1", T0).unwrap();
    let card = screen.ask_for_help(&id, "stuck", T0).unwrap();
    assert_eq!(screen.skip("needs-99"), Err(LeaseError::NoSuchCard));
    assert!(screen.card(T0 + 1).is_some(), "a wrong card id leaves the card alone");
    let skipped = screen.skip(&card.card_id).unwrap();
    assert_eq!(skipped.run_id, "run-1");
    assert!(screen.check(&id, T0 + 2).is_ok());
    assert_eq!(screen.banner(T0 + 2), Banner::BotUsing { bot: "fish".into() });
    assert_eq!(screen.skip(&card.card_id), Err(LeaseError::NoSuchCard));
}

#[test]
fn an_unanswered_card_expires_and_the_clock_says_so() {
    let mut screen = ScreenLease::new();
    let id = screen.take_for_bot("fish", "run-1", T0).unwrap();
    let card = screen.ask_for_help(&id, "stuck", T0).unwrap();
    assert!(screen.tick(T0 + NEEDS_YOU_TTL_MS - 1).is_empty());
    let events = screen.tick(T0 + NEEDS_YOU_TTL_MS);
    assert!(events.contains(&LeaseEvent::NeedsYouExpired { card_id: card.card_id, bot: "fish".into(), run_id: "run-1".into() }));
    assert!(screen.card(T0 + NEEDS_YOU_TTL_MS).is_none());
}

#[test]
fn a_run_that_ends_takes_its_lease_with_it_and_its_card_too_unless_the_card_is_open_and_waiting() {
    let mut screen = ScreenLease::new();
    let id = screen.take_for_bot("fish", "run-1", T0).unwrap();
    screen.end_run("run-2", T0 + 1);
    assert!(screen.check(&id, T0 + 1).is_ok(), "another run's end changes nothing");
    screen.end_run("run-1", T0 + 2);
    assert_eq!(screen.check(&id, T0 + 2), Err(LeaseError::LeaseLost));
    // a card that is already past its time goes with its run
    let id = screen.take_for_bot("fish", "run-3", T0 + 3).unwrap();
    screen.ask_for_help(&id, "stuck", T0 + 3).unwrap();
    screen.end_run("run-3", T0 + 3 + NEEDS_YOU_TTL_MS);
    assert!(screen.card(T0 + 4 + NEEDS_YOU_TTL_MS).is_none());
    // the person's lease is not a run's: end_run does not drop it
    let (person, _) = screen.take_over(T0 + 3);
    screen.end_run("person", T0 + 3);
    assert!(screen.check(&person, T0 + 4).is_ok());
}

#[test]
fn a_run_that_ends_its_turn_with_its_card_open_keeps_the_card_and_the_lease_for_the_person() {
    let mut screen = ScreenLease::new();
    let id = screen.take_for_bot("fish", "run-1", T0).unwrap();
    let card = screen.ask_for_help(&id, "A code is needed", T0).unwrap();
    screen.end_run("run-1", T0 + 1000);
    assert!(screen.check(&id, T0 + 1001).is_ok(), "the lease waits with the card");
    assert_eq!(screen.banner(T0 + 1001), Banner::NeedsYou { bot: "fish".into(), card_id: card.card_id.clone() });
    // it is still the 30 minutes of the card, not more: the clock ends both
    screen.tick(T0 + NEEDS_YOU_TTL_MS);
    assert!(screen.card(T0 + NEEDS_YOU_TTL_MS).is_none());
    assert_eq!(screen.banner(T0 + NEEDS_YOU_TTL_MS), Banner::Free);
}

#[test]
fn the_person_can_take_over_from_a_parked_card_and_the_run_is_named_as_displaced() {
    let mut screen = ScreenLease::new();
    let id = screen.take_for_bot("fish", "run-1", T0).unwrap();
    screen.ask_for_help(&id, "stuck", T0).unwrap();
    screen.end_run("run-1", T0 + 1);
    let (_, displaced) = screen.take_over(T0 + 2);
    assert_eq!(displaced, Some(Displaced { bot: "fish".into(), run_id: "run-1".into() }));
    assert!(screen.card(T0 + 3).is_none(), "taking over answers the card");
    // handing back gives the screen to nobody; the bot takes a new one in its next turn
    screen.hand_back(T0 + 4);
    assert!(screen.take_for_bot("fish", "run-1", T0 + 5).is_ok());
}

#[test]
fn skip_on_a_parked_card_ends_the_waiting_and_a_later_end_of_run_takes_the_lease() {
    let mut screen = ScreenLease::new();
    let id = screen.take_for_bot("fish", "run-1", T0).unwrap();
    let card = screen.ask_for_help(&id, "stuck", T0).unwrap();
    screen.end_run("run-1", T0 + 1);
    screen.skip(&card.card_id).unwrap();
    assert!(screen.card(T0 + 2).is_none());
    screen.end_run("run-1", T0 + 3);
    assert_eq!(screen.check(&id, T0 + 4), Err(LeaseError::LeaseLost), "no card, so the end of the run takes the lease");
}

#[test]
fn the_same_run_working_again_in_its_next_turn_is_not_waiting_any_more() {
    let mut screen = ScreenLease::new();
    let id = screen.take_for_bot("fish", "run-1", T0).unwrap();
    screen.ask_for_help(&id, "stuck", T0).unwrap();
    // the same turn goes on after the ask: the card stays so the person can see it
    assert_eq!(screen.take_for_bot("fish", "run-1", T0 + 1).unwrap(), id);
    assert!(screen.card(T0 + 2).is_some());
    // the turn ends, the person is asked; the next turn starts working: the card goes
    screen.end_run("run-1", T0 + 3);
    assert!(screen.card(T0 + 4).is_some());
    assert_eq!(screen.take_for_bot("fish", "run-1", T0 + 5).unwrap(), id, "the same lease is renewed");
    assert!(screen.card(T0 + 6).is_none());
    assert_eq!(screen.banner(T0 + 6), Banner::BotUsing { bot: "fish".into() });
}

#[test]
fn a_new_run_drops_the_card_of_an_older_one() {
    let mut screen = ScreenLease::new();
    let id = screen.take_for_bot("fish", "run-1", T0).unwrap();
    screen.ask_for_help(&id, "stuck", T0).unwrap();
    // run-1 is parked on its card: another run is turned away and the card stays
    assert!(screen.take_for_bot("fish", "run-2", T0 + 5).is_err());
    assert!(screen.card(T0 + 6).is_some());
    // once the lease and the card ran out, a new run starts clean
    screen.take_for_bot("fish", "run-2", T0 + NEEDS_YOU_TTL_MS + 5).unwrap();
    assert!(screen.card(T0 + NEEDS_YOU_TTL_MS + 6).is_none());
}

#[test]
fn only_the_bot_holding_the_lease_can_ask() {
    let mut screen = ScreenLease::new();
    let (person, _) = screen.take_over(T0);
    assert_eq!(screen.ask_for_help(&person, "x", T0 + 1), Err(LeaseError::NotTheHolder));
}

#[test]
fn error_codes_are_wire_words() {
    assert_eq!(LeaseError::LeaseLost.code(), LEASE_LOST);
    assert_eq!(LEASE_LOST, "LEASE_LOST");
    for error in [LeaseError::PersonHasControl, LeaseError::BusyWithBot("x".into()), LeaseError::NoSuchCard, LeaseError::NotTheHolder] {
        assert!(error.code().starts_with("LEASE_"));
    }
}
