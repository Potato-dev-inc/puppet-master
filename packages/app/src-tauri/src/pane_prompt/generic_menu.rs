//! A question nobody's profile describes, recognised by its SHAPE.
//!
//! A screen-read agent waits on an interactive menu (an arrow-key or numbered yes / no / allow menu, a `[y/n]` line)
//! that none of its `[[tui.prompt]]` tables matches. Without this the run stops with `manual_input_required` and no
//! prompt id, and a lead has to take over the pane to press one key. Here the menu is read the way the Agent Wizard
//! reads a question in its own pane (`wizard::visible::answer::parse_menu`) and published as a card:
//!
//! - kind `unknown_question`, `recognised_by: "shape"`, and a summary that says so: it is not a profile's reading;
//! - one choice per option, in the order shown: id `option_<n>`, the label as the screen words it, the keys that
//!   would pick it, which row is highlighted;
//! - an id made of the question and the options (item 1: it survives redraws).
//!
//! Nothing here presses a key. A caller picks an option with `answer_prompt` (by id, number or label), through the
//! same guards as every answer: the pane must be waiting (never a busy pane), a stale id types nothing, and an option
//! that says always / don't ask again / for this session needs the explicit broad opt-in. Sign-in, payment, model and
//! telemetry questions are not published at all: those are the person's.

use serde_json::{json, Value};

use crate::tui::declarative::wizard::visible::answer::{classify_option, off_limits, parse_menu, Menu, MenuKind, OptionClass};

/// The key text for one Down / Up press (what the answer route types before its own Enter).
const DOWN: &str = "\u{1b}[B";
const UP: &str = "\u{1b}[A";

/// Most options a menu card shows.
const MAX_OPTIONS: usize = 9;

