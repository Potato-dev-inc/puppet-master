//! The card a permission request becomes: an ordinary question (`questions::Question`, kind `acp_permission`) whose
//! choices are the options the agent offered, so the Inbox, the phone's question list, the "Needs you" count and
//! the push pipeline already know how to show it. Pure: it builds the words and the choices, and maps a choice back
//! to the agent's option id. Creating and answering the card is `AcpChat`'s.
//!
//! The agent's own option names are shown as the agent wrote them ("Allow Once", "Allow for Session", "Reject"):
//! none is made a default, and the card has no timeout of its own that answers for the person. A card that runs out
//! of time (a day) is raised again while the agent still waits (`AcpChat::reconcile_cards`).

use crate::acp::state::{OptionBrief, PermissionWait};
use crate::questions::{Choice, NewQuestion, ACP_KIND_PERMISSION, MAX_CHOICES, MAX_LABEL_CHARS};
use crate::session_chat::detail::cut_to;

/// How long a card lives before it is raised again (the longest a question may).
pub const CARD_TIMEOUT_S: u64 = 24 * 3600;

/// One choice of a card and the option of the agent it stands for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardOption {
    /// The id on the card (the agent's option id when it fits, else `opt<n>`).
    pub choice_id: String,
    /// The agent's own option id: what goes back to the agent.
    pub option_id: String,
    pub name: String,
    /// `allow_once`, `allow_always`, `reject_once`, `reject_always`, or the agent's other word.
    pub kind: String,
}

/// A card that is on screen for one open question of the agent.
#[derive(Debug, Clone)]
pub struct Card {
    pub question_id: String,
    pub options: Vec<CardOption>,
    /// The person answered (or dismissed) it and the answer is on its way to the agent.
    pub resolved: bool,
}

impl Card {
    pub fn option_for_choice(&self, choice_id: &str) -> Option<&CardOption> {
        self.options.iter().find(|o| o.choice_id == choice_id)
    }

    /// The option a dismissed card stands for: the agent's "reject once", else "reject always". `None`: the agent
    /// offered no way to say no, and the turn is stopped instead.
    pub fn rejection(&self) -> Option<&CardOption> {
        ["reject_once", "reject_always"].iter().find_map(|kind| self.options.iter().find(|o| o.kind == *kind))
    }
}

fn rank(kind: &str) -> u8 {
    match kind {
        "allow_once" => 0,
        "allow_always" => 1,
        "reject_once" => 2,
        "reject_always" => 3,
        _ => 4,
    }
}

/// The card's choices for the options the agent offered. A card holds at most `MAX_CHOICES`: when the agent offers
/// more, the ones that say yes or no come first and the rest is left out. Order is the agent's otherwise.
pub fn options_of(offered: &[OptionBrief]) -> Vec<CardOption> {
    let mut list: Vec<(usize, &OptionBrief)> = offered.iter().enumerate().collect();
    if list.len() > MAX_CHOICES {
        list.sort_by_key(|(at, o)| (rank(&o.kind), *at));
        list.truncate(MAX_CHOICES);
        list.sort_by_key(|(at, _)| *at);
    }
    let mut out: Vec<CardOption> = Vec::new();
    for (at, o) in list {
        let fits = !o.id.trim().is_empty() && o.id.chars().count() <= MAX_LABEL_CHARS && !out.iter().any(|known| known.choice_id == o.id.trim());
        let choice_id = if fits { o.id.trim().to_string() } else { format!("opt{at}") };
        let name = if o.name.trim().is_empty() { o.kind.clone() } else { cut_to(o.name.trim(), MAX_LABEL_CHARS) };
        out.push(CardOption { choice_id, option_id: o.id.clone(), name, kind: o.kind.clone() });
    }
    out
}

