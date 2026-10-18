//! What counts as the program's own error (`wizard/agent_error.rs`): real rows only, never the banner.

use super::super::agent_error::{find, first_text_row, is_banner_row, is_chrome_row, is_logo_row, is_real_message};

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
