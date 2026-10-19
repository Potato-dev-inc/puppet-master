//! The AI look at a menu the wizard could not read (SPEC 11.20). Only with "Finish without me" on, only in a pane the wizard
//! started in its own temporary folder.
//!
//! When the safe answerer cannot find the highlighted row or any menu on a trust or permission question, the front end
//! sends the masked screen to the drafter AI with a fixed prompt that asks two things: the visible text of the option that
//! means "allow this once / yes", and the key(s) that select it, from a short whitelist. The AI never types. This file is
//! the Rust side: it checks the answer against the screen and against the safety rules, and only then hands the keys to
//! the answerer, which presses them ONE AT A TIME and looks at the screen after each.
//!
//! What Rust checks (any failure is a no and nothing is pressed):
//! - the screen is not a sign-in, payment, model or telemetry question (`off_limits`);
//! - every key is on the whitelist (Enter, Up, Down, Left, Right, Tab, `y`, `n`, `1`-`9`), there are at most
//!   `MAX_AI_KEYS`, Enter is only ever the last key, the last key answers (Enter, a digit or `y`), and `n` is never sent
//!   (it answers no);
//! - the option text is on the screen as one option, exactly (not a part of a longer option), once;
//! - that option plainly means yes / allow once: it is never always / session / all / don't ask again / remember / bypass;
//! - a digit key is the number printed next to that very option; `y` only answers an option that says yes or allow;
//! - while the keys go in: the highlighted row, when it can be read at all, is the named option before Enter is pressed;
//!   a navigation key that changes nothing on the screen stops the answer; and the question must be gone within the
//!   answerer's deadline after the last key.

use super::answer::AnswerKey;
use super::menu::{classify_option, is_deny, off_limits, shown_options, OptionClass};

/// The most keys one AI answer may press.
pub const MAX_AI_KEYS: usize = 6;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiPlan {
    /// The option as the screen words it.
    pub target: String,
    pub keys: Vec<AnswerKey>,
}

/// The text of an option, folded so the AI's copy and the screen's compare equal: no marker, number or surrounding
/// quotes, one space between words, lower case, no final stop.
pub fn canon(text: &str) -> String {
    let t = text.trim().trim_matches(|c: char| matches!(c, '"' | '\'' | '`' | '\u{201C}' | '\u{201D}'));
    let t = t.trim_start_matches(|c: char| c.is_whitespace() || super::menu::marker_chars().contains(c));
    let t = t.trim_start_matches(|c: char| c.is_ascii_digit()).trim_start_matches(['.', ')', ':']).trim_start();
    t.split_whitespace().collect::<Vec<_>>().join(" ").trim_end_matches(['.', ';', ':']).to_lowercase()
}

/// A key the AI named, if it is on the whitelist.
pub fn parse_key(name: &str) -> Result<AnswerKey, String> {
    let n = name.trim();
    match n.to_ascii_lowercase().as_str() {
        "enter" | "return" => Ok(AnswerKey::Enter),
        "up" => Ok(AnswerKey::Up),
        "down" => Ok(AnswerKey::Down),
        "left" => Ok(AnswerKey::Left),
        "right" => Ok(AnswerKey::Right),
        "tab" => Ok(AnswerKey::Tab),
        "y" => Ok(AnswerKey::Text("y".into())),
        "n" => Err("the key n answers no, which is never pressed".into()),
        d if d.len() == 1 && d.chars().all(|c| ('1'..='9').contains(&c)) => Ok(AnswerKey::Text(d.to_string())),
        _ => Err(format!("the key \"{}\" is not one the wizard may press", n.chars().take(20).collect::<String>())),
    }
}

pub fn is_navigation(key: &AnswerKey) -> bool {
    matches!(key, AnswerKey::Up | AnswerKey::Down | AnswerKey::Left | AnswerKey::Right | AnswerKey::Tab)
}

