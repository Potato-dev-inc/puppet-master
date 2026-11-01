//! The usage report: the 30 days, the totals per bot and in all, today against the cap, and the JSON names the page reads.

use std::path::PathBuf;

use super::cost::{self, Tokens};
use super::usage::{build, days_ending, DAYS};

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pm-potato-usage-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn the_days_end_with_today_oldest_first_and_cross_months_and_years() {
    assert_eq!(days_ending("2026-01-02", 4), ["2025-12-30", "2025-12-31", "2026-01-01", "2026-01-02"]);
    assert_eq!(days_ending("2024-03-01", 2), ["2024-02-29", "2024-03-01"], "2024 is a leap year");
    let window = days_ending("2026-11-01", DAYS);
    assert_eq!(window.len(), 30);
    assert_eq!((window.first().unwrap().as_str(), window.last().unwrap().as_str()), ("2026-10-03", "2026-11-01"));
    assert!(days_ending("not a day", 30).is_empty());
    assert!(days_ending("2026-13-01", 30).is_empty(), "a month that does not exist is not a day");
}

#[test]
fn counting_back_day_by_day_crosses_a_leap_day_and_never_skips_or_repeats() {
    // 801 days ending on 2028-05-13 cross 2028-02-29: each day must be exactly one day before the next one
    let window = days_ending("2028-05-13", 801);
    assert_eq!(window.len(), 801);
    assert!(window.contains(&"2028-02-29".to_string()));
    assert!(!window.contains(&"2028-02-30".to_string()));
    for pair in window.windows(2) {
        assert_eq!(days_ending(&pair[1], 2)[0], pair[0], "the day before {} is {}", pair[1], pair[0]);
    }
    let mut unique = window.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), window.len());
}

#[test]
fn a_report_adds_the_bots_per_day_and_in_total_and_leaves_out_the_days_before_the_window() {
    let root = temp("report");
    let price = |t: Tokens| cost::price_tokens("deepseek-flash", t);
    let turn = Tokens { prompt: 1_000, completion: 200, cache_hit: 0, cache_miss: 1_000 };
    // a turn on the 30th of October and two on today (1 November)
    cost::charge(&root, "2026-10-30", "nimbus", "app", "deepseek-flash", turn, 50).unwrap();
    cost::charge(&root, "2026-11-01", "nimbus", "app", "deepseek-flash", turn, 50).unwrap();
    cost::charge(&root, "2026-11-01", "nimbus", "app", "deepseek-flash", turn, 50).unwrap();
    cost::charge(&root, "2026-11-01", "mochi", "app", "deepseek-flash", turn, 50).unwrap();
    // a turn in September is outside the window and is not counted
    cost::charge(&root, "2026-09-02", "nimbus", "app", "deepseek-flash", turn, 50).unwrap();
    cost::spend_sandbox_actions(&root, "2026-11-01", "nimbus", 7, 500).unwrap();

    let bots = vec![("nimbus".to_string(), "Nimbus".to_string()), ("mochi".to_string(), "Mochi".to_string())];
    let report = build(&root, "2026-11-01", &bots, 50, 500).unwrap();

    assert_eq!(report.days.len(), 30);
    assert_eq!(report.days.last().unwrap().date, "2026-11-01");
    let one_turn_cost = price(turn);
    let one_turn_tokens = 1_200;
    assert_eq!(report.total.messages, 4, "three of Nimbus and one of Mochi inside the window");
    assert_eq!(report.total.tokens, 4 * one_turn_tokens);
    assert_eq!(report.total.cost_nano, 4 * one_turn_cost);
    assert_eq!(report.today_total.messages, 3);
    assert_eq!(report.today_total.sandbox_actions, 7);

    let nimbus = report.bots.iter().find(|b| b.id == "nimbus").unwrap();
    assert_eq!(nimbus.name, "Nimbus");
    assert_eq!(nimbus.total.messages, 3);
    assert_eq!(nimbus.today.messages, 2);
    assert_eq!(nimbus.today.cost_nano, 2 * one_turn_cost);
    assert_eq!(nimbus.today.sandbox_actions, 7);
    let mochi = report.bots.iter().find(|b| b.id == "mochi").unwrap();
    assert_eq!((mochi.total.messages, mochi.today.messages), (1, 1));

    // the per-day rows add up to the total
    let per_day: u64 = report.days.iter().map(|d| d.totals.cost_nano).sum();
    assert_eq!(per_day, report.total.cost_nano);
    let oct = report.days.iter().find(|d| d.date == "2026-10-30").unwrap();
    assert_eq!(oct.totals.messages, 1);
}

#[test]
fn the_caps_are_the_clamped_settings_and_the_report_is_read_only_json() {
    let root = temp("json");
    let bots = vec![("nimbus".to_string(), "Nimbus".to_string())];
    let report = build(&root, "2026-11-01", &bots, 999_999, 1).unwrap();
    assert_eq!(report.limit_cents, 2_000, "clamped like the settings");
    assert_eq!(report.sandbox_actions_per_day, 10);
    assert_eq!(report.total.messages, 0, "a missing file is an empty month");
    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["sandboxActionsPerDay"], 10);
    assert_eq!(json["bots"][0]["name"], "Nimbus");
    assert_eq!(json["days"][0]["costNano"], 0);
    assert!(json["days"][0].get("messages").is_some(), "the totals sit flat beside the date");
    assert!(!root.join("usage").join("2026-11.json").exists(), "reading never writes");
}

#[test]
fn a_damaged_usage_file_is_an_error_and_is_not_overwritten() {
    let root = temp("damaged");
    std::fs::create_dir_all(root.join("usage")).unwrap();
    let path = root.join("usage").join("2026-11.json");
    std::fs::write(&path, "{ not json").unwrap();
    let err = build(&root, "2026-11-01", &[], 50, 500).unwrap_err();
    assert!(err.contains("damaged"), "{err}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json");
}
