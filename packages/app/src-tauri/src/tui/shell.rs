//! Plain shells (`cmd`, `powershell`, `bash`): no agent, so "idle" means the prompt is showing.

use super::{Features, Kind, TuiProfile};
use crate::pty::ansi::strip_ansi;
use once_cell::sync::Lazy;
use regex::Regex;

static POWERSHELL_PROMPT: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)^PS\s+.+>\s*$").expect("powershell prompt regex"));
static CMD_PROMPT: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)^[a-z]:\\.*>\s*$").expect("cmd prompt regex"));
static BASH_PROMPT: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?:^|\s)[^\s]*[$#]\s*$").expect("bash prompt regex"));

const FEATURES: Features = Features {
    kind: Kind::Shell,
    dispatchable: false,
    read_only_headless: false,
    resumable_headless: false,
};

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

pub struct Cmd;

impl TuiProfile for Cmd {
    fn id(&self) -> &'static str {
        "cmd"
    }
    fn features(&self) -> Features {
        FEATURES
    }
    fn prompt_visible(&self, screen: &str) -> bool {
        prompt_on_last_line(screen, &CMD_PROMPT)
    }
}

pub struct Powershell;

impl TuiProfile for Powershell {
    fn id(&self) -> &'static str {
        "powershell"
    }
    fn features(&self) -> Features {
        FEATURES
    }
    fn prompt_visible(&self, screen: &str) -> bool {
        prompt_on_last_line(screen, &POWERSHELL_PROMPT)
    }
}

pub struct Bash;

impl TuiProfile for Bash {
    fn id(&self) -> &'static str {
        "bash"
    }
    fn features(&self) -> Features {
        FEATURES
    }
    fn prompt_visible(&self, screen: &str) -> bool {
        prompt_on_last_line(screen, &BASH_PROMPT)
    }
}
