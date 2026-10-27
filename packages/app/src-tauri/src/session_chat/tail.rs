//! Incremental reader for a file that another process appends to, and the reader for a finished
//! `.zst` (compressed) file, which is read whole, once.

use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use ruzstd::decoding::{FrameDecoder, StreamingDecoder};

/// Tracks how far a file has been read. Only bytes after the last complete line are read again.
/// A `.zst` file is finished: it is read once, whole, and not polled again.
#[derive(Debug)]
pub struct Tail {
    path: PathBuf,
    /// Byte offset where the next unread line starts.
    offset: u64,
    /// Bytes from `offset` on that do not end in '\n' yet.
    partial: Vec<u8>,
    /// File length consumed by the last poll (`offset + partial.len()`).
    len_seen: u64,
    /// A `.zst` file has been read whole.
    done: bool,
    /// The (length, modified time) of a `.zst` file that failed to decompress. It is not read again until one changes.
    broken: Option<(u64, Option<SystemTime>)>,
    /// Most bytes one poll reads; the rest of a bigger backlog is read by the next polls.
    cap: Option<u64>,
}

#[derive(Debug, PartialEq)]
pub enum Polled {
    Lines(Vec<(u64, String)>),
    Reset(Vec<(u64, String)>),
}

/// A file a finished session is kept in: `.zst` (zstd-compressed).
fn is_zst(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("zst"))
}

/// The lines of a file as a reader: decompressed as they are read when the file ends in `.zst`.
pub fn open_lines(path: &Path) -> io::Result<Box<dyn BufRead>> {
    let file = File::open(path)?;
    if is_zst(path) {
        Ok(Box::new(BufReader::new(ZstFrames::new(file))))
    } else {
        Ok(Box::new(BufReader::new(file)))
    }
}

/// The bytes of a file, with some of them read ahead and put back (`unread`).
struct Source {
    /// Bytes put back, read again before the file.
    head: Vec<u8>,
    /// Where the next put-back byte is in `head`.
    from: usize,
    file: BufReader<File>,
}

impl Source {
    /// Puts `bytes` back in front of everything not read yet.
    fn unread(&mut self, bytes: &[u8]) {
        let mut head = bytes.to_vec();
        head.extend_from_slice(&self.head[self.from..]);
        self.head = head;
        self.from = 0;
    }

    /// Fills `buf` as far as the file goes: fewer bytes than `buf` only at its end.
    fn read_head(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut got = 0;
        while got < buf.len() {
            let read = self.read(&mut buf[got..])?;
            if read == 0 {
                break;
            }
            got += read;
        }
        Ok(got)
    }
}

impl Read for Source {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.from < self.head.len() {
            let pending = &self.head[self.from..];
            let read = pending.len().min(buf.len());
            buf[..read].copy_from_slice(&pending[..read]);
            self.from += read;
            return Ok(read);
        }
        self.file.read(buf)
    }
}

/// Every zstd frame of a file, one after another. A skippable frame (magic `0x184D2A5?`) is skipped.
/// A finished Codex session is one frame, but a file made by appending compressed parts has several.
struct ZstFrames {
    /// The file between frames; `None` while a frame is being decoded (the decoder owns it).
    source: Option<Source>,
    frame: Option<StreamingDecoder<Source, FrameDecoder>>,
}

impl ZstFrames {
    fn new(file: File) -> Self {
        let file = BufReader::new(file);
        ZstFrames { source: Some(Source { head: Vec::new(), from: 0, file }), frame: None }
    }
}

impl Read for ZstFrames {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            if let Some(frame) = self.frame.as_mut() {
                let read = frame.read(buf)?;
                if read > 0 {
                    return Ok(read);
                }
                // The frame is complete; its reader comes back for the next one.
                self.source = self.frame.take().map(StreamingDecoder::into_inner);
            }
            let Some(source) = self.source.as_mut() else {
                return Ok(0);
            };
            let mut head = [0u8; 8];
            let got = source.read_head(&mut head)?;
            if got == 0 {
                return Ok(0);
            }
            let magic = u32::from_le_bytes([head[0], head[1], head[2], head[3]]);
            if got == 8 && magic & 0xFFFF_FFF0 == 0x184D_2A50 {
                let size = u64::from(u32::from_le_bytes([head[4], head[5], head[6], head[7]]));
                io::copy(&mut source.by_ref().take(size), &mut io::sink())?;
                continue;
            }
            source.unread(&head[..got]);
            let source = self.source.take().expect("the reader is here between frames");
            let frame = StreamingDecoder::new(source)
                .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err.to_string()))?;
            self.frame = Some(frame);
        }
    }
}

