//! Strip ANSI/VT escape sequences from PTY scrollback for LLM-readable output.

use once_cell::sync::Lazy;
use regex::Regex;

static ANSI_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\x1B(?:[@-Z\\-_]|\[[0-?]*[ -/]*[@-~])").expect("ansi regex"));

/// Remove CSI/OSC/etc. escape sequences and collapse noisy TUI redraw whitespace.
pub fn strip_ansi(raw: &str) -> String {
    let no_esc = ANSI_RE.replace_all(raw, "");
    let mut out = String::with_capacity(no_esc.len());
    let mut previous = String::new();
    for line in no_esc.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed == previous {
            continue;
        }
        out.push_str(trimmed);
        out.push('\n');
        previous.clear();
        previous.push_str(trimmed);
    }
    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_csi_clear_line() {
        let raw = "\x1b[Khello\x1b[0m\nworld\n";
        assert_eq!(strip_ansi(raw), "hello\nworld");
    }

    #[test]
    fn collapses_consecutive_tui_redraw_lines() {
        let raw = "Working\n\x1b[2KWorking\nDone\nDone\n";
        assert_eq!(strip_ansi(raw), "Working\nDone");
    }
}
