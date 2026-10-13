//! Auto-titles: a short human title for a pane, derived from the first task it was given.
//!
//! Pure and deterministic (no LLM): sanitise the line, strip filler ("please", "can you"), keep the
//! first sentence, cap at [`MAX_TITLE_CHARS`]. [`LineCapture`] reconstructs what a person typed
//! from the raw input chunks that reach `pty::registry::write_input` (it understands backspace,
//! line kill, escape sequences and paste), and yields a title at the first submitted line that
//! has one. Nothing here looks at agent names.

/// Longest title, in characters, ellipsis included.
pub const MAX_TITLE_CHARS: usize = 40;
/// Stop growing the capture buffer past this many characters (a pasted wall of text).
const MAX_BUFFER_CHARS: usize = 600;

/// Leading filler, matched case-insensitively on word boundaries, longest first.
const LEADING_FILLER: &[&str] = &[
    "i would like you to",
    "i would like to",
    "i want you to",
    "i need you to",
    "i'd like you to",
    "go ahead and",
    "i want to",
    "i need to",
    "i'd like to",
    "we need to",
    "help me to",
    "could you",
    "would you",
    "can you",
    "will you",
    "could we",
    "can we",
    "help me",
    "kindly",
    "hello",
    "please",
    "let's",
    "lets",
    "also",
    "then",
    "okay",
    "just",
    "pls",
    "hey",
    "hi",
    "ok",
];

/// Trailing filler, longest first.
const TRAILING_FILLER: &[&str] = &["thank you", "thanks", "please", "pls", "thx"];

/// Replies that are answers to a prompt, not tasks.
const NOT_A_TASK: &[&str] = &[
    "yes", "no", "y", "n", "ok", "okay", "continue", "go", "next", "done", "test", "stop", "exit", "quit", "retry", "sure", "yep", "nope",
];

/// Remove ANSI escape sequences and control characters, fold typographic quotes, collapse
/// whitespace. Newlines and tabs become spaces.
pub fn sanitize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut state = Esc::Normal;
    for ch in text.chars() {
        if state.consume(ch) {
            continue;
        }
        match ch {
            '\u{2019}' | '\u{2018}' => out.push('\''),
            '\u{201c}' | '\u{201d}' => out.push('"'),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Escape-sequence skipper, resumable across chunks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Esc {
    #[default]
    Normal,
    /// Saw ESC.
    Esc,
    /// ESC [ ... until a final byte 0x40..=0x7e.
    Csi,
    /// ESC ] ... until BEL or ESC \.
    Osc,
    /// ESC ] ... ESC (waiting for the `\`).
    OscEsc,
    /// ESC O / ESC ( etc.: one more byte follows.
    One,
}

impl Esc {
    /// Returns true when `ch` belongs to an escape sequence (and must be skipped).
    fn consume(&mut self, ch: char) -> bool {
        match *self {
            Esc::Normal => {
                if ch == '\u{1b}' {
                    *self = Esc::Esc;
                    true
                } else {
                    false
                }
            }
            Esc::Esc => {
                *self = match ch {
                    '[' => Esc::Csi,
                    ']' => Esc::Osc,
                    'O' | '(' | ')' | '#' | '%' => Esc::One,
                    _ => Esc::Normal,
                };
                true
            }
            Esc::Csi => {
                if ('\u{40}'..='\u{7e}').contains(&ch) {
                    *self = Esc::Normal;
                }
                true
            }
            Esc::Osc => {
                if ch == '\u{7}' {
                    *self = Esc::Normal;
                } else if ch == '\u{1b}' {
                    *self = Esc::OscEsc;
                }
                true
            }
            Esc::OscEsc => {
                *self = if ch == '\\' { Esc::Normal } else { Esc::Osc };
                true
            }
            Esc::One => {
                *self = Esc::Normal;
                true
            }
        }
    }
}

fn starts_with_word(text: &str, phrase: &str) -> bool {
    let Some(head) = text.get(..phrase.len()) else { return false };
    if !head.eq_ignore_ascii_case(phrase) {
        return false;
    }
    text[phrase.len()..].chars().next().map_or(true, |c| !c.is_alphanumeric() && c != '\'')
}

fn ends_with_word(text: &str, phrase: &str) -> bool {
    if text.len() < phrase.len() || !text.is_char_boundary(text.len() - phrase.len()) {
        return false;
    }
    let (head, tail) = text.split_at(text.len() - phrase.len());
    tail.eq_ignore_ascii_case(phrase) && head.chars().next_back().map_or(true, |c| !c.is_alphanumeric())
}

fn trim_edges(text: &str) -> &str {
    text.trim_matches(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | ':' | '-' | '.' | '!' | '?' | '>' | '"' | '\'' | '`'))
}

fn looks_like_secret(text: &str) -> bool {
    !text.contains(' ') && text.chars().count() >= 20 && text.chars().any(|c| c.is_ascii_digit())
}

/// Cap at `MAX_TITLE_CHARS`, preferring a word boundary, with an ellipsis.
fn truncate(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= MAX_TITLE_CHARS {
        return text.to_string();
    }
    let keep = MAX_TITLE_CHARS - 1;
    let mut end = keep;
    if !chars[keep].is_whitespace() {
        if let Some(space) = chars[..keep].iter().rposition(|c| c.is_whitespace()) {
            if space >= MAX_TITLE_CHARS / 2 {
                end = space;
            }
        }
    }
    let cut: String = chars[..end].iter().collect();
    format!("{}\u{2026}", cut.trim_end_matches(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | ':' | '-' | '.')))
}