/// Every line of a file (decompressed when it ends in `.zst`), each with the byte offset where it starts in
/// the text. Empty lines are skipped (their bytes still count) and a CRLF ending is trimmed, as in `Tail`.
pub fn read_all_lines(path: &Path) -> io::Result<Vec<(u64, String)>> {
    let mut reader = open_lines(path)?;
    let mut lines = Vec::new();
    let mut offset = 0u64;
    let mut raw = Vec::new();
    loop {
        raw.clear();
        let read = reader.read_until(b'\n', &mut raw)?;
        if read == 0 {
            break;
        }
        let text = raw.strip_suffix(b"\n").unwrap_or(&raw);
        let text = text.strip_suffix(b"\r").unwrap_or(text);
        if !text.is_empty() {
            lines.push((offset, String::from_utf8_lossy(text).into_owned()));
        }
        offset += read as u64;
    }
    Ok(lines)
}

impl Tail {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Tail { path: path.into(), offset: 0, partial: Vec::new(), len_seen: 0, done: false, broken: None, cap: None }
    }

    /// Reads at most `cap` bytes per poll.
    pub fn with_cap(mut self, cap: Option<u64>) -> Self {
        self.cap = cap;
        self
    }

    /// Complete lines appended since the last poll, each with the byte offset where it starts.
    /// A last line without '\n' is kept back until it is complete. If the file got shorter
    /// (replaced or truncated), start again from 0 and return Reset with all lines.
    /// A missing file gives Ok(Lines(vec![])). A `.zst` file gives all its lines on the first poll
    /// that reads it, and nothing after that. A damaged `.zst` is an error once, then gives nothing until its
    /// length or modified time changes.
    pub fn poll(&mut self) -> io::Result<Polled> {
        if is_zst(&self.path) {
            return self.poll_finished();
        }
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
        file.take(self.cap.unwrap_or(u64::MAX)).read_to_end(&mut fresh)?;

        let lines = self.take_complete_lines(fresh);
        if reset {
            Ok(Polled::Reset(lines))
        } else {
            Ok(Polled::Lines(lines))
        }
    }

    /// A finished `.zst` session: read whole on the first poll that finds it, never again after that.
    /// A file that failed to decompress is skipped until its length or modified time changes.
    fn poll_finished(&mut self) -> io::Result<Polled> {
        if self.done {
            return Ok(Polled::Lines(Vec::new()));
        }
        let stamp = match std::fs::metadata(&self.path) {
            Ok(meta) => (meta.len(), meta.modified().ok()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Polled::Lines(Vec::new())),
            Err(err) => return Err(err),
        };
        if self.broken == Some(stamp) {
            return Ok(Polled::Lines(Vec::new()));
        }
        match read_all_lines(&self.path) {
            Ok(lines) => {
                self.done = true;
                self.broken = None;
                Ok(Polled::Lines(lines))
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(Polled::Lines(Vec::new())),
            Err(err) => {
                self.broken = Some(stamp);
                Err(err)
            }
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

    #[test]
    fn a_capped_poll_reads_a_big_backlog_over_several_polls_without_losing_a_line() {
        let scratch = Scratch::new("capped.jsonl");
        scratch.write(b"aaaa
bbbb
cccc
dddd
");
        let mut tail = Tail::new(scratch.path()).with_cap(Some(7));
        let mut all = Vec::new();
        for _ in 0..6 {
            all.extend(lines(tail.poll().unwrap()));
        }
        let text: Vec<&str> = all.iter().map(|(_, line)| line.as_str()).collect();
        assert_eq!(text, ["aaaa", "bbbb", "cccc", "dddd"]);
        assert_eq!(all[3].0, 15);
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

    /// A zstd frame with one raw (stored, not compressed) block. Payloads stay under 128 KiB, so one block is
    /// enough. ruzstd has no encoder; this is the smallest frame it reads.
    fn zst_frame(payload: &[u8]) -> Vec<u8> {
        let mut frame = vec![0x28, 0xB5, 0x2F, 0xFD, 0xA0]; // magic, single segment, 4-byte content size
        frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        let block_header = ((payload.len() as u32) << 3) | 1; // size << 3, type 0 (raw), last block
        frame.extend_from_slice(&block_header.to_le_bytes()[..3]);
        frame.extend_from_slice(payload);
        frame
    }

    #[test]
    fn a_zst_file_gives_every_line_once_with_its_byte_offset() {
        let scratch = Scratch::new("lines.jsonl.zst");
        scratch.write(&zst_frame(b"one\r\ntwo\n\nthree"));
        let want = owned(&[(0, "one"), (5, "two"), (10, "three")]);
        assert_eq!(read_all_lines(scratch.path()).unwrap(), want);

        let mut tail = Tail::new(scratch.path());
        assert_eq!(tail.poll().unwrap(), Polled::Lines(want));
        assert_eq!(tail.poll().unwrap(), Polled::Lines(owned(&[])));
    }

    #[test]
    fn a_zst_file_is_read_once_and_not_polled_again() {
        let scratch = Scratch::new("once.jsonl.zst");
        let mut tail = Tail::new(scratch.path());
        assert_eq!(tail.poll().unwrap(), Polled::Lines(vec![]));

        scratch.write(&zst_frame(b"one\n"));
        assert_eq!(lines(tail.poll().unwrap()), owned(&[(0, "one")]));

        // Finished: a change to the file is not read.
        scratch.write(b"not zstd at all");
        assert_eq!(tail.poll().unwrap(), Polled::Lines(vec![]));
    }

    #[test]
    fn a_damaged_zst_file_is_an_error_and_is_read_again_once_it_changes() {
        let scratch = Scratch::new("damaged.jsonl.zst");
        scratch.write(b"not zstd at all");
        let mut tail = Tail::new(scratch.path());
        assert!(tail.poll().is_err());

        scratch.write(&zst_frame(b"two\n"));
        assert_eq!(lines(tail.poll().unwrap()), owned(&[(0, "two")]));
    }

    /// A skippable zstd frame: the magic `0x184D2A5?`, a 4-byte size, then that many bytes of anything.
    fn skippable_frame(payload: &[u8]) -> Vec<u8> {
        let mut frame = vec![0x50, 0x2A, 0x4D, 0x18];
        frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        frame.extend_from_slice(payload);
        frame
    }

    #[test]
    fn every_frame_of_a_zst_file_is_read_and_skippable_frames_give_nothing() {
        let scratch = Scratch::new("frames.jsonl.zst");
        let mut bytes = skippable_frame(b"not text at all");
        bytes.extend(zst_frame(b"one\n"));
        bytes.extend(zst_frame(b"two\n"));
        bytes.extend(skippable_frame(b"x"));
        bytes.extend(zst_frame(b"three"));
        scratch.write(&bytes);
        assert_eq!(read_all_lines(scratch.path()).unwrap(), owned(&[(0, "one"), (4, "two"), (8, "three")]));
    }

    #[test]
    fn a_damaged_zst_file_is_skipped_until_its_length_or_time_changes() {
        use std::time::{Duration, SystemTime};

        let scratch = Scratch::new("skipped.jsonl.zst");
        scratch.write(b"not zstd at all");
        let mut tail = Tail::new(scratch.path());
        assert!(tail.poll().is_err());
        assert_eq!(tail.poll().unwrap(), Polled::Lines(vec![]), "same length and time: not decompressed again");

        // Only the modified time changes: the file is tried again, and it still fails.
        let later = SystemTime::now() + Duration::from_secs(60);
        OpenOptions::new().write(true).open(scratch.path()).unwrap().set_modified(later).unwrap();
        assert!(tail.poll().is_err());
        assert_eq!(tail.poll().unwrap(), Polled::Lines(vec![]));

        scratch.write(&zst_frame(b"two\n"));
        assert_eq!(lines(tail.poll().unwrap()), owned(&[(0, "two")]));
    }
}
