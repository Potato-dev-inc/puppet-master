//! What counts as the program's own error (`wizard/agent_error.rs`): real rows only, never the banner.

use super::super::agent_error::{find, find_after, first_text_row, is_banner_row, is_chrome_row, is_logo_row, is_real_message, is_status_row, only_status_rows_look_wrong, Baseline};

const BANNER_SCREEN: &str = "\u{2584}\u{2580} Antigravity CLI 1.4.2.\nSigned in as dev@example.com\n\n\u{2502} > Type your message  \u{2502}\n  ctrl+c to exit | /login to sign in\n";

#[test]
fn logo_rows_rules_and_version_banners_are_chrome() {
    for row in [
        "\u{2584}\u{2580}",
        "\u{2597}\u{2584}\u{2584}\u{2596}  \u{2590}\u{258c}",
        "\u{28ff}\u{28ff}\u{28ff}\u{28ff}",
        "\u{256d}\u{2500}\u{2500}\u{2500}\u{2500}\u{256e}",
        "+--------+",
        "\u{2584}\u{2580} Antigravity CLI 1.4.2.",
        "Some Agent v2.0.1 (build 3)",
        "v1.4.2",
        "Nimbus CLI 1.4.2",
        "\u{2502} > Type your message  \u{2502}",
        "ctrl+c to exit",
        "esc to interrupt",
        "",
    ] {
        assert!(is_chrome_row(row), "{row:?} should be chrome");
    }
    assert!(is_logo_row("\u{2584}\u{2580}") && !is_logo_row("Error: 401 Unauthorized"));
    assert!(is_banner_row("\u{2584}\u{2580} Antigravity CLI 1.4.2.") && !is_banner_row("Error 1.4"));
}

#[test]
fn error_rows_are_never_chrome() {
    for row in [
        "Error: No endpoints found that support tool use.",
        "Cannot connect to 10.0.0.1",
        "401 Unauthorized",
        "You have run out of credits. Add more to continue.",
        "\u{2502} Please sign in to continue \u{2502}",
        "Not logged in. Run /login.",
    ] {
        assert!(!is_chrome_row(row), "{row:?} is not chrome");
    }
}

#[test]
fn a_banner_and_a_signed_in_status_row_are_not_an_agent_error() {
    assert_eq!(find(BANNER_SCREEN, None, &[], false), None);
    assert_eq!(find(BANNER_SCREEN, None, &[], true), None);
}

#[test]
fn the_error_row_is_found_below_the_banner_and_is_the_message() {
    let screen = format!("{BANNER_SCREEN}\nError: No endpoints found for this model.\n");
    let found = find(&screen, None, &[], false).expect("an error row");
    assert_eq!(found.message, "Error: No endpoints found for this model.");
    assert!(found.external);
    let boxed = "\u{2584}\u{2580} Nimbus CLI 1.4.2.\n\u{2502} Please sign in to continue \u{2502}\n";
    assert_eq!(find(boxed, None, &[], false).unwrap().message, "Please sign in to continue");
}

#[test]
fn a_row_that_quotes_the_tests_own_message_is_the_echo_not_an_error() {
    let screen = "> Reply with exactly the word: pong-abcdef and log in\n";
    assert!(find(screen, None, &[], false).is_none(), "an input row");
    let skip = vec!["pong-abcdef".to_string()];
    assert!(find("Reply with exactly the word: pong-abcdef, token limit\n", None, &skip, false).is_none());
}

#[test]
fn a_plain_error_row_counts_only_when_asked_and_a_usage_error_never() {
    let said = "\u{2584}\u{2580} Nimbus CLI 1.4.2.\nthread 'main' panicked at src/main.rs:12:5\n";
    assert_eq!(find(said, None, &[], false), None, "nothing points at sign-in, quota or provider");
    let plain = find(said, None, &[], true).unwrap();
    assert_eq!((plain.message.as_str(), plain.external), ("thread 'main' panicked at src/main.rs:12:5", false));
    let usage = "error: unexpected argument '--foo' found\nUsage: nimbus [OPTIONS]\n";
    assert_eq!(find(usage, None, &[], true), None, "a wrong argument is the profile's problem");
}

#[test]
fn json_rows_are_left_to_the_event_parser() {
    assert_eq!(find("{\"type\":\"error\",\"message\":\"quota exceeded\"}\n", None, &[], true), None);
}

#[test]
fn the_first_text_row_of_an_error_event_skips_logo_rows() {
    assert_eq!(first_text_row("\u{2584}\u{2580} Nimbus CLI 1.4.2.\n\nQuota exceeded. Try later.\nmore").as_deref(), Some("Quota exceeded. Try later."));
    assert_eq!(first_text_row("\u{2584}\u{2580}\n"), None);
}

#[test]
fn a_message_is_real_only_when_it_holds_words() {
    assert!(is_real_message("Error: No endpoints found"));
    for m in ["", "  ", "\u{2584}\u{2580}", "\u{2584}\u{2580} Antigravity CLI 1.4.2.", "v1.4.2"] {
        assert!(!is_real_message(m), "{m:?}");
    }
}

