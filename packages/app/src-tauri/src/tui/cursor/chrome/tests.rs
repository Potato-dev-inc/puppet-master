use super::*;

#[test]
fn idle_placeholder_and_footer_are_chrome() {
    assert!(is_chrome_line("→ plan, search, build anything"));
    assert!(is_chrome_line("composer 2.5"));
    assert!(is_chrome_line("composer 2.5 · 7.2%"));
}

#[test]
fn tool_rows_are_chrome_but_prose_is_not() {
    assert!(is_tool_line("$ cd proj && python -m pytest 11s"));
    assert!(is_tool_line("… 2 output lines hidden · ctrl+o to expand"));
    assert!(is_tool_line("read todo.py"));
    assert!(!is_chrome_line("read the docs first."));
}

#[test]
fn diff_rows_and_truncation_notices_are_chrome() {
    assert!(is_chrome_line("\u{258e}+ import os"));
    assert!(is_chrome_line("\u{258e}- old = 1"));
    assert!(is_chrome_line("\u{258e}  unchanged context"));
    assert!(is_tool_line("\u{2026} truncated (14 more lines)"));
    assert!(is_chrome_line("... truncated (3 more lines)"));
    assert!(is_tool_line("edited todo.py +3 -1"));
    assert!(is_tool_line("created src/notes.md +12"));
    assert!(!is_chrome_line("edited the file to add a feature."));
    assert!(!is_chrome_line("the output was truncated because it was long."));
}

const TRUST_RESOLVED: &str = include_str!("../fixtures/workspace_trust_visible_resolved.txt");

#[test]
fn a_boxed_trust_dialog_is_stripped_but_other_boxes_survive() {
    let stripped = strip_dialog_boxes(TRUST_RESOLVED);
    assert!(!stripped.contains("Workspace Trust Required"), "{stripped}");
    assert!(!stripped.contains("Trust this workspace"), "{stripped}");
    // The dialog's own directory row goes; the `~\...` footer outside it is ordinary chrome.
    assert!(!stripped.contains(r"C:\Users\Ren-pc\AppData\Local\Temp\cur-_b1a_ip3"));
    assert!(stripped.contains("Plan, search, build anything"), "TUI below the dialog stays");
    let table = "\u{256d}\u{2500}\u{2500}\u{256e}\n\u{2502} a \u{2502} b \u{2502}\n\u{2570}\u{2500}\u{2500}\u{256f}";
    assert_eq!(strip_dialog_boxes(table), table);
}

#[test]
fn a_trust_dialog_without_its_border_rows_is_stripped_too() {
    let borderless: String = TRUST_RESOLVED
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            !t.starts_with('╭') && !t.starts_with('╰')
        })
        .collect::<Vec<_>>()
        .join("
");
    let stripped = strip_dialog_boxes(&borderless);
    assert!(!stripped.contains("Workspace Trust Required"), "{stripped}");
    assert!(!stripped.contains("Cursor Agent can execute"), "{stripped}");
    assert!(stripped.contains("Plan, search, build anything"));
}

#[test]
fn shell_call_headers_and_durations_are_tool_rows() {
    assert!(is_tool_line("$ cd \"c:\\proj\" && python todo.py add buy-milk && python todo.py list 6.0s"));
    assert!(is_tool_line("6.0s"));
    assert!(is_tool_line("1m 3s"));
    assert!(!is_tool_line("2 seconds later it finished"));
    assert!(!is_tool_line("done"));
}

#[test]
fn banner_rows_are_chrome() {
    assert!(is_chrome_line("cursor agent"));
    assert!(is_chrome_line("v2026.10.01-e373342"));
    assert!(is_chrome_line("tip: press ? for help"));
    assert!(is_chrome_line("~\\proj"));
}

#[test]
fn search_and_group_headers_are_tool_rows_but_prose_is_not() {
    assert!(is_tool_line("read, globbed 1 file, 1 glob"));
    assert!(is_tool_line("globbed, read 2 globs, 1 file"));
    assert!(is_tool_line("globbed \"**/test*.py\" in ."));
    assert!(is_tool_line("grepped \"todo\" in src"));
    assert!(is_tool_line("reading todo.py"));
    assert!(is_tool_line("running\u{2026}"));
    assert!(!is_tool_line("read the docs first."));
    assert!(!is_tool_line("read files."));
    assert!(!is_tool_line("searched for the bug in utils and found it"));
    assert!(!is_tool_line("exploring todo.py to identify supported commands"));
}

#[test]
fn a_dollar_row_is_a_shell_header_only_with_a_duration_or_approval_wait() {
    assert!(is_tool_line("$ python -m pytest -q 9.3s"));
    assert!(is_tool_line("$ python -m pytest -q 213ms"));
    assert!(is_tool_line("$ python -m pytest -q 1m 27s"));
    assert!(is_tool_line("$ python todo.py list waiting for approval..."));
    assert!(!is_tool_line("$ python todo.py list"));
    assert!(!is_tool_line("$ echo hello world"));
}

#[test]
fn a_sentence_that_starts_with_the_model_name_is_not_a_footer() {
    assert!(!is_chrome_line("composer 2.5 is mentioned in this answer."));
    assert!(!is_chrome_line("composer 2.5 \u{b7} up 5%"));
    assert!(!is_chrome_line("composer 2.5 \u{b7} is a model, 90% sure"));
    assert!(is_chrome_line("composer 2.5 \u{b7} 7.3%"));
    assert!(is_chrome_line("composer 2.5 \u{b7} 8.9% \u{b7} 1 file edited"));
}
