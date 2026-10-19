//! The screen WITH its cell attributes, for the safe answerer (SPEC 11.20).
//!
//! The wizard reads a pane's screen as text. Many TUIs show which option of a menu is highlighted with a colour, a
//! background or inverse video and draw NO marker (`>` or `❯`) at all, so the text alone says nothing about where the
//! cursor is. `StyledScreen` keeps, for every row of the pane's emulator grid, the row's text and the attributes of each
//! character (inverse, bold, dim, underline, foreground and background colour), as runs so a 120x40 grid stays small.
//!
//! It is built from the emulator's grid (`StyledScreen::from_screen`) and carries no pane, no clock and no AI. The
//! text of a row is exactly what the emulator would print for it with trailing blanks cut; a wide character counts once.
//! Tests make one from raw terminal bytes (`StyledScreen::from_ansi`), so a screen with an inverse row is a string.

use vt100::Color;

/// How a run of characters is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellStyle {
    pub inverse: bool,
    pub bold: bool,
    pub dim: bool,
    pub underline: bool,
    pub fg: Color,
    pub bg: Color,
}

impl Default for CellStyle {
    fn default() -> Self {
        Self { inverse: false, bold: false, dim: false, underline: false, fg: Color::Default, bg: Color::Default }
    }
}

impl CellStyle {
    /// The plain look: no inverse, no background, no bold, default colour.
    pub fn is_plain(&self) -> bool {
        *self == CellStyle::default()
    }

    /// Drawn as a filled band: inverse video or a background colour. The "selected row" of most menus.
    pub fn is_band(&self) -> bool {
        self.inverse || self.bg != Color::Default
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyledRow {
    /// The row's text, trailing blanks cut.
    pub text: String,
    /// `(first char index, style)`: the style holds until the next run starts. Always starts at 0 when `text` is not empty.
    runs: Vec<(usize, CellStyle)>,
}

impl StyledRow {
    pub fn new(text: &str, runs: Vec<(usize, CellStyle)>) -> Self {
        Self { text: text.to_string(), runs }
    }

    /// The style of the character at char index `at`.
    pub fn style_at(&self, at: usize) -> CellStyle {
        let mut style = CellStyle::default();
        for (start, s) in &self.runs {
            if *start > at {
                break;
            }
            style = *s;
        }
        style
    }

    /// The style most of the characters in `from..to` (char indexes, blanks not counted) are drawn in.
    pub fn style_of(&self, from: usize, to: usize) -> CellStyle {
        let chars: Vec<char> = self.text.chars().collect();
        let to = to.min(chars.len());
        let mut counts: Vec<(CellStyle, usize)> = Vec::new();
        for i in from..to {
            if chars[i].is_whitespace() {
                continue;
            }
            let style = self.style_at(i);
            match counts.iter_mut().find(|(s, _)| *s == style) {
                Some((_, n)) => *n += 1,
                None => counts.push((style, 1)),
            }
        }
        counts.into_iter().max_by_key(|(_, n)| *n).map(|(s, _)| s).unwrap_or_default()
    }

    /// Is the blank at char index `at` (or beyond the text) drawn as part of a band? A selected row is usually painted
    /// across the whole row, past its last letter.
    pub fn band_at(&self, at: usize) -> bool {
        self.style_at(at).is_band()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StyledScreen {
    pub rows: Vec<StyledRow>,
}

impl StyledScreen {
    /// Every row of the emulator's grid.
    pub fn from_screen(screen: &vt100::Screen) -> Self {
        let (rows, cols) = screen.size();
        let mut out = Vec::with_capacity(rows as usize);
        for r in 0..rows {
            let mut text = String::new();
            let mut runs: Vec<(usize, CellStyle)> = Vec::new();
            let mut chars = 0usize;
            // where the last visible (non-blank) character ended, in chars: the run list is cut there too, except that a
            // painted band keeps its blanks (the caller asks `band_at` for them)
            for c in 0..cols {
                let Some(cell) = screen.cell(r, c) else { break };
                if cell.is_wide_continuation() {
                    continue;
                }
                let style = CellStyle {
                    inverse: cell.inverse(),
                    bold: cell.bold(),
                    dim: cell.dim(),
                    underline: cell.underline(),
                    fg: cell.fgcolor(),
                    bg: cell.bgcolor(),
                };
                let contents = cell.contents();
                let shown: &str = if contents.is_empty() { " " } else { contents };
                let n = shown.chars().count();
                if runs.last().map(|(_, s)| *s) != Some(style) {
                    runs.push((chars, style));
                }
                text.push_str(shown);
                chars += n;
            }
            // cut trailing blanks from the text; keep the runs (a band continues over the blanks)
            let kept = text.trim_end().chars().count();
            let trimmed: String = text.chars().take(kept).collect();
            out.push(StyledRow { text: trimmed, runs });
        }
        Self { rows: out }
    }

    /// A screen made from raw terminal bytes at `cols` x `rows`: the tests' way to draw an inverse row.
    pub fn from_ansi(rows: u16, cols: u16, bytes: &str) -> Self {
        let mut parser = vt100::Parser::new(rows, cols, 0);
        parser.process(bytes.replace('\n', "\r\n").as_bytes());
        Self::from_screen(parser.screen())
    }

    pub fn text(&self) -> String {
        self.rows.iter().map(|r| r.text.as_str()).collect::<Vec<_>>().join("\n")
    }

    /// The last row whose text, with surrounding blanks and a box border cut, equals `text` (cut the same way).
    pub fn row_like(&self, text: &str) -> Option<&StyledRow> {
        let wanted = text.trim();
        self.rows.iter().rev().find(|r| r.text.trim() == wanted)
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_inverse_row_has_a_band_and_the_text_is_plain() {
        let s = StyledScreen::from_ansi(6, 40, "Allow it?\n\x1b[7m Yes, allow once        \x1b[0m\n No\n");
        assert_eq!(s.text().lines().nth(1).unwrap(), " Yes, allow once");
        let yes = &s.rows[1];
        assert!(yes.style_of(1, 16).inverse);
        assert!(yes.band_at(20), "the band goes on past the letters");
        assert!(!s.rows[2].style_of(1, 3).is_band());
    }

    #[test]
    fn colours_and_bold_are_kept_per_run() {
        let s = StyledScreen::from_ansi(3, 30, "\x1b[1;36mYes\x1b[0m  No");
        let row = &s.rows[0];
        assert!(row.style_at(0).bold);
        assert_eq!(row.style_at(0).fg, Color::Idx(6));
        assert!(row.style_at(6).is_plain());
    }

    #[test]
    fn a_wide_character_counts_once() {
        let s = StyledScreen::from_ansi(2, 20, "\u{4e2d}\u{6587} ok");
        assert_eq!(s.rows[0].text, "\u{4e2d}\u{6587} ok");
    }
}