fn alnum(text: &str) -> String {
    text.chars().filter(|ch| ch.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// The last non-empty rows above the first option: the question being asked.
fn question_rows(screen: &str, first_option: &str) -> Vec<String> {
    let rows: Vec<&str> = screen.lines().map(str::trim_end).filter(|r| !r.trim().is_empty()).collect();
    let at = rows.iter().rposition(|r| r.contains(first_option.trim()));
    let end = at.unwrap_or(rows.len());
    rows[..end].iter().rev().take(3).rev().map(|r| r.trim().to_string()).collect()
}

/// The card for the menu in the last rows of `screen`, or `None` when there is no plain menu, the question is one
/// only the person answers, or the pane is busy. `status` is the pane's observed status.
///
/// A pane that says it is `waiting_input` (the shared screen heuristics saw a question) gets the card for any menu
/// with a cursor, a `[y/n]` line or a row that asks. A menu those heuristics do not know (`Write hello.txt?` with
/// `Yes` / `No` rows) leaves the pane `running` and then, once the screen holds still, `idle`: an `idle` pane gets a
/// card only for a menu that is plainly an approval (a row that asks, a plain yes and a plain no among the options),
/// so a list in a finished reply is never taken for one. A `running` pane never gets one.
pub(crate) fn unknown_question(agent_type: &str, screen: &str, status: Option<&str>) -> Option<Value> {
    let strict = match status {
        Some("waiting_input") => false,
        Some("idle") => true,
        _ => return None,
    };
    if off_limits(screen).is_some() {
        return None;
    }
    let menu = parse_menu(screen)?;
    let options = menu_options(&menu);
    if options.is_empty() || options.len() > MAX_OPTIONS {
        return None;
    }
    let first = options.first().map(|o| o.label.clone()).unwrap_or_default();
    let question = question_rows(screen, &first);
    // A plain list of rows is not a question: it needs a cursor, a [y/n] line, or a row that asks something.
    let asks = question.iter().any(|r| r.contains('?'));
    let has_cursor = menu.highlighted().is_some();
    if menu.kind != MenuKind::YesNo && !asks && !has_cursor {
        return None;
    }
    if strict {
        let says = |yes: bool| {
            options.iter().any(|o| {
                let l = o.label.to_ascii_lowercase();
                if yes {
                    classify_option(&o.label) == OptionClass::SafeYes || l.starts_with("yes") || l.starts_with("allow")
                } else {
                    ["no", "deny", "reject", "cancel", "decline", "skip"].iter().any(|w| l == *w || l.starts_with(&format!("{w} ")) || l.starts_with(&format!("{w},")))
                }
            })
        };
        if !asks || !says(true) || !says(false) || !menu_is_last(screen, &options) {
            return None;
        }
    }
    let kind_word = match menu.kind {
        MenuKind::Arrow => "an arrow-key menu",
        MenuKind::Numbered => "a numbered menu",
        MenuKind::YesNo => "a yes / no question",
    };
    let choices: Vec<String> = options.iter().map(|o| o.id.clone()).collect();
    let labels: serde_json::Map<String, Value> = options.iter().map(|o| (o.id.clone(), Value::String(o.label.clone()))).collect();
    let shown: Vec<Value> = options
        .iter()
        .map(|o| json!({"id": o.id, "label": o.label, "number": o.number, "highlighted": o.highlighted, "broad": o.broad}))
        .collect();
    let identity = format!(
        "unknown_question|{}|{}",
        question.iter().map(|r| alnum(r)).collect::<String>(),
        options.iter().map(|o| alnum(&o.label)).collect::<Vec<_>>().join("|")
    );
    let text = format!("{}\n{}", question.join("\n"), options.iter().map(|o| o.label.as_str()).collect::<Vec<_>>().join("\n"));
    Some(json!({
        "agent_type": agent_type,
        "kind": "unknown_question",
        "text": text,
        "choices": choices,
        "labels": labels,
        "options": shown,
        "menu": match menu.kind { MenuKind::Arrow => "arrow", MenuKind::Numbered => "numbered", MenuKind::YesNo => "yes_no" },
        "recognised_by": "shape",
        "identity": identity,
        "summary": format!(
            "{} (recognised by shape, not by profile: {kind_word}; this agent's profile has no [[tui.prompt]] for it)",
            question.last().map(String::as_str).unwrap_or("The agent is asking a question")
        ),
    }))
}

/// The menu is the last thing on the screen: after its last option there are at most two rows and they are hints
/// (`up/down to move, enter to confirm`). A menu with an input box or more text under it was answered long ago
/// (its rows are still in the transcript) or is part of a reply.
fn menu_is_last(screen: &str, options: &[Option_]) -> bool {
    let rows: Vec<&str> = screen.lines().map(str::trim).filter(|r| !r.is_empty()).collect();
    let Some(last_option) = options.last().map(|o| o.label.as_str()) else { return false };
    let Some(at) = rows.iter().rposition(|r| r.contains(last_option)) else { return true };
    let after = &rows[at + 1..];
    after.len() <= 2
        && after.iter().all(|r| {
            let l = r.to_ascii_lowercase();
            ["enter", "esc", "navigate", "select", "confirm", "move", "arrow", "\u{2191}", "\u{2193}"].iter().any(|w| l.contains(w))
        })
}

struct Option_ {
    id: String,
    label: String,
    number: Option<u32>,
    highlighted: bool,
    broad: bool,
}

fn menu_options(menu: &Menu) -> Vec<Option_> {
    if menu.kind == MenuKind::YesNo {
        return vec![
            Option_ { id: "option_1".into(), label: "Yes".into(), number: None, highlighted: false, broad: false },
            Option_ { id: "option_2".into(), label: "No".into(), number: None, highlighted: false, broad: false },
        ];
    }
    menu.options
        .iter()
        .enumerate()
        .map(|(i, o)| Option_ {
            id: format!("option_{}", i + 1),
            label: o.text.clone(),
            number: o.number,
            highlighted: o.highlighted,
            broad: classify_option(&o.text) == OptionClass::Risky,
        })
        .collect()
}

/// Which option a caller's `choice` names: `option_2`, `2`, or the label (case and punctuation aside, or its start).
fn pick(prompt: &Value, choice: &str) -> Result<usize, String> {
    let options = prompt.get("options").and_then(Value::as_array).ok_or("this question has no options")?;
    let wanted = choice.trim();
    if let Some(at) = options.iter().position(|o| o["id"].as_str() == Some(wanted)) {
        return Ok(at);
    }
    if let Ok(n) = wanted.parse::<usize>() {
        if (1..=options.len()).contains(&n) {
            return Ok(n - 1);
        }
    }
    let want = alnum(wanted);
    if want.is_empty() {
        return Err("no option was named".into());
    }
    // The words every other card uses: the one plain "yes, this once" option, or the one plain "no".
    let label_of = |i: usize| options[i]["label"].as_str().unwrap_or_default().to_string();
    let named = match want.as_str() {
        "allowonce" | "once" | "yes" | "allow" => Some((0..options.len()).filter(|i| classify_option(&label_of(*i)) == OptionClass::SafeYes).collect::<Vec<_>>()),
        "deny" | "reject" | "no" => Some(
            (0..options.len())
                .filter(|i| {
                    let l = label_of(*i).to_ascii_lowercase();
                    ["no", "deny", "reject", "cancel", "decline", "skip", "don't", "do not"].iter().any(|w| l == *w || l.starts_with(&format!("{w} ")) || l.starts_with(&format!("{w},")))
                })
                .collect(),
        ),
        _ => None,
    };
    match named.as_deref() {
        Some([one]) => return Ok(*one),
        Some([]) => return Err(format!("no option plainly means {wanted:?}; the options are {}", labels_of(options))),
        Some(_) => return Err(format!("more than one option could mean {wanted:?}; name one: {}", labels_of(options))),
        None => {}
    }
    let exact: Vec<usize> = (0..options.len()).filter(|i| alnum(options[*i]["label"].as_str().unwrap_or_default()) == want).collect();
    if let [one] = exact.as_slice() {
        return Ok(*one);
    }
    let starts: Vec<usize> = (0..options.len()).filter(|i| alnum(options[*i]["label"].as_str().unwrap_or_default()).starts_with(&want)).collect();
    match starts.as_slice() {
        [one] => Ok(*one),
        [] => Err(format!("no option is called {wanted:?}; the options are {}", labels_of(options))),
        _ => Err(format!("{wanted:?} could be more than one option; the options are {}", labels_of(options))),
    }
}

fn labels_of(options: &[Value]) -> String {
    options.iter().map(|o| format!("{} ({:?})", o["id"].as_str().unwrap_or(""), o["label"].as_str().unwrap_or(""))).collect::<Vec<_>>().join(", ")
}

/// The reply that picks `choice`: the text the answer route types before its own Enter. An option that is a broad
/// approval (always / don't ask again / for this session) needs `allow_broad`.
pub(crate) fn reply(prompt: &Value, choice: &str, allow_broad: bool) -> Result<crate::tui::Reply, String> {
    let at = pick(prompt, choice)?;
    let options = prompt.get("options").and_then(Value::as_array).ok_or("this question has no options")?;
    let option = &options[at];
    if option["broad"].as_bool() == Some(true) && !allow_broad {
        return Err(format!("{:?} is a broad approval; it needs allow_broad", option["label"].as_str().unwrap_or("")));
    }
    let menu = prompt.get("menu").and_then(Value::as_str).unwrap_or_default();
    let highlighted = options.iter().position(|o| o["highlighted"].as_bool() == Some(true));
    let text = match (menu, highlighted) {
        ("yes_no", _) => if at == 0 { "y".to_string() } else { "n".to_string() },
        // A cursor row: move it to the option; the route's own Enter confirms.
        (_, Some(from)) => {
            if at >= from {
                DOWN.repeat(at - from)
            } else {
                UP.repeat(from - at)
            }
        }
        // Numbered, no cursor: the number selects.
        _ => option["number"].as_u64().map(|n| n.to_string()).unwrap_or_else(|| (at + 1).to_string()),
    };
    Ok(crate::tui::Reply { key: Some(text), text: None })
}

#[cfg(test)]
mod tests;