/// A title from one submitted line, or `None` when the line is not a task (empty, a slash
/// command, a one-word answer, something that looks like a token).
pub fn derive_title(line: &str) -> Option<String> {
    let clean = sanitize(line);
    let mut text = clean.trim();
    if text.starts_with('/') || text.starts_with('!') {
        return None;
    }
    if looks_like_secret(text) {
        return None;
    }
    // Leading filler, repeatedly ("hey, can you please ...").
    loop {
        text = text.trim_start_matches(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | ':' | '-' | '>' | '"' | '`'));
        let Some(phrase) = LEADING_FILLER.iter().find(|p| starts_with_word(text, p)) else { break };
        text = &text[phrase.len()..];
    }
    // First sentence only.
    let mut end = text.len();
    for (i, c) in text.char_indices() {
        if matches!(c, '.' | '!' | '?') && text[i + c.len_utf8()..].starts_with(' ') {
            end = i;
            break;
        }
    }
    let mut text = trim_edges(&text[..end]);
    // Trailing filler ("... please", "... thanks").
    loop {
        let Some(phrase) = TRAILING_FILLER.iter().find(|p| ends_with_word(text, p)) else { break };
        text = trim_edges(&text[..text.len() - phrase.len()]);
    }
    if text.chars().filter(|c| c.is_alphanumeric()).count() < 3 {
        return None;
    }
    if NOT_A_TASK.iter().any(|w| text.eq_ignore_ascii_case(w)) {
        return None;
    }
    Some(truncate(text))
}

/// Rebuilds the line a person is typing from raw input chunks.
#[derive(Debug, Default)]
pub struct LineCapture {
    buf: Vec<char>,
    esc: Esc,
}

impl LineCapture {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one input chunk. `append_newline` means the writer also presses Enter after `text`.
    /// Returns the title as soon as a submitted line has one; lines that yield none (empty,
    /// "y", "/clear") are dropped and capture continues.
    pub fn feed(&mut self, text: &str, append_newline: bool) -> Option<String> {
        for ch in text.chars() {
            if let Some(title) = self.key(ch) {
                return Some(title);
            }
        }
        if append_newline {
            return self.submit();
        }
        None
    }

    fn key(&mut self, ch: char) -> Option<String> {
        if self.esc.consume(ch) {
            return None;
        }
        match ch {
            '\r' | '\n' => return self.submit(),
            '\u{7f}' | '\u{8}' => {
                self.buf.pop();
            }
            // Ctrl-U (kill line) and Ctrl-C (abandon).
            '\u{15}' | '\u{3}' => self.buf.clear(),
            // Ctrl-W: delete the last word.
            '\u{17}' => {
                while self.buf.last().is_some_and(|c| c.is_whitespace()) {
                    self.buf.pop();
                }
                while self.buf.last().is_some_and(|c| !c.is_whitespace()) {
                    self.buf.pop();
                }
            }
            '\t' => self.push(' '),
            c if c.is_control() => {}
            c => self.push(c),
        }
        None
    }

    fn push(&mut self, c: char) {
        if self.buf.len() < MAX_BUFFER_CHARS {
            self.buf.push(c);
        }
    }

    fn submit(&mut self) -> Option<String> {
        let line: String = self.buf.drain(..).collect();
        derive_title(&line)
    }
}
