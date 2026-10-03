//! Prompt shapes of each shell, matched against the last non-empty screen line.

use crate::pty::ansi::strip_ansi;
use once_cell::sync::Lazy;
use regex::Regex;

static POWERSHELL_PROMPT: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)^PS\s+.+>\s*$").expect("powershell prompt regex"));
static CMD_PROMPT: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)^[a-z]:\\.*>\s*$").expect("cmd prompt regex"));
static BASH_PROMPT: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?:^|\s)[^\s]*[$#]\s*$").expect("bash prompt regex"));

/// True when the last non-empty line of `screen` matches `prompt`.
fn prompt_on_last_line(screen: &str, prompt: &Regex) -> bool {
    let clean = strip_ansi(screen);
    let line = clean
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .trim();
    !line.is_empty() && prompt.is_match(line)
}

pub(super) fn powershell_visible(screen: &str) -> bool {
    prompt_on_last_line(screen, &POWERSHELL_PROMPT)
}

pub(super) fn cmd_visible(screen: &str) -> bool {
    prompt_on_last_line(screen, &CMD_PROMPT)
}

pub(super) fn bash_visible(screen: &str) -> bool {
    prompt_on_last_line(screen, &BASH_PROMPT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_shell_recognises_only_its_own_prompt() {
        assert!(powershell_visible("PS C:\\repo>"));
        assert!(!powershell_visible("C:\\repo>"));
        assert!(cmd_visible("C:\\repo>"));
        assert!(bash_visible("user@host:~/repo$"));
        assert!(!bash_visible("Composing changes..."));
    }
}
