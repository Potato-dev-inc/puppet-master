//! Text Your Agents: reads the text the person sends to the bot and says what it asks for.
//!
//! Pure string work: no network, no state. Anything odd becomes `Unknown` or `TooLong`, never a panic.

/// Longest text (in characters) the person may send to a pane or to a companion.
pub const MAX_IN_CHARS: usize = 1000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// `/start` with the pairing code (if any).
    Start(Option<String>),
    Help,
    Status,
    Panes,
    /// `/to N text`: text for the pane with that number in the last `/panes` list.
    To { n: usize, text: String },
    /// `/c Name text`: text for the companion with that name.
    Companion { name: String, text: String },
    Stop,
    /// Plain text (no slash): goes to the companion, the caller decides which one.
    Text(String),
    /// The text is longer than `MAX_IN_CHARS`.
    TooLong,
    /// Not a command we know, or the arguments are wrong.
    Unknown,
}

/// Reads one message. `bot_username` (without the @) lets `/cmd@bot` work and ignores a command
/// aimed at another bot. When the bot name is not known yet, any `@suffix` is accepted.
pub fn parse(text: &str, bot_username: Option<&str>) -> Command {
    let text = text.trim();
    if text.is_empty() {
        return Command::Unknown;
    }
    if !text.starts_with('/') {
        return if too_long(text) {
            Command::TooLong
        } else {
            Command::Text(text.to_string())
        };
    }
    let (head, rest) = split_word(text);
    let Some(name) = command_name(head, bot_username) else {
        return Command::Unknown;
    };
    match name.as_str() {
        // Extra words after a command that takes none are ignored.
        "start" => Command::Start(if rest.is_empty() {
            None
        } else {
            Some(rest.to_string())
        }),
        "help" => Command::Help,
        "status" => Command::Status,
        "panes" => Command::Panes,
        "stop" => Command::Stop,
        "to" => parse_to(rest),
        "c" => parse_companion(rest),
        _ => Command::Unknown,
    }
}

/// `N text`: N is a pane number from 1 up, text is not empty and not too long.
fn parse_to(rest: &str) -> Command {
    let (number, body) = split_word(rest);
    if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
        return Command::Unknown;
    }
    let Ok(n) = number.parse::<usize>() else {
        return Command::Unknown;
    };
    if n == 0 || body.is_empty() {
        return Command::Unknown;
    }
    if too_long(body) {
        return Command::TooLong;
    }
    Command::To {
        n,
        text: body.to_string(),
    }
}

/// `Name text`: the name is one word, the text is not empty and not too long.
fn parse_companion(rest: &str) -> Command {
    let (name, body) = split_word(rest);
    if name.is_empty() || body.is_empty() {
        return Command::Unknown;
    }
    if too_long(body) {
        return Command::TooLong;
    }
    Command::Companion {
        name: name.to_string(),
        text: body.to_string(),
    }
}

/// The command name (lower case) from `/name` or `/name@bot`, or `None` for another bot's command.
fn command_name(head: &str, bot_username: Option<&str>) -> Option<String> {
    let body = head.strip_prefix('/')?;
    let (name, suffix) = match body.split_once('@') {
        Some((name, suffix)) => (name, Some(suffix)),
        None => (body, None),
    };
    if let (Some(suffix), Some(bot)) = (suffix, bot_username) {
        if !suffix.eq_ignore_ascii_case(bot) {
            return None;
        }
    }
    if name.is_empty() {
        return None;
    }
    Some(name.to_ascii_lowercase())
}

/// Splits off the first word. The rest has its ends trimmed; inner line breaks are kept.
fn split_word(s: &str) -> (&str, &str) {
    let s = s.trim_start();
    match s.find(char::is_whitespace) {
        Some(i) => (&s[..i], s[i..].trim()),
        None => (s, ""),
    }
}

