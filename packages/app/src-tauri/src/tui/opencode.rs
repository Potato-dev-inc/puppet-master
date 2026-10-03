//! OpenCode: the terminal TUI (`opencode`) and the same binary driven over its HTTP API
//! (`opencode_native`). The API client itself lives in `crate::opencode`; this is only the
//! TUI-facing profile.

use super::{Features, Kind, TuiProfile};
use crate::pty::ansi::strip_ansi;
use once_cell::sync::Lazy;
use regex::Regex;

/// OpenCode TUI menus (numbered yes/no, submit hints).
static MENU_PATTERNS: Lazy<Vec<Regex>> = Lazy::new(|| {
    [
        r"(?i)enter submit",
        r"(?i)esc dismiss",
        r"(?i)↑↓ select",
        r"(?i)\d+\.\s*yes\b",
        r"(?i)\d+\.\s*no\b",
        r"(?i)type your own answer",
    ]
    .into_iter()
    .map(|p| Regex::new(p).expect("valid opencode tui regex"))
    .collect()
});

pub struct OpenCode;

impl TuiProfile for OpenCode {
    fn id(&self) -> &'static str {
        "opencode"
    }

    fn looks_like_menu(&self, text: &str) -> bool {
        looks_like_menu(text)
    }
}

pub struct OpenCodeNative;

impl TuiProfile for OpenCodeNative {
    fn id(&self) -> &'static str {
        "opencode_native"
    }

    fn features(&self) -> Features {
        Features {
            kind: Kind::NativeProtocol,
            ..Features::CLI
        }
    }

    fn looks_like_menu(&self, text: &str) -> bool {
        looks_like_menu(text)
    }
}

fn looks_like_menu(text: &str) -> bool {
    let clean = strip_ansi(text);
    let tail: String = clean
        .lines()
        .filter(|l| !l.trim().is_empty())
        .rev()
        .take(12)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n");
    MENU_PATTERNS.iter().any(|re| re.is_match(&tail))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_numbered_yes_no_menu() {
        let text = "Permission required\n  1. Yes\n  2. No\n";
        assert!(OpenCode.looks_like_menu(text));
        assert!(!OpenCode.looks_like_menu("just some output"));
    }
}
