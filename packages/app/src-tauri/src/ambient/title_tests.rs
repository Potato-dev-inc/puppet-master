//! Tests for `title.rs`. Every expectation was derived by reading the implementation by hand.

use super::title::*;

fn assert_title(input: &str, expected: Option<&str>) {
    assert_eq!(derive_title(input).as_deref(), expected, "derive_title({input:?})");
}

#[test]
fn derives_titles_from_sample_lines() {
    let cases: &[(&str, Option<&str>)] = &[
        // Leading filler.
        ("please fix the login bug", Some("fix the login bug")),
        ("Can you refactor the auth module", Some("refactor the auth module")),
        ("could you please add tests", Some("add tests")),
        ("hey, can you add a README", Some("add a README")),
        ("I want you to refactor the parser", Some("refactor the parser")),
        ("go ahead and write the changelog", Some("write the changelog")),
        ("let's add dark mode", Some("add dark mode")),
        ("Let\u{2019}s add dark mode", Some("add dark mode")),
        ("help me fix the crash", Some("fix the crash")),
        ("hello, could we rename the config keys", Some("rename the config keys")),
        ("PLEASE fix x", Some("fix x")),
        ("Can You Fix Parser", Some("Fix Parser")),
        // Trailing filler.
        ("fix the login bug please", Some("fix the login bug")),
        ("fix the login bug, thanks", Some("fix the login bug")),
        ("fix the bug thank you", Some("fix the bug")),
        // First sentence only.
        ("Fix the login bug. Then also add tests", Some("Fix the login bug")),
        ("Fix the login bug! Then more", Some("Fix the login bug")),
        ("Refactor the parser?  Also tests", Some("Refactor the parser")),
        // Case, URLs, paths, unicode.
        ("Refactor the auth module", Some("Refactor the auth module")),
        ("check https://example.com/a/b.html please", Some("check https://example.com/a/b.html")),
        ("open src/main.rs and fix it", Some("open src/main.rs and fix it")),
        ("修复登录错误 please", Some("修复登录错误")),
        // Rejections.
        ("", None),
        ("   ", None),
        ("y", None),
        ("yes", None),
        ("ok", None),
        ("/clear", None),
        ("!ls", None),
        ("1", None),
        ("continue", None),
        ("can you", None),
        ("sk1234567890abcdefghijklm", None),
        ("?!", None),
    ];
    for &(input, expected) in cases {
        assert_title(input, expected);
    }
}

#[test]
fn titles_are_capped_at_40_chars_on_a_word_boundary() {
    let input = "Refactor the authentication middleware so that every request handler validates tokens consistently across all services";
    assert!(input.chars().count() > 80);
    let title = derive_title(input).expect("long line yields a title");
    assert!(title.chars().count() <= MAX_TITLE_CHARS);
    assert_eq!(title, "Refactor the authentication middleware\u{2026}");
    let body = title.strip_suffix('\u{2026}').expect("ends with ellipsis");
    assert!(input.starts_with(body));
    assert_eq!(input[body.len()..].chars().next(), Some(' '));
}

#[test]
fn single_long_word_is_hard_cut_with_ellipsis() {
    let word = "a".repeat(60);
    let title = derive_title(&word).expect("title");
    assert_eq!(title.chars().count(), MAX_TITLE_CHARS);
    assert_eq!(title, format!("{}\u{2026}", "a".repeat(MAX_TITLE_CHARS - 1)));
}

#[test]
fn exactly_40_chars_is_returned_unchanged() {
    let exact = "Refactor the auth module, then add tests";
    assert_eq!(exact.chars().count(), MAX_TITLE_CHARS);
    assert_eq!(derive_title(exact).as_deref(), Some(exact));
}

#[test]
fn sanitizes_ansi_and_control_chars() {
    assert_title("\u{1b}[31mfix\u{1b}[0m the \u{7}bug\r\n", Some("fix the bug"));
    assert_title("\u{1b}]0;window title\u{7}run tests", Some("run tests"));
    assert_eq!(sanitize("a\tb\n c"), "a b c");
}