fn too_long(s: &str) -> bool {
    s.chars().count() > MAX_IN_CHARS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn to(n: usize, text: &str) -> Command {
        Command::To {
            n,
            text: text.to_string(),
        }
    }

    #[test]
    fn to_accepts_bot_suffix_and_number() {
        assert_eq!(parse("/to@bot 2 hi", Some("bot")), to(2, "hi"));
        assert_eq!(parse("/TO@Bot 2 hi", Some("bot")), to(2, "hi"));
        assert_eq!(parse("/to 2 run the tests", None), to(2, "run the tests"));
    }

    #[test]
    fn to_refuses_zero_bad_numbers_and_empty_text() {
        assert_eq!(parse("/to 0 hi", None), Command::Unknown);
        assert_eq!(parse("/to x hi", None), Command::Unknown);
        assert_eq!(parse("/to -1 hi", None), Command::Unknown);
        assert_eq!(parse("/to +2 hi", None), Command::Unknown);
        assert_eq!(parse("/to 99999999999999999999999 hi", None), Command::Unknown);
        assert_eq!(parse("/to 2", None), Command::Unknown);
        assert_eq!(parse("/to", None), Command::Unknown);
    }

    #[test]
    fn to_ignores_another_bots_command() {
        assert_eq!(parse("/to@other_bot 2 hi", Some("bot")), Command::Unknown);
    }

    #[test]
    fn companion_keeps_text_after_one_name() {
        assert_eq!(
            parse("/c mochi  text", None),
            Command::Companion {
                name: "mochi".to_string(),
                text: "text".to_string()
            }
        );
        assert_eq!(parse("/c mochi", None), Command::Unknown);
        assert_eq!(parse("/c", None), Command::Unknown);
    }

    #[test]
    fn one_thousand_characters_pass_and_one_thousand_and_one_are_too_long() {
        let ok = "a".repeat(MAX_IN_CHARS);
        let long = "a".repeat(MAX_IN_CHARS + 1);
        assert_eq!(parse(&format!("/to 1 {ok}"), None), to(1, &ok));
        assert_eq!(parse(&format!("/to 1 {long}"), None), Command::TooLong);
        assert_eq!(
            parse(&format!("/c mochi {long}"), None),
            Command::TooLong
        );
        assert_eq!(parse(&long, None), Command::TooLong);
        assert_eq!(parse(&ok, None), Command::Text(ok.clone()));
    }

    #[test]
    fn length_counts_characters_not_bytes() {
        let ok = "é".repeat(MAX_IN_CHARS);
        assert_eq!(parse(&ok, None), Command::Text(ok.clone()));
    }

    #[test]
    fn plain_text_and_empty_text() {
        assert_eq!(
            parse("  is the build green?  ", None),
            Command::Text("is the build green?".to_string())
        );
        assert_eq!(parse("", None), Command::Unknown);
        assert_eq!(parse("   \n ", None), Command::Unknown);
    }

    #[test]
    fn start_takes_the_code_when_there_is_one() {
        assert_eq!(parse("/start PM7K2QX9", None), Command::Start(Some("PM7K2QX9".to_string())));
        assert_eq!(parse("/start@bot PM7K2QX9", Some("bot")), Command::Start(Some("PM7K2QX9".to_string())));
        assert_eq!(parse("/start", None), Command::Start(None));
    }

    #[test]
    fn simple_commands_and_unknown_ones() {
        assert_eq!(parse("/help", None), Command::Help);
        assert_eq!(parse("/status", None), Command::Status);
        assert_eq!(parse("/panes", None), Command::Panes);
        assert_eq!(parse("/stop", None), Command::Stop);
        assert_eq!(parse("/stop@bot", Some("bot")), Command::Stop);
        assert_eq!(parse("/Panes", None), Command::Panes);
        assert_eq!(parse("/", None), Command::Unknown);
        assert_eq!(parse("/nope", None), Command::Unknown);
        assert_eq!(parse("/@bot", Some("bot")), Command::Unknown);
    }
}
