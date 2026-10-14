//! Prompt shapes of each shell, matched against the last non-empty screen line.

use crate::pty::ansi::strip_ansi;
use crate::tui::StandbyScreen;
use once_cell::sync::Lazy;
use regex::Regex;

static POWERSHELL_PROMPT: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)^PS\s+.+>\s*$").expect("powershell prompt regex"));
static CMD_PROMPT: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)^[a-z]:\\.*>\s*$").expect("cmd prompt regex"));
static BASH_PROMPT: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?:^|\s)[^\s]*[$#]\s*$").expect("bash prompt regex"));

// Standby readiness: the bare prompt and nothing typed after it. A path cannot contain `>` on
// Windows, so `[^>]*` stops at the prompt's own `>` and any typed text after it fails the match.
static POWERSHELL_READY: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)^PS [^>]*>$").expect("powershell ready regex"));
static CMD_READY: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)^[a-z]:\\[^>]*>$").expect("cmd ready regex"));
/// `user@host:~$`, `user@host:/path#`, or a bare `$` / `#`.
static BASH_READY: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^(?:[^\s@]+@[^\s:]+:\S*)?[$#]$").expect("bash ready regex"));

/// The last non-empty line of `screen`, ANSI-stripped and trimmed (empty when there is none).
fn last_line(screen: &str) -> String {
    let clean = strip_ansi(screen);
    clean
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .trim()
        .to_string()
}

/// True when the last non-empty line of `screen` matches `prompt`.
fn prompt_on_last_line(screen: &str, prompt: &Regex) -> bool {
    let line = last_line(screen);
    !line.is_empty() && prompt.is_match(&line)
}

/// `Ready` when the last line is exactly this shell's bare prompt; `NotReady` otherwise (still
/// starting, output on the last line, or text already typed after the prompt).
fn ready_on_last_line(screen: &str, ready: &Regex) -> StandbyScreen {
    if prompt_on_last_line(screen, ready) {
        StandbyScreen::Ready
    } else {
        StandbyScreen::NotReady
    }
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

/// A bare shell prompt as the last line, in the shapes a pane's host shell prints when the agent it
/// launched has exited and given the terminal back: `PS C:\repo>`, `C:\repo>`, `user@host:~/repo$`.
/// Stricter than `*_visible`: nothing typed after it, and a lone `$` or `#` does not count (an agent
/// screen can end in one).
pub(crate) fn host_prompt_visible(screen: &str) -> bool {
    static BASH_HOST: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"^[^\s@]+@[^\s:]+:\S*[$#]$").expect("bash host prompt regex"));
    prompt_on_last_line(screen, &POWERSHELL_READY)
        || prompt_on_last_line(screen, &CMD_READY)
        || prompt_on_last_line(screen, &BASH_HOST)
}

pub(super) fn powershell_ready(screen: &str) -> StandbyScreen {
    ready_on_last_line(screen, &POWERSHELL_READY)
}

pub(super) fn cmd_ready(screen: &str) -> StandbyScreen {
    ready_on_last_line(screen, &CMD_READY)
}

pub(super) fn bash_ready(screen: &str) -> StandbyScreen {
    ready_on_last_line(screen, &BASH_READY)
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

    #[test]
    fn a_host_prompt_is_a_bare_prompt_in_one_of_the_three_shapes() {
        assert!(host_prompt_visible("some dialog text\nPS D:\\pm-r7-trust-a> "));
        assert!(host_prompt_visible("C:\\repo>"));
        assert!(host_prompt_visible("user@host:~/repo$"));
        // Not prompts: typed commands, agent screens that end in `$`/`#`, ordinary output.
        assert!(!host_prompt_visible("PS D:\\repo> npm test"));
        assert!(!host_prompt_visible("PS D:\\repo>\nRunning tests..."));
        assert!(!host_prompt_visible("## Heading\n#"));
        assert!(!host_prompt_visible("price: 5$"));
        assert!(!host_prompt_visible("\u{276f} \n  ? for shortcuts"));
        assert!(!host_prompt_visible(""));
    }

    #[test]
    fn cmd_is_ready_only_at_a_bare_prompt() {
        assert_eq!(cmd_ready("Microsoft Windows [Version 10.0]\r\n\r\nC:\\Users\\me>"), StandbyScreen::Ready);
        assert_eq!(cmd_ready("C:\\repo>  \n\n"), StandbyScreen::Ready, "trailing blank rows are not content");
        assert_eq!(cmd_ready("C:\\repo>dir"), StandbyScreen::NotReady, "a typed command is not an empty prompt");
        assert_eq!(cmd_ready("C:\\repo>echo hi\r\nhi\r\nC:\\repo>dir"), StandbyScreen::NotReady);
        assert_eq!(cmd_ready("C:\\repo> dir >"), StandbyScreen::NotReady, "typed text with a redirect");
        assert_eq!(cmd_ready("Starting..."), StandbyScreen::NotReady);
        assert_eq!(cmd_ready(""), StandbyScreen::NotReady);
        assert_eq!(cmd_ready("PS C:\\repo>"), StandbyScreen::NotReady, "a PowerShell prompt is not cmd");
    }

    #[test]
    fn powershell_is_ready_only_at_a_bare_prompt() {
        assert_eq!(powershell_ready("Windows PowerShell\r\nPS C:\\repo> "), StandbyScreen::Ready);
        assert_eq!(powershell_ready("PS C:\\Users\\me>"), StandbyScreen::Ready);
        assert_eq!(powershell_ready("PS C:\\repo> Get-Date"), StandbyScreen::NotReady);
        assert_eq!(powershell_ready("PS C:\\repo> Get-Item > out"), StandbyScreen::NotReady);
        assert_eq!(powershell_ready("C:\\repo>"), StandbyScreen::NotReady);
        assert_eq!(powershell_ready("Loading personal and system profiles..."), StandbyScreen::NotReady);
    }

    #[test]
    fn bash_is_ready_only_at_a_bare_prompt() {
        assert_eq!(bash_ready("Last login: Mon\r\nuser@host:~$ "), StandbyScreen::Ready);
        assert_eq!(bash_ready("user@host:/srv/app#"), StandbyScreen::Ready, "root prompt");
        assert_eq!(bash_ready("$ "), StandbyScreen::Ready);
        assert_eq!(bash_ready("user@host:~$ ls -la"), StandbyScreen::NotReady);
        assert_eq!(bash_ready("user@host:~$ echo $"), StandbyScreen::NotReady);
        assert_eq!(bash_ready("Composing changes..."), StandbyScreen::NotReady);
        assert_eq!(bash_ready("\u{1b}[32muser@host:~$\u{1b}[0m "), StandbyScreen::Ready, "colour codes are stripped");
    }
}