#[test]
fn capture_splits_chunks_until_enter() {
    let mut cap = LineCapture::new();
    assert_eq!(cap.feed("refactor the ", false, ""), None);
    assert_eq!(cap.feed("auth", false, ""), None);
    assert_eq!(cap.feed(" module", false, ""), None);
    assert_eq!(cap.feed("\r", false, "").as_deref(), Some("refactor the auth module"));
}

#[test]
fn capture_append_newline_submits_the_chunk() {
    let mut cap = LineCapture::new();
    assert_eq!(cap.feed("please add a README", true, "").as_deref(), Some("add a README"));
}

#[test]
fn capture_handles_escape_sequence_split_across_chunks() {
    let mut cap = LineCapture::new();
    assert_eq!(cap.feed("fix \u{1b}", false, ""), None);
    assert_eq!(cap.feed("[31mred", false, ""), None);
    assert_eq!(cap.feed(" bug", false, ""), None);
    assert_eq!(cap.feed("\r", false, "").as_deref(), Some("fix red bug"));
}

#[test]
fn capture_backspace_deletes_previous_char() {
    let mut cap = LineCapture::new();
    assert_eq!(cap.feed("fiz\u{7f}x the bug\r", false, "").as_deref(), Some("fix the bug"));
    let mut cap = LineCapture::new();
    assert_eq!(cap.feed("fiz\u{8}x the bug\r", false, "").as_deref(), Some("fix the bug"));
}

#[test]
fn capture_ctrl_u_kills_line() {
    let mut cap = LineCapture::new();
    assert_eq!(cap.feed("wrong\u{15}right thing\r", false, "").as_deref(), Some("right thing"));
}

#[test]
fn capture_ctrl_w_deletes_words() {
    let mut cap = LineCapture::new();
    assert_eq!(cap.feed("fix the bugs\u{17}bug\r", false, "").as_deref(), Some("fix the bug"));
    // First Ctrl-W drops "bug"; second drops the trailing space and "login".
    let mut cap = LineCapture::new();
    assert_eq!(cap.feed("fix the login bug\u{17}\u{17}parser\r", false, "").as_deref(), Some("fix the parser"));
}

#[test]
fn capture_empty_enter_then_real_line() {
    let mut cap = LineCapture::new();
    assert_eq!(cap.feed("\r", false, ""), None);
    assert_eq!(cap.feed("add tests\r", false, "").as_deref(), Some("add tests"));
}

#[test]
fn capture_rejected_line_then_real_line() {
    let mut cap = LineCapture::new();
    assert_eq!(cap.feed("y\r", false, ""), None);
    assert_eq!(cap.feed("fix the parser\r", false, "").as_deref(), Some("fix the parser"));
}

#[test]
fn capture_multiline_paste_returns_first_title() {
    let mut cap = LineCapture::new();
    assert_eq!(cap.feed("\nfirst task here\nsecond line\r", false, "").as_deref(), Some("first task here"));
}

#[test]
fn capture_ignores_arrow_keys_mid_line() {
    let mut cap = LineCapture::new();
    assert_eq!(cap.feed("ab\u{1b}[Acd\u{1b}OAef\r", false, "").as_deref(), Some("abcdef"));
}

#[test]
fn huge_paste_does_not_grow_without_bound() {
    let mut cap = LineCapture::new();
    assert_eq!(cap.feed(&"x".repeat(100_000), false, ""), None);
    let title = cap.feed("\r", false, "").expect("paste yields a title");
    assert!(title.chars().count() <= MAX_TITLE_CHARS);
    assert_eq!(title, format!("{}\u{2026}", "x".repeat(MAX_TITLE_CHARS - 1)));
}

