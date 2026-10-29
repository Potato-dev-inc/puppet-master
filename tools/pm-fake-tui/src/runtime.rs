//! The real loop: console input -> keys -> engine -> frames -> console output.

use crate::engine::{strip, Engine};
use crate::keys::KeyParser;
use crate::render::Frame;
use crate::script::Script;
use crate::term::{self, Console};
use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

/// Paints frames as the minimum rows that changed, like a real TUI.
struct Painter {
    last: Vec<String>,
    last_cursor: Option<(usize, usize)>,
    size: (usize, usize),
    started: bool,
}

impl Painter {
    fn new() -> Painter {
        Painter { last: Vec::new(), last_cursor: None, size: (0, 0), started: false }
    }

    fn write(bytes: &str) {
        let mut out = std::io::stdout().lock();
        let _ = out.write_all(bytes.as_bytes());
        let _ = out.flush();
    }

    fn draw(&mut self, frame: &Frame, size: (usize, usize)) {
        let mut buf = String::new();
        if !self.started || size != self.size {
            // First paint, or the window changed: start from a clean screen.
            buf.push_str("\x1b[?25l\x1b[0m\x1b[H\x1b[2J");
            if !self.started {
                buf.push_str("\x1b[?2004h");
            }
            self.last.clear();
            self.started = true;
            self.size = size;
        }
        let mut rows_changed = false;
        for (r, row) in frame.rows.iter().enumerate() {
            if self.last.get(r) != Some(row) {
                if !rows_changed && buf.is_empty() {
                    buf.push_str("\x1b[?25l");
                }
                rows_changed = true;
                // Erase the rest of the row, except after a row that fills it: at the last column the
                // cursor is in its pending-wrap state and an erase there would blank the last cell.
                let erase = if crate::render::visible_len(row) < size.0 { "\x1b[K" } else { "" };
                buf.push_str(&format!("\x1b[{};1H{row}\x1b[0m{erase}", r + 1));
            }
        }
        for r in frame.rows.len()..self.last.len() {
            buf.push_str(&format!("\x1b[{};1H\x1b[K", r + 1));
        }
        self.last = frame.rows.clone();
        if !buf.is_empty() || self.last_cursor != frame.cursor {
            match frame.cursor {
                Some((r, c)) => buf.push_str(&format!("\x1b[{};{}H\x1b[?25h", r + 1, c + 1)),
                None => buf.push_str("\x1b[?25l"),
            }
            self.last_cursor = frame.cursor;
            Self::write(&buf);
        }
    }

    /// Leave the screen as it is and put the cursor under it, where the shell's prompt will go.
    fn finish(&mut self) {
        let used = self.last.iter().rposition(|row| !strip(row).trim().is_empty()).map_or(0, |at| at + 1);
        let mut buf = String::from("\x1b[?2004l\x1b[0m");
        if used >= self.size.1 {
            buf.push_str(&format!("\x1b[{};1H\r\n", self.size.1.max(1)));
        } else {
            buf.push_str(&format!("\x1b[{};1H", used + 1));
        }
        buf.push_str("\x1b[?25h");
        Self::write(&buf);
    }
}

/// Paints like an inline TUI (no alternate screen, no cell addressing from the top of the conversation): the
/// conversation is printed top-down, the rows that overflow the screen scroll into the history, and only the part
/// still on screen is ever redrawn. On a resize, `reprint` agents clear the screen and print the WHOLE
/// conversation again (so the rows that overflowed are in the history twice); the others redraw what is visible.
struct InlinePainter {
    last: Vec<String>,
    last_cursor: Option<(usize, usize)>,
    /// Index of the conversation row on screen row 0; the rows before it are in the history for good.
    win_start: usize,
    size: (usize, usize),
    started: bool,
}

impl InlinePainter {
    fn new() -> InlinePainter {
        InlinePainter { last: Vec::new(), last_cursor: None, win_start: 0, size: (0, 0), started: false }
    }