/// Check the AI's answer against the screen. `option` is the option's visible text, `keys` the key names.
pub fn validate(screen: &str, option: &str, keys: &[String]) -> Result<AiPlan, String> {
    if let Some(word) = off_limits(screen) {
        return Err(format!("the question mentions \"{word}\", and sign-in, payment, model and telemetry questions are only yours to answer"));
    }
    if keys.is_empty() {
        return Err("no key was named".into());
    }
    if keys.len() > MAX_AI_KEYS {
        return Err(format!("{} keys were named; at most {MAX_AI_KEYS} are pressed", keys.len()));
    }
    let parsed: Vec<AnswerKey> = keys.iter().map(|k| parse_key(k)).collect::<Result<_, _>>()?;
    let name = canon(option);
    if name.is_empty() {
        return Err("no option text was named".into());
    }
    // the option must be on the screen, as one option, once
    let mut hits: Vec<_> = Vec::new();
    for shown in shown_options(screen) {
        if canon(&shown.text) == name && !hits.iter().any(|h: &super::menu::Shown| h.text == shown.text && h.number == shown.number) {
            hits.push(shown);
        }
    }
    let hit = match hits.as_slice() {
        [] => return Err("the named option is not on the screen".into()),
        [one] => one.clone(),
        _ => return Err("the named option text appears more than once on the screen".into()),
    };
    if classify_option(&hit.text) != OptionClass::SafeYes || is_deny(&hit.text) {
        return Err(format!("\"{}\" is not a plain yes or allow-once option, so the wizard never picks it", hit.text));
    }
    // the keys
    let last = parsed.last().expect("at least one key");
    if parsed[..parsed.len() - 1].iter().any(|k| *k == AnswerKey::Enter) {
        return Err("Enter may only be the last key".into());
    }
    let ends_the_question = match last {
        AnswerKey::Enter => true,
        AnswerKey::Text(t) => t.chars().all(|c| c.is_ascii_alphanumeric()),
        _ => false,
    };
    if !ends_the_question {
        return Err("the keys do not end with Enter, a number or y, so they would not answer".into());
    }
    for key in &parsed {
        let AnswerKey::Text(t) = key else { continue };
        if t == "y" {
            let c = canon(&hit.text);
            let yes = c.starts_with("yes") || c.starts_with("y ") || c == "y" || c.starts_with("allow") || c.starts_with("approve") || c.starts_with("trust") || hit.hotkey == Some('y');
            if !yes {
                return Err("y answers an option that says yes or allow, and the named option does not".into());
            }
        } else if hit.number.map(|n| n.to_string()).as_deref() != Some(t.as_str()) {
            return Err(format!("the key {t} is not the number printed next to the named option"));
        }
    }
    Ok(AiPlan { target: hit.text, keys: parsed })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    const MENU: &str = "Run this command?\n  echo ok\n\n  Yes, allow once\n  Yes, always allow in this project\n  No\n";

    #[test]
    fn a_plain_once_option_with_navigation_and_enter_is_accepted() {
        let plan = validate(MENU, "Yes, allow once", &keys(&["Down", "Enter"])).expect("valid");
        assert_eq!(plan.target, "Yes, allow once");
        assert_eq!(plan.keys, vec![AnswerKey::Down, AnswerKey::Enter]);
    }

    #[test]
    fn a_broad_option_is_never_accepted() {
        let e = validate(MENU, "Yes, always allow in this project", &keys(&["Enter"])).unwrap_err();
        assert!(e.contains("never picks"), "{e}");
    }

    #[test]
    fn an_option_that_is_not_on_the_screen_is_refused() {
        assert!(validate(MENU, "Yes, allow once and forever", &keys(&["Enter"])).unwrap_err().contains("not on the screen"));
        // a part of a longer option is not that option
        assert!(validate(MENU, "Yes", &keys(&["Enter"])).is_err());
    }

    #[test]
    fn keys_outside_the_whitelist_or_too_many_are_refused() {
        assert!(validate(MENU, "Yes, allow once", &keys(&["Escape"])).is_err());
        assert!(validate(MENU, "Yes, allow once", &keys(&["ctrl+c"])).is_err());
        assert!(validate(MENU, "Yes, allow once", &keys(&["a"])).is_err());
        assert!(validate(MENU, "Yes, allow once", &keys(&["n"])).unwrap_err().contains("answers no"));
        assert!(validate(MENU, "Yes, allow once", &keys(&["Down"; 7])).unwrap_err().contains("at most"));
        assert!(validate(MENU, "Yes, allow once", &keys(&["Enter", "Down", "Enter"])).is_err());
        assert!(validate(MENU, "Yes, allow once", &keys(&["Down"])).unwrap_err().contains("would not answer"));
    }

    #[test]
    fn a_digit_must_be_the_number_of_the_named_option() {
        let numbered = "Allow nimbus to run echo?\n 1. Allow once\n 2. Always allow in this project\n 3. Deny\n";
        assert!(validate(numbered, "Allow once", &keys(&["1"])).is_ok());
        assert!(validate(numbered, "Allow once", &keys(&["2"])).unwrap_err().contains("not the number"));
        assert!(validate(MENU, "Yes, allow once", &keys(&["1"])).is_err(), "no number is printed on this menu");
    }

    #[test]
    fn a_sign_in_screen_is_never_answered() {
        let s = "Sign in to continue\n  Yes, allow once\n  No\n";
        assert!(validate(s, "Yes, allow once", &keys(&["Enter"])).unwrap_err().contains("only yours"));
    }

    #[test]
    fn a_button_row_names_its_segment() {
        let row = "Allow nimbus to run echo ok?\n\n  Allow once   Always allow   Deny\n";
        let plan = validate(row, "Allow once", &keys(&["Enter"])).expect("valid");
        assert_eq!(plan.target, "Allow once");
        assert!(validate(row, "Always allow", &keys(&["Right", "Enter"])).is_err());
        assert!(validate(row, "Deny", &keys(&["Enter"])).is_err());
    }
}