#[test]
fn every_secret_prompt_pattern_is_recognised_case_insensitively() {
    let prompts = [
        "Password:",
        "[sudo] password for ren:",
        "user@host's password:",
        "Password for 'https://github.com':",
        "Enter passphrase for key '/home/ren/.ssh/id_ed25519':",
        "Passcode:",
        "Enter PIN:",
        "Token:",
        "Paste your token:",
        "Client SECRET:",
        "Enter API key:",
        "Enter your apikey:",
        "api_key:",
        "OTP:",
        "2FA code:",
        "Verification code:",
        "Enter code:",
        "enter the code?",
        "Private key:",
    ];
    for prompt in prompts {
        assert!(is_secret_prompt(prompt), "{prompt:?} should be a secret prompt");
    }
}

#[test]
fn ordinary_screen_lines_are_not_secret_prompts() {
    let lines = ["> ", "fix the parser", "spinning up the worker", "shipping notes", "Pinned tasks", "Working (12s)", ""];
    for line in lines {
        assert!(!is_secret_prompt(line), "{line:?} should not be a secret prompt");
    }
}

#[test]
fn last_line_skips_trailing_blank_rows() {
    assert_eq!(last_line("first\n\nPassword:\n   \n"), "Password:");
    assert_eq!(last_line(""), "");
}

#[test]
fn a_line_submitted_at_a_secret_prompt_is_dropped_and_the_next_one_titles() {
    let mut cap = LineCapture::new();
    assert_eq!(cap.feed("hunter2\r", false, "Password:"), None);
    assert!(!cap.gave_up());
    assert_eq!(cap.feed("fix the parser\r", false, "> "), Some("fix the parser".into()));
}

#[test]
fn capture_gives_up_after_three_secret_prompt_submissions() {
    let mut cap = LineCapture::new();
    assert_eq!(cap.feed("one\r", false, "Enter API key:"), None);
    assert_eq!(cap.feed("two\r", false, "Enter API key:"), None);
    assert_eq!(cap.feed("three\r", true, "Enter API key:"), None);
    assert!(cap.gave_up());
    assert_eq!(cap.feed("fix the parser\r", false, "> "), None);
}

#[test]
fn a_secret_line_typed_in_one_chunk_with_enter_is_still_dropped() {
    let mut cap = LineCapture::new();
    assert_eq!(cap.feed("s3cret-value", true, "Password:"), None);
    assert_eq!(cap.feed("add tests\r", false, "> "), Some("add tests".into()));
}

#[test]
fn token_like_strings_are_still_rejected_on_a_normal_screen() {
    let mut cap = LineCapture::new();
    assert_eq!(cap.feed("ghp1234567890abcdefghijkl\r", false, "> "), None);
    assert!(!cap.gave_up());
}

#[test]
fn secret_prompt_table_matches_real_prompts_only() {
    let cases: &[(&str, bool)] = &[
        // Must match: a keyword near the end, then a prompt terminator.
        ("Password:", true),
        ("[sudo] password for ren:", true),
        ("Enter passphrase for key '/c/Users/x/.ssh/id_ed25519':", true),
        ("Paste your API key:", true),
        ("Token?", true),
        ("Verification code >", true),
        ("PIN:", true),
        ("Password: []", true),
        ("Password: (input hidden)", true),
        ("Password:   ", true),
        // Must not match: status lines and sentences that mention a keyword without asking.
        ("12.4k tokens", false),
        ("\u{2727} Thinking\u{2026} (3s \u{b7} 1.2k tokens)", false),
        ("context: 45% \u{b7} 98k tokens left", false),
        ("Using API key from env", false),
        ("Token usage summary", false),
        ("tokens: 3,200 used", false),
        // Keyword too far from the terminator to be the prompt's question.
        ("Enter the secret name of the release bucket for the nightly build job now:", false),
        ("", false),
    ];
    for &(line, expected) in cases {
        assert_eq!(is_secret_prompt(line), expected, "is_secret_prompt({line:?})");
    }
}
