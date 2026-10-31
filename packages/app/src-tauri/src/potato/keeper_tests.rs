//! The keeper, on nothing: a clock that is a number, and no process.

use super::keeper::*;

const S: i64 = 1_000;

/// A keeper that was asked for, started, and said hello.
fn ready(now: i64) -> Keeper {
    let mut k = Keeper::new();
    k.want_run(now);
    assert_eq!(k.step(now), vec![Action::Spawn]);
    k.spawned(now);
    k.hello(now + 100);
    assert_eq!(k.phase(), &Phase::Ready);
    k
}

#[test]
fn nothing_starts_until_a_message_asks() {
    let mut k = Keeper::new();
    assert!(k.step(0).is_empty());
    assert!(k.step(10 * 60 * S).is_empty());
    assert_eq!(k.phase(), &Phase::Off);
}

#[test]
fn a_message_starts_it_once_and_hello_makes_it_ready() {
    let mut k = Keeper::new();
    k.want_run(0);
    assert_eq!(k.step(0), vec![Action::Spawn]);
    k.spawned(0);
    assert!(matches!(k.phase(), Phase::Starting { .. }));
    // asked again while starting: no second start
    assert!(k.step(1_000).is_empty());
    k.hello(2_000);
    assert_eq!(k.phase(), &Phase::Ready);
    assert!(k.step(3_000).is_empty());
}

#[test]
fn no_hello_in_10_seconds_kills_it_and_counts_a_crash() {
    let mut k = Keeper::new();
    k.want_run(0);
    k.step(0);
    k.spawned(0);
    assert!(k.step(HELLO_LIMIT_MS - 1).is_empty());
    assert_eq!(k.step(HELLO_LIMIT_MS), vec![Action::Kill]);
    assert_eq!(k.crashes(), 1);
    assert!(matches!(k.phase(), Phase::Waiting { until_ms } if *until_ms == HELLO_LIMIT_MS + BACKOFF_FIRST_MS));
    assert!(!k.is_alive());
}

#[test]
fn a_crash_waits_1_2_4_8_seconds_and_the_fifth_stops_it_for_good() {
    assert_eq!([backoff_ms(0), backoff_ms(1), backoff_ms(2), backoff_ms(3)], [1_000, 2_000, 4_000, 8_000]);
    assert_eq!(backoff_ms(10), BACKOFF_MAX_MS, "never more than 60 seconds");
    let mut k = ready(0);
    let mut now = 1_000;
    for (n, wait) in [1_000, 2_000, 4_000, 8_000].into_iter().enumerate() {
        assert!(k.exited(now, false).is_empty(), "crash {}", n + 1);
        // it waits out the backoff
        assert!(k.step(now + wait - 1).is_empty());
        assert_eq!(k.step(now + wait), vec![Action::Spawn]);
        k.spawned(now + wait);
        k.hello(now + wait + 50);
        now += wait + 100;
    }
    // the fifth crash inside 10 minutes
    assert_eq!(k.exited(now, false), vec![Action::RaiseStopped]);
    assert!(k.is_stopped());
    assert!(k.step(now + 60 * S).is_empty(), "stopped is stopped");
    // a message does not start it either
    k.want_run(now + 61 * S);
    assert!(k.step(now + 61 * S).is_empty());
    // only the person's "Start again"
    k.retry(now + 62 * S);
    assert_eq!(k.crashes(), 0);
    assert_eq!(k.step(now + 62 * S), vec![Action::Spawn]);
}

#[test]
fn crashes_older_than_10_minutes_do_not_count() {
    let mut k = ready(0);
    for i in 0..4 {
        k.exited(i * 70 * S, false);
        k.step(i * 70 * S + 60 * S);
        k.spawned(i * 70 * S + 60 * S);
        k.hello(i * 70 * S + 61 * S);
    }
    // 4 crashes spread over 3.5 minutes; far later, a fifth is only the first of a new window
    let later = 20 * 60 * S;
    k.step(later);
    assert_eq!(k.crashes(), 0);
    assert!(k.exited(later, false).is_empty());
    assert!(!k.is_stopped());
}

#[test]
fn thirty_minutes_without_a_word_stops_it_without_a_crash() {
    let mut k = ready(0);
    // `ready` says hello at 100: that is the last word
    assert!(k.step(IDLE_LIMIT_MS + 99).is_empty());
    assert_eq!(k.step(IDLE_LIMIT_MS + 100), vec![Action::Shutdown]);
    // it leaves because it was asked: not a crash, and nothing restarts it
    assert!(k.exited(IDLE_LIMIT_MS + 500, true).is_empty());
    assert_eq!(k.crashes(), 0);
    assert_eq!(k.phase(), &Phase::Off);
    assert!(k.step(IDLE_LIMIT_MS + 1_000).is_empty());
    // the next message starts it again
    k.want_run(IDLE_LIMIT_MS + 2_000);
    assert_eq!(k.step(IDLE_LIMIT_MS + 2_000), vec![Action::Spawn]);
}

#[test]
fn activity_pushes_the_idle_stop_back() {
    let mut k = ready(0);
    k.touch(IDLE_LIMIT_MS - S);
    assert!(k.step(IDLE_LIMIT_MS).is_empty());
    assert_eq!(k.step(IDLE_LIMIT_MS - S + IDLE_LIMIT_MS), vec![Action::Shutdown]);
}

#[test]
fn a_stop_the_person_asked_for_is_never_a_crash_and_never_restarted() {
    let mut k = ready(0);
    k.person_stop();
    assert_eq!(k.step(1_000), vec![Action::Shutdown]);
    assert!(k.exited(2_000, true).is_empty());
    assert_eq!(k.crashes(), 0);
    assert!(k.step(3_000).is_empty());
    // even a process that dies on its own after the person stopped it is no crash
    let mut k = ready(0);
    k.person_stop();
    assert!(k.exited(1_000, false).is_empty());
    assert_eq!(k.crashes(), 0);
    assert_eq!(k.phase(), &Phase::Off);
}

#[test]
fn a_start_that_fails_counts_as_a_crash_and_waits() {
    let mut k = Keeper::new();
    k.want_run(0);
    k.step(0);
    assert!(k.spawn_failed(0).is_empty());
    assert_eq!(k.crashes(), 1);
    assert!(k.step(500).is_empty());
    assert_eq!(k.step(BACKOFF_FIRST_MS), vec![Action::Spawn]);
}

#[test]
fn a_new_message_during_the_wait_does_not_skip_the_backoff() {
    let mut k = ready(0);
    k.exited(1_000, false);
    k.want_run(1_100);
    assert!(k.step(1_100).is_empty());
    assert_eq!(k.step(2_000), vec![Action::Spawn]);
}

#[test]
fn start_again_clears_a_wait_too() {
    let mut k = ready(0);
    k.exited(1_000, false);
    k.retry(1_200);
    assert_eq!(k.step(1_200), vec![Action::Spawn]);
}

#[test]
fn a_message_while_it_is_leaving_starts_it_again_after_it_has_gone() {
    let mut k = ready(0);
    k.person_stop();
    assert_eq!(k.step(100), vec![Action::Shutdown]);
    k.want_run(200);
    assert!(k.exited(300, true).is_empty());
    assert_eq!(k.step(300), vec![Action::Spawn]);
}
