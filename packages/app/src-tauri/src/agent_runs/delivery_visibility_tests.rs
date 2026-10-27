//! Whether a long prompt shows in the input box of a small pane. A phone grid is 45 columns by 12
//! rows: the box is taller than the pane, so only the end of the text shows, and the TUI wraps the
//! text at the box edge (mid-word). Included from `delivery.rs`.

use super::*;

/// A prompt of exactly `total` characters: `opening`, filler, `ending`.
fn prompt_of(total: usize, opening: &str, ending: &str) -> String {
    let filler =
        "the cookie is dropped after the redirect and the test only fails in CI ".repeat(8);
    let body: String = filler
        .chars()
        .take(total - opening.chars().count() - ending.chars().count() - 2)
        .collect();
    let text = format!("{opening} {body} {ending}");
    assert_eq!(text.chars().count(), total);
    text
}

/// The pane's rows for a prompt typed into a box `cols` wide: the text hard-wrapped at the box edge
/// (a word can split across two rows), the box border, two footer rows. Only the bottom `rows` rows
/// are on screen, as when the box is taller than the pane.
fn screen_with_box(text: &str, cols: usize, rows: usize) -> String {
    let inner = cols - 4;
    let chars: Vec<char> = text.chars().collect();
    let mut lines = vec![format!("╭{}╮", "─".repeat(cols - 2))];
    for chunk in chars.chunks(inner) {
        let row: String = chunk.iter().collect();
        lines.push(format!("│ {row:<inner$} │"));
    }
    lines.push(format!("╰{}╯", "─".repeat(cols - 2)));
    lines.push("  ? for shortcuts".to_string());
    lines.push("Composer 2.5 · 3%".to_string());
    let start = lines.len().saturating_sub(rows);
    lines[start..].join("\n")
}

/// The same text as a transcript echo after Enter: hard-wrapped rows, no box.
fn echo_of(text: &str, cols: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    let rows: Vec<String> = chars
        .chunks(cols - 2)
        .map(|chunk| format!("> {}", chunk.iter().collect::<String>()))
        .collect();
    format!("{}\nworking\n? for shortcuts", rows.join("\n"))
}

#[test]
fn the_end_of_a_390_character_prompt_in_a_45_by_12_pane_is_visible_when_its_start_scrolled_away() {
    let prompt = prompt_of(
        390,
        "Review the login change",
        "Then reply with the word CEDAR only.",
    );
    let screen = screen_with_box(&prompt, 45, 12);
    let head = head_of(&prompt).unwrap();
    assert!(
        !squeezed(&screen).contains(&head),
        "the start of the prompt should be out of view:\n{screen}"
    );
    assert!(prompt_visible_in_input(&screen, &prompt), "{screen}");
}

#[test]
fn a_different_long_prompt_whose_end_is_not_on_screen_is_not_visible() {
    let shown = prompt_of(
        390,
        "Review the login change",
        "Then reply with the word CEDAR only.",
    );
    let typed = prompt_of(
        390,
        "Check the billing export",
        "Then reply with the word OAK only.",
    );
    let screen = screen_with_box(&shown, 45, 12);
    assert!(!prompt_visible_in_input(&screen, &typed), "{screen}");
}

#[test]
fn the_whole_390_character_prompt_in_a_159_column_pane_is_visible() {
    let prompt = prompt_of(
        390,
        "Review the login change",
        "Then reply with the word CEDAR only.",
    );
    let screen = screen_with_box(&prompt, 159, 56);
    assert!(prompt_visible_in_input(&screen, &prompt), "{screen}");
}

#[test]
fn a_follow_up_sharing_the_opening_is_not_visible_from_the_previous_prompt() {
    // 390 characters at 45 columns (only the end shows); 250 at 159 (the whole text shows, and a
    // prompt over 300 characters is only checked by its opening, as before).
    for (total, cols, rows) in [(390, 45, 12), (250, 159, 56)] {
        let previous = prompt_of(
            total,
            "Review the login change",
            "Then reply with the word CEDAR only.",
        );
        let next = prompt_of(
            total,
            "Review the login change",
            "Then reply with the word OAK only.",
        );
        let screen = screen_with_box(&previous, cols, rows);
        assert!(
            prompt_visible_in_input(&screen, &previous),
            "{cols} columns: the previous prompt shows\n{screen}"
        );
        assert!(
            !prompt_visible_in_input(&screen, &next),
            "{cols} columns: a follow-up passed on the shared opening alone\n{screen}"
        );
    }
}

#[test]
fn a_prompt_with_a_word_split_by_the_wrap_at_45_columns_is_visible() {
    // The wrap cuts `session_cookie_renewal_handler` after `ren`: the rows read `ren` / `ewal`.
    let prompt = "Please explain why the session_cookie_renewal_handler fails after every single retry attempt";
    let screen = screen_with_box(prompt, 45, 12);
    assert!(
        !normalize(&screen).contains(&normalize(prompt)),
        "the wrap should split a word:\n{screen}"
    );
    assert!(prompt_visible_in_input(&screen, prompt), "{screen}");
}

#[test]
fn a_long_prompt_echoed_wrapped_after_enter_is_a_started_turn() {
    let prompt = prompt_of(
        390,
        "Review the login change",
        "Then reply with the word CEDAR only.",
    );
    let before = screen_with_box(&prompt, 45, 12);
    let after = echo_of(&prompt, 45);
    assert!(turn_started(&before, &after, &prompt, false), "{after}");
}

#[test]
fn a_long_prompt_left_in_the_box_after_enter_is_not_a_started_turn() {
    let prompt = prompt_of(
        390,
        "Review the login change",
        "Then reply with the word CEDAR only.",
    );
    let before = screen_with_box(&prompt, 45, 12);
    let now = before.replace("? for shortcuts", "esc to cancel");
    assert_ne!(before, now);
    assert!(!turn_started(&before, &now, &prompt, false), "{now}");
}

#[test]
fn a_long_prompt_cleared_from_the_box_after_enter_is_a_started_turn() {
    let prompt = prompt_of(
        390,
        "Review the login change",
        "Then reply with the word CEDAR only.",
    );
    let before = screen_with_box(&prompt, 45, 12);
    let now = "? for shortcuts\nComposer 2.5 · 3%\nworking";
    assert!(turn_started(&before, now, &prompt, false));
}
