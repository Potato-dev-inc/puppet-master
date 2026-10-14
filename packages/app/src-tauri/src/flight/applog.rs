//! A tail of the app's own log, kept in memory for the diagnostic bundle.
//!
//! The app logs to the console only. `Tee` is the `tracing` writer that keeps doing that and also
//! keeps the last few hundred lines here. Nothing is written to disk until the person saves a
//! diagnostic file (and then it is redacted).

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::io::Write;

const MAX_LINES: usize = 800;
const MAX_LINE_BYTES: usize = 2_000;

static TAIL: Lazy<Mutex<VecDeque<String>>> = Lazy::new(|| Mutex::new(VecDeque::with_capacity(MAX_LINES)));

fn keep(line: &str) {
    let clean = crate::pty::ansi::strip_ansi(line);
    let clean = clean.trim_end();
    if clean.is_empty() {
        return;
    }
    let mut text = clean.to_string();
    if text.len() > MAX_LINE_BYTES {
        let mut cut = MAX_LINE_BYTES;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text.truncate(cut);
        text.push_str(" ...");
    }
    let mut tail = TAIL.lock();
    if tail.len() == MAX_LINES {
        tail.pop_front();
    }
    tail.push_back(text);
}

/// The last log lines, oldest first.
pub fn tail() -> Vec<String> {
    TAIL.lock().iter().cloned().collect()
}

/// Console output plus the in-memory tail.
#[derive(Clone, Copy, Default)]
pub struct Tee;

pub struct TeeWriter;

impl Write for TeeWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        keep(&String::from_utf8_lossy(buf));
        std::io::stdout().write_all(buf)?;
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        std::io::stdout().flush()
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Tee {
    type Writer = TeeWriter;
    fn make_writer(&'a self) -> Self::Writer {
        TeeWriter
    }
}
