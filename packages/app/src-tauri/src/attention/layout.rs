//! How the choices of an approval are laid out, decided once in Rust so the phone and the desktop
//! cannot drift. This is `layoutChoices` of `lib/approval-risk.ts` ported (that function stays the
//! desktop's reader, with a fallback for an old bridge).
//!
//! Normal prompts keep every choice in view and the first one primary (the agent's own order).
//! High-risk prompts hide the "don't ask again" family behind "More options", never make Allow the
//! primary or the focused button, and make Deny both when there is one. Nothing is removed: every
//! choice stays reachable.

use super::model::{ChoiceFact, Layout, Level};

pub fn layout(choices: &[ChoiceFact], level: Option<Level>) -> Layout {
    if level != Some(Level::High) {
        return Layout { primary: choices.first().map(|c| c.id.clone()), focus: None, more: Vec::new() };
    }
    let more: Vec<String> = choices.iter().filter(|c| is_broad(c)).map(|c| c.id.clone()).collect();
    let deny = choices.iter().filter(|c| !is_broad(c)).find(|c| is_deny(c)).map(|c| c.id.clone());
    Layout { primary: deny.clone(), focus: deny, more }
}

/// The detector flags broad answers (`prompt_risk::choice_is_broad`); the words are the fallback
/// for a choice that arrives without the flag.
fn is_broad(choice: &ChoiceFact) -> bool {
    choice.broad || crate::prompt_risk::choice_is_broad(&choice.id, &choice.label)
}

/// `^(deny|no|reject|cancel|decline|quit|stop|don'?t)\b` on the label or on the id with `_` and `-`
/// read as spaces, like the front end.
fn is_deny(choice: &ChoiceFact) -> bool {
    starts_with_deny_word(choice.label.trim()) || starts_with_deny_word(choice.id.replace(['_', '-'], " ").trim())
}

fn starts_with_deny_word(text: &str) -> bool {
    let word: String = text
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '\'' || *c == '\u{2019}')
        .flat_map(char::to_lowercase)
        .map(|c| if c == '\u{2019}' { '\'' } else { c })
        .collect();
    matches!(word.as_str(), "deny" | "no" | "reject" | "cancel" | "decline" | "quit" | "stop" | "don't" | "dont")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choice(id: &str, label: &str, broad: bool) -> ChoiceFact {
        ChoiceFact { id: id.into(), label: label.into(), broad }
    }

    fn command_choices() -> Vec<ChoiceFact> {
        vec![choice("allow_once", "Allow once", false), choice("allow_all", "Allow all commands", true), choice("deny", "Deny", false)]
    }

    #[test]
    fn a_calm_prompt_keeps_every_choice_and_the_agents_first_one_primary() {
        for level in [Some(Level::Low), Some(Level::Medium), None] {
            let l = layout(&command_choices(), level);
            assert_eq!(l, Layout { primary: Some("allow_once".into()), focus: None, more: vec![] }, "{level:?}");
        }
    }

    #[test]
    fn a_high_risk_prompt_leads_with_deny_and_tucks_the_broad_answers_away() {
        let l = layout(&command_choices(), Some(Level::High));
        assert_eq!(l.primary.as_deref(), Some("deny"));
        assert_eq!(l.focus.as_deref(), Some("deny"));
        assert_eq!(l.more, ["allow_all"]);
    }

    #[test]
    fn high_risk_without_a_deny_choice_focuses_nothing_rather_than_allow() {
        let l = layout(&[choice("allow_once", "Yes", false)], Some(Level::High));
        assert_eq!(l, Layout { primary: None, focus: None, more: vec![] });
    }

    #[test]
    fn deny_is_recognised_by_label_or_id_like_the_front_end() {
        let l = layout(&[choice("trust", "Trust this folder", false), choice("quit", "Don't trust", false)], Some(Level::High));
        assert_eq!(l.primary.as_deref(), Some("quit"), "a label that starts with don't");
        let l = layout(&[choice("allow_once", "Yes", false), choice("reject-it", "Anything", false)], Some(Level::High));
        assert_eq!(l.primary.as_deref(), Some("reject-it"), "an id that starts with reject");
        let l = layout(&[choice("a", "Yes", false), choice("n", "None of these", false)], Some(Level::High));
        assert_eq!(l.primary, None, "`none` is not `no`");
    }

    #[test]
    fn the_flag_is_not_needed_for_broad_wording() {
        let l = layout(&[choice("yes_always", "Yes, always", false), choice("deny", "Deny", false)], Some(Level::High));
        assert_eq!(l.more, ["yes_always"]);
    }
}