    fn print(&mut self, rows: &[String], height: usize) -> String {
        let mut buf = String::new();
        let shown = &rows[self.win_start.min(rows.len())..];
        for (i, row) in shown.iter().enumerate() {
            if i > 0 {
                buf.push_str("\r\n");
            }
            buf.push_str(row);
            buf.push_str("\x1b[0m");
        }
        self.win_start += shown.len().saturating_sub(height);
        buf
    }

    fn draw(&mut self, frame: &Frame, size: (usize, usize), reprint: bool) {
        let resized = self.started && size != self.size;
        if self.started && !resized && self.last == frame.rows && self.last_cursor == frame.cursor {
            return;
        }
        let mut buf = String::from("\x1b[?25l\x1b[0m\x1b[H");
        if !self.started {
            buf.push_str("\x1b[2J\x1b[?2004h");
        } else if resized && reprint {
            buf.push_str("\x1b[2J");
            self.win_start = 0;
        } else {
            buf.push_str("\x1b[J");
        }
        self.started = true;
        self.size = size;
        let printed = self.print(&frame.rows, size.1.max(1));
        buf.push_str(&printed);
        self.last = frame.rows.clone();
        self.last_cursor = frame.cursor;
        match frame.cursor.and_then(|(r, c)| r.checked_sub(self.win_start).map(|r| (r, c))) {
            Some((r, c)) => buf.push_str(&format!("\x1b[{};{}H\x1b[?25h", r + 1, c + 1)),
            None => buf.push_str("\x1b[?25l"),
        }
        Painter::write(&buf);
    }

    fn finish(&mut self) {
        let shown = self.last.len().saturating_sub(self.win_start);
        let mut buf = String::from("\x1b[?2004l\x1b[0m");
        if shown >= self.size.1 {
            buf.push_str(&format!("\x1b[{};1H\r\n", self.size.1.max(1)));
        } else {
            buf.push_str(&format!("\x1b[{};1H", shown + 1));
        }
        buf.push_str("\x1b[?25h");
        Painter::write(&buf);
    }
}

/// Plays `script` on the console until it exits; returns the process exit code.
pub fn run(script: Script, base_dir: PathBuf) -> i32 {
    let console = Console::raw();
    let start = Instant::now();
    let now = || start.elapsed().as_millis() as u64;
    let mut size = Console::size();
    let mut engine = Engine::new(script, base_dir, size.0, size.1, 0);
    let input = term::spawn_stdin_reader();
    let mut parser = KeyParser::default();
    let mut painter = Painter::new();
    let mut inline = InlinePainter::new();
    let mut stdin_open = true;
    let mut next_size_check = 0u64;
    // Keys read but not handled yet: with `input_keys_per_turn` a loaded machine takes input in small steps.
    let per_turn = engine.keys_per_turn();
    // Each key with the time it was read: a key handled late still counts as having arrived when it did.
    let mut waiting: std::collections::VecDeque<(crate::keys::Key, u64)> = std::collections::VecDeque::new();
    let code = loop {
        if stdin_open {
            match input.recv_timeout(Duration::from_millis(15)) {
                Ok(bytes) => {
                    let t = now();
                    waiting.extend(parser.feed(&bytes, t).into_iter().map(|key| (key, t)));
                    while let Ok(more) = input.try_recv() {
                        waiting.extend(parser.feed(&more, t).into_iter().map(|key| (key, t)));
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => stdin_open = false,
            }
        } else {
            std::thread::sleep(Duration::from_millis(15));
        }
        let t = now();
        waiting.extend(parser.poll(t).into_iter().map(|key| (key, t)));
        let mut handled = 0;
        while per_turn == 0 || handled < per_turn {
            let Some((key, arrived)) = waiting.pop_front() else { break };
            engine.on_key_arrived(key, t, arrived);
            handled += 1;
        }
        if t >= next_size_check {
            next_size_check = t + 100;
            size = Console::size();
            engine.resize(size.0, size.1, t);
        }
        engine.tick(t);
        if engine.inline() {
            inline.draw(&engine.frame_all(t), size, engine.reprints_on_resize());
        } else {
            painter.draw(&engine.frame(t), size);
        }
        if let Some(code) = engine.exit_code() {
            break code;
        }
    };
    if engine.inline() {
        inline.finish();
    } else {
        painter.finish();
    }
    drop(console);
    code
}
