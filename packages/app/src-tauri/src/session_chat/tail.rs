//! Incremental reader for a file that another process appends to.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::PathBuf;

/// Tracks how far a file has been read. Only bytes after the last complete line are read again.
#[derive(Debug)]
pub struct Tail {
    path: PathBuf,
    /// Byte offset where the next unread line starts.
    offset: u64,
    /// Bytes from `offset` on that do not end in '\n' yet.
    partial: Vec<u8>,
    /// File length consumed by the last poll (`offset + partial.len()`).
    len_seen: u64,
}

#[derive(Debug, PartialEq)]
pub enum Polled {
    Lines(Vec<(u64, String)>),
    Reset(Vec<(u64, String)>),
}

impl Tail {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Tail { path: path.into(), offset: 0, partial: Vec::new(), len_seen: 0 }
    }

    /// Complete lines appended since the last poll, each with the byte offset where it starts.
    /// A last line without '\n' is kept back until it is complete. If the file got shorter
    /// (replaced or truncated), start again from 0 and return Reset with all lines.
    /// A missing file gives Ok(Lines(vec![])).
    pub fn poll(&mut self) -> io::Result<Polled> {
        let mut file = match File::open(&self.path) {
            Ok(file) => file,
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                return Ok(Polled::Lines(Vec::new()));
            }
            Err(err) => return Err(err),
        };
        let len = file.metadata()?.len();
        let reset = len < self.len_seen;
        if reset {
            self.offset = 0;
            self.partial.clear();
        }

        let from = self.offset + self.partial.len() as u64;
        file.seek(SeekFrom::Start(from))?;
        let mut fresh = Vec::new();
        file.read_to_end(&mut fresh)?;

        let lines = self.take_complete_lines(fresh);
        if reset {
            Ok(Polled::Reset(lines))
        } else {
            Ok(Polled::Lines(lines))
        }
    }

    /// Joins the kept-back bytes with `fresh`, returns the complete non-empty lines, and keeps
    /// whatever follows the last '\n' for the next poll.
    fn take_complete_lines(&mut self, fresh: Vec<u8>) -> Vec<(u64, String)> {
        let base = self.offset;
        let mut data = std::mem::take(&mut self.partial);
        data.extend_from_slice(&fresh);

        let mut lines = Vec::new();
        let mut start = 0usize;
        while let Some(rel) = data[start..].iter().position(|&b| b == b'\n') {
            let end = start + rel;
            let mut raw = &data[start..end];
            if raw.last() == Some(&b'\r') {
                raw = &raw[..raw.len() - 1];
            }
            if !raw.is_empty() {
                lines.push((base + start as u64, String::from_utf8_lossy(raw).into_owned()));
            }
            start = end + 1;
        }

        data.drain(..start);
        self.offset = base + start as u64;
        self.len_seen = self.offset + data.len() as u64;
        self.partial = data;
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    /// A file in the system temp dir, removed when the guard drops.
    struct Scratch {
        path: PathBuf,
    }

    impl Scratch {
        fn new(name: &str) -> Self {
            let unique = COUNTER.fetch_add(1, Ordering::SeqCst);
            let path = std::env::temp_dir().join(format!(
                "pm-session-chat-tail-{}-{}-{}",
                std::process::id(),
                unique,
                name
            ));
            let _ = std::fs::remove_file(&path);
            Scratch { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }

        fn write(&self, bytes: &[u8]) {
            std::fs::write(&self.path, bytes).unwrap();
        }

        fn append(&self, bytes: &[u8]) {
            let mut file = OpenOptions::new().append(true).create(true).open(&self.path).unwrap();
            file.write_all(bytes).unwrap();
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    fn lines(polled: Polled) -> Vec<(u64, String)> {
        match polled {
            Polled::Lines(lines) => lines,
            Polled::Reset(_) => panic!("expected Lines, got Reset"),
        }
    }

    fn owned(items: &[(u64, &str)]) -> Vec<(u64, String)> {
        items.iter().map(|(offset, text)| (*offset, text.to_string())).collect()
    }

    #[test]
    fn missing_file_gives_no_lines() {
        let scratch = Scratch::new("missing.jsonl");
        let mut tail = Tail::new(scratch.path());
        assert_eq!(tail.poll().unwrap(), Polled::Lines(vec![]));
        assert_eq!(tail.poll().unwrap(), Polled::Lines(vec![]));
    }

    #[test]
    fn appended_lines_come_once_with_their_offsets() {
        let scratch = Scratch::new("pieces.jsonl");
        let mut tail = Tail::new(scratch.path());

        scratch.write(b"a\n");
        assert_eq!(lines(tail.poll().unwrap()), owned(&[(0, "a")]));
        assert_eq!(lines(tail.poll().unwrap()), owned(&[]));

        scratch.append(b"bc\n");
        assert_eq!(lines(tail.poll().unwrap()), owned(&[(2, "bc")]));
    }

    #[test]
    fn half_written_line_is_kept_back_until_complete() {
        let scratch = Scratch::new("half.jsonl");
        let mut tail = Tail::new(scratch.path());

        scratch.write(b"first\nsec");
        assert_eq!(lines(tail.poll().unwrap()), owned(&[(0, "first")]));

        scratch.append(b"ond");
        assert_eq!(lines(tail.poll().unwrap()), owned(&[]));

        scratch.append(b"\nthird\n");
        assert_eq!(lines(tail.poll().unwrap()), owned(&[(6, "second"), (13, "third")]));
    }

    #[test]
    fn empty_lines_are_skipped_but_still_counted_in_offsets() {
        let scratch = Scratch::new("empty.jsonl");
        let mut tail = Tail::new(scratch.path());

        scratch.write(b"a\n\nb\n");
        assert_eq!(lines(tail.poll().unwrap()), owned(&[(0, "a"), (3, "b")]));

        scratch.append(b"\n\nc\n");
        assert_eq!(lines(tail.poll().unwrap()), owned(&[(7, "c")]));
    }

    #[test]
    fn crlf_line_endings_are_trimmed_and_offsets_count_both_bytes() {
        let scratch = Scratch::new("crlf.jsonl");
        let mut tail = Tail::new(scratch.path());

        scratch.write(b"x\r\ny\r\n");
        assert_eq!(lines(tail.poll().unwrap()), owned(&[(0, "x"), (3, "y")]));
    }

    #[test]
    fn crlf_split_across_polls_keeps_the_line_whole() {
        let scratch = Scratch::new("crlf-split.jsonl");
        let mut tail = Tail::new(scratch.path());

        scratch.write(b"one\r");
        assert_eq!(lines(tail.poll().unwrap()), owned(&[]));

        scratch.append(b"\ntwo\r\n");
        assert_eq!(lines(tail.poll().unwrap()), owned(&[(0, "one"), (5, "two")]));
    }

    #[test]
    fn multibyte_text_advances_offsets_by_bytes() {
        let scratch = Scratch::new("multibyte.jsonl");
        let mut tail = Tail::new(scratch.path());

        // "é" is two bytes, so the second line starts at byte 3.
        scratch.write("é\nab\n".as_bytes());
        assert_eq!(lines(tail.poll().unwrap()), owned(&[(0, "é"), (3, "ab")]));
    }

    #[test]
    fn invalid_utf8_is_replaced_not_rejected() {
        let scratch = Scratch::new("bad-utf8.jsonl");
        let mut tail = Tail::new(scratch.path());

        scratch.write(b"ok\n\xff\xfe\n");
        let got = lines(tail.poll().unwrap());
        assert_eq!(got.len(), 2);
        assert_eq!(got[0], (0, "ok".to_string()));
        assert_eq!(got[1].0, 3);
        assert!(got[1].1.contains('\u{fffd}'));
    }

    #[test]
    fn truncation_gives_reset_with_every_line() {
        let scratch = Scratch::new("truncate.jsonl");
        let mut tail = Tail::new(scratch.path());

        scratch.write(b"one\ntwo\n");
        assert_eq!(lines(tail.poll().unwrap()), owned(&[(0, "one"), (4, "two")]));

        scratch.write(b"new\n");
        assert_eq!(tail.poll().unwrap(), Polled::Reset(owned(&[(0, "new")])));

        scratch.append(b"more\n");
        assert_eq!(lines(tail.poll().unwrap()), owned(&[(4, "more")]));
    }

    #[test]
    fn truncation_to_empty_gives_reset_with_no_lines() {
        let scratch = Scratch::new("empty-reset.jsonl");
        let mut tail = Tail::new(scratch.path());

        scratch.write(b"one\n");
        assert_eq!(lines(tail.poll().unwrap()), owned(&[(0, "one")]));

        scratch.write(b"");
        assert_eq!(tail.poll().unwrap(), Polled::Reset(vec![]));
    }

    #[test]
    fn truncation_drops_a_kept_back_partial_line() {
        let scratch = Scratch::new("reset-partial.jsonl");
        let mut tail = Tail::new(scratch.path());

        scratch.write(b"a\npart");
        assert_eq!(lines(tail.poll().unwrap()), owned(&[(0, "a")]));

        scratch.write(b"b\n");
        assert_eq!(tail.poll().unwrap(), Polled::Reset(owned(&[(0, "b")])));
    }
}
