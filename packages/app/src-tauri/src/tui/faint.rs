//! Reading a terminal screen without some of its cells.
//!
//! An agent's input box can hold text the agent drew itself (a placeholder, a suggestion of the next
//! prompt) in a faint style; only the cell attributes tell it from what the user typed. A profile
//! that has such text builds the screen text it hands to readiness with [`contents_without`].

/// The screen's text exactly as `vt100::Screen::contents` writes it (rows joined, a soft-wrapped row
/// glued to the next one, trailing blank rows dropped), except that a cell for which `leave_out`
/// answers true counts as empty. `leave_out(row, col, cell)`.
pub fn contents_without(screen: &vt100::Screen, leave_out: impl Fn(u16, u16, &vt100::Cell) -> bool) -> String {
    let (rows, cols) = screen.size();
    let mut contents = String::new();
    let mut wrapping = false;
    for row in 0..rows {
        let mut prev_col = 0u16;
        let mut prev_was_wide = false;
        for col in 0..cols {
            let Some(cell) = screen.cell(row, col) else { continue };
            if prev_was_wide {
                prev_was_wide = false;
                continue;
            }
            prev_was_wide = cell.is_wide();
            if cell.has_contents() && !leave_out(row, col, cell) {
                for _ in 0..(col - prev_col) {
                    contents.push(' ');
                }
                prev_col = col;
                contents.push_str(cell.contents());
                prev_col += if cell.is_wide() { 2 } else { 1 };
            }
        }
        if prev_col == 0 && wrapping {
            contents.push('\n');
        }
        let wrapped = screen.row_wrapped(row);
        if !wrapped {
            contents.push('\n');
        }
        wrapping = wrapped;
    }
    while contents.ends_with('\n') {
        contents.pop();
    }
    contents
}

#[cfg(test)]
mod tests {
    use super::contents_without;

    fn parse(bytes: &str, rows: u16, cols: u16) -> vt100::Parser {
        let mut parser = vt100::Parser::new(rows, cols, 0);
        parser.process(bytes.as_bytes());
        parser
    }

    #[test]
    fn leaving_nothing_out_is_exactly_contents() {
        for bytes in [
            "plain\r\nsecond row\r\n\r\n\u{1b}[2mfaint\u{1b}[m tail",
            "a very long line that wraps around the narrow emulator and then continues on\r\nnext",
            "wide \u{4e2d}\u{6587} cells \u{276f} and gaps\u{1b}[5Cafter",
            "",
        ] {
            let parser = parse(bytes, 6, 20);
            assert_eq!(contents_without(parser.screen(), |_, _, _| false), parser.screen().contents(), "{bytes:?}");
        }
    }

    #[test]
    fn faint_cells_can_be_left_out_and_the_rest_keeps_its_columns() {
        let parser = parse("\u{276f} \u{1b}[2mcommit this\u{1b}[m\r\nunder", 4, 30);
        let text = contents_without(parser.screen(), |_, _, cell| cell.dim());
        assert_eq!(text, "\u{276f} \nunder");
        let typed = parse("\u{276f} typed \u{1b}[2mghost\u{1b}[m", 4, 30);
        assert_eq!(contents_without(typed.screen(), |_, _, cell| cell.dim()), "\u{276f} typed ");
    }
}
