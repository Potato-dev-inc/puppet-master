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
    let mut stdin_open = true;
    let mut next_size_check = 0u64;
    let code = loop {
        if stdin_open {
            match input.recv_timeout(Duration::from_millis(15)) {
                Ok(bytes) => {
                    let t = now();
                    for key in parser.feed(&bytes, t) {
                        engine.on_key(key, t);
                    }
                    while let Ok(more) = input.try_recv() {
                        for key in parser.feed(&more, t) {
                            engine.on_key(key, t);
                        }
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => stdin_open = false,
            }
        } else {
            std::thread::sleep(Duration::from_millis(15));
        }
        let t = now();
        for key in parser.poll(t) {
            engine.on_key(key, t);
        }
        if t >= next_size_check {
            next_size_check = t + 100;
            size = Console::size();
            engine.resize(size.0, size.1, t);
        }
        engine.tick(t);
        painter.draw(&engine.frame(t), size);
        if let Some(code) = engine.exit_code() {
            break code;
        }
    };
    painter.finish();
    drop(console);
    code
}
