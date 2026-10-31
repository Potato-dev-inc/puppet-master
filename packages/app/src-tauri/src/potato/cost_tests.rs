//! Cost caps: pricing, the unknown model, the 80% and 100% lines, and the usage file.

use std::path::PathBuf;

use super::cost::*;

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pm-potato-cost-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn tokens(prompt: u64, completion: u64, hit: u64, miss: u64) -> Tokens {
    Tokens { prompt, completion, cache_hit: hit, cache_miss: miss }
}

#[test]
fn a_known_model_is_priced_by_its_row() {
    let p = price_of("deepseek-flash");
    assert_eq!(price_tokens("deepseek-flash", tokens(1_000, 100, 400, 600)), 400 * p.input_hit + 600 * p.input_miss + 100 * p.output);
    // by prefix, the longest match wins
    assert_eq!(price_of("deepseek-reasoner-2"), price_of("deepseek-reasoner"));
    assert_eq!(price_of("DeepSeek-Chat"), price_of("deepseek-chat"));
}

#[test]
fn an_unknown_model_is_charged_at_the_highest_price_in_the_table() {
    let top = highest_price();
    for name in ["some-new-model", "", "gpt-whatever"] {
        assert_eq!(price_of(name), top, "{name:?}");
    }
    assert!(top.output >= price_of("deepseek-flash").output);
    assert!(top.output >= price_of("deepseek-reasoner").output);
    assert_eq!(price_tokens("some-new-model", tokens(0, 1_000, 0, 0)), 1_000 * top.output);
}

#[test]
fn the_free_test_models_cost_nothing() {
    assert_eq!(price_tokens("fake", tokens(10, 5, 0, 10)), 0);
    assert_eq!(price_tokens("stub", tokens(1_000_000, 1_000_000, 0, 0)), 0);
}

#[test]
fn input_tokens_the_brain_does_not_split_are_charged_as_misses() {
    let p = price_of("deepseek-flash");
    assert_eq!(price_tokens("deepseek-flash", tokens(1_000, 0, 0, 0)), 1_000 * p.input_miss);
    assert_eq!(price_tokens("deepseek-flash", tokens(1_000, 0, 300, 0)), 300 * p.input_hit + 700 * p.input_miss);
}

#[test]
fn big_numbers_do_not_overflow() {
    let cost = price_tokens("never-heard-of-it", tokens(u64::MAX, u64::MAX, u64::MAX, u64::MAX));
    assert_eq!(cost, u64::MAX);
}

#[test]
fn the_limit_is_kept_between_5_cents_and_20_dollars() {
    assert_eq!(clamp_limit_cents(0), 5);
    assert_eq!(clamp_limit_cents(50), 50);
    assert_eq!(clamp_limit_cents(1_000_000), 2_000);
    assert_eq!(DEFAULT_LIMIT_CENTS, 50);
    assert_eq!(dollars(50), "$0.50");
    assert_eq!(dollars(2_000), "$20.00");
    assert_eq!(dollars(5), "$0.05");
}

#[test]
fn eighty_percent_warns_and_a_hundred_stops() {
    let limit = limit_nano(50);
    assert_eq!(budget(0, 50), Budget::Under);
    assert_eq!(budget(limit * 79 / 100, 50), Budget::Under);
    assert_eq!(budget(limit * 80 / 100, 50), Budget::Warn);
    assert_eq!(budget(limit - 1, 50), Budget::Warn);
    assert_eq!(budget(limit, 50), Budget::Over);
    assert_eq!(budget(limit * 3, 50), Budget::Over);
}

#[test]
fn charging_adds_up_per_day_bot_and_surface_and_says_when_a_line_is_crossed() {
    let root = temp("charge");
    // 50 cents = 500_000_000 nano. "deepseek-flash" output is 1100 nano a token: 100_000 tokens = 110_000_000.
    let say = |root: &PathBuf, day: &str, bot: &str, completion: u64| charge(root, day, bot, "app", "deepseek-flash", tokens(0, completion, 0, 0), 50).unwrap();
    let a = say(&root, "2026-11-01", "nimbus", 100_000);
    assert_eq!((a.cost_nano, a.used_nano, a.budget, a.warn_now, a.limit_now), (110_000_000, 110_000_000, Budget::Under, false, false));
    let b = say(&root, "2026-11-01", "nimbus", 300_000);
    assert_eq!(b.used_nano, 440_000_000);
    assert_eq!(b.budget, Budget::Warn);
    assert!(b.warn_now && !b.limit_now, "the 80% note is due, once");
    let c = say(&root, "2026-11-01", "nimbus", 10_000);
    assert_eq!(c.budget, Budget::Warn);
    assert!(!c.warn_now, "the note comes once a day");
    let d = say(&root, "2026-11-01", "nimbus", 100_000);
    assert_eq!(d.budget, Budget::Over);
    assert!(d.limit_now, "the pause is due now");
    let e = say(&root, "2026-11-01", "nimbus", 10_000);
    assert!(!e.limit_now, "the pause is announced once");
    assert_eq!(e.budget, Budget::Over);
    // another bot has its own day, and so does the next day
    let other = say(&root, "2026-11-01", "mochi", 1_000);
    assert_eq!(other.used_nano, 1_100_000);
    let tomorrow = say(&root, "2026-11-02", "nimbus", 1_000);
    assert_eq!((tomorrow.used_nano, tomorrow.budget), (1_100_000, Budget::Under));
    // the file: per day, per bot, per surface
    let day = day_of(&root, "2026-11-01", "nimbus").unwrap();
    assert_eq!(day.surfaces["app"].turns, 5);
    assert_eq!(day.surfaces["app"].completion_tokens, 520_000);
    assert!(day.limit_hit && day.warned);
}

#[test]
fn surfaces_add_up_against_one_limit() {
    let root = temp("surfaces");
    charge(&root, "2026-11-01", "nimbus", "app", "deepseek-flash", tokens(0, 200_000, 0, 0), 50).unwrap();
    let other = charge(&root, "2026-11-01", "nimbus", "outer", "deepseek-flash", tokens(0, 200_000, 0, 0), 50).unwrap();
    assert_eq!(other.used_nano, 440_000_000, "the limit is per bot and covers every surface");
    let day = day_of(&root, "2026-11-01", "nimbus").unwrap();
    assert_eq!(day.surfaces.len(), 2);
}

#[test]
fn the_usage_file_is_one_per_month_and_a_damaged_one_is_never_overwritten() {
    let root = temp("damaged");
    charge(&root, "2026-11-30", "nimbus", "app", "fake", tokens(10, 5, 0, 10), 50).unwrap();
    charge(&root, "2026-12-01", "nimbus", "app", "fake", tokens(10, 5, 0, 10), 50).unwrap();
    assert!(root.join("usage").join("2026-11.json").is_file());
    assert!(root.join("usage").join("2026-12.json").is_file());
    let path = month_path(&root, "2026-11-30");
    std::fs::write(&path, "{ not json").unwrap();
    assert!(charge(&root, "2026-11-30", "nimbus", "app", "fake", tokens(1, 1, 0, 1), 50).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json", "a damaged file is left as it is");
    // a missing month reads as empty
    assert_eq!(day_of(&root, "2027-01-05", "nimbus").unwrap().cost_nano(), 0);
}