/// The words of the card. `label`: the agent's name ("Codex"). `title`: what the tool call is called. `kind` and
/// `paths`: what the tool does and the files it names, when they are known.
pub fn new_question(pane_id: &str, label: &str, wait: &PermissionWait, title: Option<&str>, kind: Option<&str>, paths: &[String]) -> (NewQuestion, Vec<CardOption>) {
    let options = options_of(&wait.options);
    let what = title.map(str::trim).filter(|t| !t.is_empty()).map(|t| cut_to(t, 300));
    let question = match &what {
        Some(what) => format!("{label} asks permission: {what}"),
        None => format!("{label} asks permission to use a tool"),
    };
    let mut context = Vec::new();
    if let Some(kind) = kind.filter(|k| !k.is_empty() && *k != "other") {
        context.push(format!("Kind of action: {kind}"));
    }
    if !paths.is_empty() {
        let shown: Vec<&str> = paths.iter().take(5).map(String::as_str).collect();
        context.push(format!("Files: {}", shown.join(", ")));
    }
    context.push("The agent waits for your answer. Nothing is allowed for you.".to_string());
    let new = NewQuestion {
        asker: None,
        agent: Some(label.to_string()),
        pane_id: Some(pane_id.to_string()),
        question,
        choices: options.iter().map(|o| Choice { id: o.choice_id.clone(), label: o.name.clone() }).collect(),
        allow_free_text: Some(false),
        context: Some(cut_to(&context.join("\n"), 900)),
        timeout_s: Some(CARD_TIMEOUT_S),
        urgency: Some("high".into()),
        kind: Some(ACP_KIND_PERMISSION.to_string()),
        consent: None,
        limit: None,
    };
    (new, options)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offered(list: &[(&str, &str, &str)]) -> Vec<OptionBrief> {
        list.iter().map(|(id, name, kind)| OptionBrief { id: id.to_string(), name: name.to_string(), kind: kind.to_string() }).collect()
    }

    fn wait(options: Vec<OptionBrief>) -> PermissionWait {
        PermissionWait { ask_id: 1, tool_call_id: Some("t1".into()), title: Some("Write notes.txt".into()), tool_kind: Some("edit".into()), options, since_ms: 0 }
    }

    #[test]
    fn the_cards_choices_are_the_agents_own_options_in_its_order() {
        let options = options_of(&offered(&[("allow_once", "Allow Once", "allow_once"), ("allow_always", "Allow for Session", "allow_always"), ("reject_once", "Reject", "reject_once")]));
        let names: Vec<&str> = options.iter().map(|o| o.name.as_str()).collect();
        assert_eq!(names, ["Allow Once", "Allow for Session", "Reject"]);
        assert_eq!(options[1].option_id, "allow_always");
        assert_eq!(options[1].choice_id, "allow_always");
    }

    #[test]
    fn an_option_id_that_does_not_fit_or_repeats_gets_a_safe_choice_id_but_keeps_its_own_id_for_the_agent() {
        let long = "x".repeat(150);
        let options = options_of(&offered(&[(&long, "Long", "allow_once"), ("dup", "A", "allow_always"), ("dup", "B", "reject_once")]));
        assert_eq!(options[0].choice_id, "opt0");
        assert_eq!(options[0].option_id, long);
        assert_eq!(options[1].choice_id, "dup");
        assert_eq!(options[2].choice_id, "opt2");
        assert_eq!(options[2].option_id, "dup");
    }

    #[test]
    fn more_options_than_a_card_holds_keep_the_yes_and_no_ones() {
        let many: Vec<(String, String, String)> = (0..9).map(|n| (format!("o{n}"), format!("Other {n}"), "other".to_string())).collect();
        let mut list: Vec<(&str, &str, &str)> = many.iter().map(|(a, b, c)| (a.as_str(), b.as_str(), c.as_str())).collect();
        list.push(("no", "Reject", "reject_once"));
        list.push(("yes", "Allow", "allow_once"));
        let options = options_of(&offered(&list));
        assert_eq!(options.len(), MAX_CHOICES);
        assert!(options.iter().any(|o| o.option_id == "yes") && options.iter().any(|o| o.option_id == "no"));
    }

    #[test]
    fn the_card_names_what_the_agent_wants_and_has_no_default_answer() {
        let (new, options) = new_question(
            "pane-1",
            "Codex",
            &wait(offered(&[("allow_once", "Allow Once", "allow_once"), ("reject_once", "Reject", "reject_once")])),
            Some("Write notes.txt"),
            Some("edit"),
            &["C:\\work\\notes.txt".to_string()],
        );
        assert_eq!(new.question, "Codex asks permission: Write notes.txt");
        assert_eq!(new.pane_id.as_deref(), Some("pane-1"));
        assert_eq!(new.kind.as_deref(), Some(ACP_KIND_PERMISSION));
        assert_eq!(new.allow_free_text, Some(false));
        assert!(new.consent.is_none());
        let context = new.context.unwrap();
        assert!(context.contains("Kind of action: edit") && context.contains("notes.txt"));
        assert_eq!(new.choices.len(), 2);
        assert_eq!(options.len(), 2);
    }

    #[test]
    fn a_dismissed_card_rejects_with_the_agents_reject_option() {
        let (_, options) = new_question("p", "Codex", &wait(offered(&[("a", "Allow", "allow_once"), ("r", "No thanks", "reject_once")])), None, None, &[]);
        let card = Card { question_id: "q".into(), options, resolved: false };
        assert_eq!(card.rejection().map(|o| o.option_id.as_str()), Some("r"));
        let only_yes = Card { question_id: "q".into(), options: options_of(&offered(&[("a", "Allow", "allow_once")])), resolved: false };
        assert!(only_yes.rejection().is_none());
    }

    #[test]
    fn a_card_without_a_tool_title_still_says_who_asks() {
        let (new, _) = new_question("p", "Codex", &wait(vec![]), None, None, &[]);
        assert_eq!(new.question, "Codex asks permission to use a tool");
    }
}