// ---- account and status rows are not errors; a row from before the task never is (founder run 2026-10-19) ---------

const ACCOUNT_ROW: &str = "someone@example.com (Nimbus Starter Quota)";

#[test]
fn an_account_row_with_an_email_or_a_plan_in_parentheses_is_status_not_an_error() {
    for row in [
        ACCOUNT_ROW,
        "\u{2502} someone@example.com (Nimbus Starter Quota) \u{2502}",
        "<email> (Nimbus Starter Quota)",
        "Plan: Starter",
        "Credits: 12.40 left",
        "Account someone@example.com",
        "Nimbus (Pro tier)",
        "Tier = Starter",
        "Usage: 40% of the plan",
        "someone@example.com \u{b7} Nimbus Starter Plan (500 credits)",
    ] {
        assert!(is_status_row(row), "{row:?} is a status row");
        assert_eq!(find(&format!("\u{2584}\u{2580} Nimbus CLI 1.4.2.\n{row}\n"), None, &[], false), None, "{row:?} is no error");
        assert_eq!(find(&format!("{row}\n"), None, &[], true), None, "{row:?} is no error, plain");
    }
}

#[test]
fn a_row_with_an_error_word_is_not_a_status_row_even_with_an_email_or_a_plan() {
    for row in [
        "Error: quota exceeded",
        "Login failed for someone@example.com",
        "someone@example.com: 429 Too Many Requests",
        "Plan limit reached (Starter)",
        "Quota exceeded (Starter plan)",
        "Credits: invalid",
        "Your session expired. Please sign in again as someone@example.com",
        "(Starter plan) is out of credits",
        "HTTP 402 Payment Required",
        "No endpoints found that support tool use. Check your provider settings.",
        "Not logged in. Run /login.",
    ] {
        assert!(!is_status_row(row), "{row:?} is not a status row");
    }
    assert_eq!(find("Error: quota exceeded\n", None, &[], false).unwrap().message, "Error: quota exceeded");
    assert_eq!(find("429 Too Many Requests\n", None, &[], false).unwrap().message, "429 Too Many Requests");
    assert!(find("Login failed for someone@example.com (Starter plan)\n", None, &[], false).unwrap().external);
    let found = find(&format!("{ACCOUNT_ROW}\nError: No endpoints found that support tool use.\n"), None, &[], false).unwrap();
    assert_eq!(found.message, "Error: No endpoints found that support tool use.", "the account row above it is skipped");
}

#[test]
fn a_row_that_was_on_the_screen_before_the_task_is_never_the_error_a_new_one_is() {
    let startup = "\u{2584}\u{2580} Nimbus CLI 1.4.2.\nTip: run /login to sign in with another provider\n\n| > Type a task |\n";
    let mut baseline = Baseline::new();
    baseline.add_screen(startup);
    let after = format!("{startup}\n> Reply with exactly the word: abc\nreply: nothing\n");
    // without a baseline the tip reads as a sign-in problem
    assert!(find(&after, None, &[], false).unwrap().message.contains("run /login"));
    // with it, the row that was there before is not the error
    assert_eq!(find_after(&after, None, &[], false, &baseline), None);
    assert_eq!(find_after(&after, None, &[], true, &baseline), None);
    // a real error that is new stays the message, whatever else was there
    let with_error = format!("{after}Error: quota exceeded\n");
    assert_eq!(find_after(&with_error, None, &[], false, &baseline).unwrap().message, "Error: quota exceeded");
    // a row the pane wrapped differently (a resize) is still the same row; a counter that ticked too
    let mut b = Baseline::new();
    b.add_screen("Tip: run /login to sign in with another provider (used 3 times)\n");
    assert!(b.has_row("Tip: run /login to sign in with"));
    assert!(b.has_row("Tip: run /login to sign in with another provider (used 7 times)"));
    assert!(!b.has_row("Error: quota exceeded"));
    // a short row is no evidence either way
    let mut short = Baseline::new();
    short.add_screen("Error: 429\n");
    assert!(!short.has_row("Error: 429"));
}

#[test]
fn a_verdict_error_about_only_status_or_baseline_rows_is_refused_but_a_real_error_is_not() {
    let empty = Baseline::new();
    assert!(only_status_rows_look_wrong(&format!("NIMBUS\n{ACCOUNT_ROW}\n| > Type a task |\n"), &empty));
    assert!(only_status_rows_look_wrong("NIMBUS\n<email> (Nimbus Starter Quota)\n", &empty));
    // a real error row beside the account row: the AI's look stands
    assert!(!only_status_rows_look_wrong(&format!("{ACCOUNT_ROW}\nError: 429 rate limit reached\n"), &empty));
    // nothing here reads like a problem at all: no say either way
    assert!(!only_status_rows_look_wrong("NIMBUS\nsomething odd happened\n", &empty));
    // a row from before the task is not new
    let mut b = Baseline::new();
    b.add_screen("Tip: run /login to sign in with another provider\n");
    assert!(only_status_rows_look_wrong("Tip: run /login to sign in with another provider\n", &b));
    assert!(!only_status_rows_look_wrong("Tip: run /login to sign in with another provider\nError: network down\n", &b));
}
