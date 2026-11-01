// Behaviour from desktoplab (github.com/v7ren/desktoplab), commit 6e5360e, MIT.
// (hop-engine `clipboard.rs` policy; hop-net chunking of PLAN.md N2.3.)
//! Copy here, paste there. Text and one image travel when the pointer crosses.
//!
//! What DevHop did wrong, and what is done here (tests in `tests.rs`):
//! - It wrote the whole payload (up to 32 MiB) as ONE frame on the control stream, so one slow write stopped every
//!   heartbeat and the other side dropped the session. Here the clipboard has its own capability and its own link
//!   (`pm/desk-clipboard/1`); a payload is cut into 64 KiB chunks, and the control link never waits for it.
//! - The size cap was checked by the sender only. Here the receiver checks the announced size, counts what actually
//!   arrives and refuses a payload that grows past what it announced or past 32 MiB. Nothing partial is ever applied.
//! - A copy that came from the other computer was sent straight back on the next crossing. Here the last thing
//!   written (or sent) is remembered by a hash and is not sent again.
//! - Contents never reach a log, an error message or a Debug print.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use desk_proto::{fits_inline, ClipAnnounce, ClipKind, StreamMsg, MAX_INLINE_BYTES};

use super::remote::ClipItem;

/// Bytes per chunk frame.
pub const CHUNK: usize = 64 * 1024;
/// Nothing larger than this is sent or accepted (per copy, all items together).
pub const CAP: usize = MAX_INLINE_BYTES;
/// The most items one copy may hold (text + image today).
pub const MAX_ITEMS: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipError {
    TooBig,
    Malformed(&'static str),
}

impl std::fmt::Display for ClipError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClipError::TooBig => f.write_str("the copy is too big to send"),
            ClipError::Malformed(why) => write!(f, "bad clipboard data: {why}"),
        }
    }
}

fn kind_tag(item: &ClipItem) -> u8 {
    match item {
        ClipItem::Text(_) => 1,
        ClipItem::Png(_) => 2,
    }
}

fn kind_of(item: &ClipItem) -> ClipKind {
    match item {
        ClipItem::Text(_) => ClipKind::Text,
        ClipItem::Png(_) => ClipKind::Png,
    }
}

fn bytes_of(item: &ClipItem) -> &[u8] {
    match item {
        ClipItem::Text(t) => t.as_bytes(),
        ClipItem::Png(b) => b,
    }
}

/// A fingerprint of a copy, to tell "I wrote this myself a moment ago" from "the person copied something new".
pub fn fingerprint(items: &[ClipItem]) -> u64 {
    let mut h = DefaultHasher::new();
    for item in items {
        kind_tag(item).hash(&mut h);
        bytes_of(item).hash(&mut h);
    }
    h.finish()
}

/// Drops what cannot travel (an item over the cap; more items than allowed) and says whether the whole copy fits.
/// Returns the items to send. An empty result means "nothing to send".
pub fn prepare(items: Vec<ClipItem>) -> Result<Vec<ClipItem>, ClipError> {
    let mut out = Vec::new();
    let mut total = 0usize;
    for item in items.into_iter().take(MAX_ITEMS) {
        if item.len() == 0 {
            continue;
        }
        if !fits_inline(item.len()) || total + item.len() > CAP {
            return Err(ClipError::TooBig);
        }
        total += item.len();
        out.push(item);
    }
    Ok(out)
}

/// The frames for one copy: an announce, then 64 KiB chunks of each item in order.
pub fn frames(token: u64, items: &[ClipItem]) -> Vec<StreamMsg> {
    let mut out = vec![StreamMsg::ClipboardAnnounce {
        token,
        items: items.iter().map(|i| ClipAnnounce { kind: kind_of(i), bytes: i.len() as u64 }).collect(),
    }];
    for item in items {
        let data = bytes_of(item);
        let n = data.len().div_ceil(CHUNK);
        for (seq, chunk) in data.chunks(CHUNK).enumerate() {
            out.push(StreamMsg::ClipboardChunk { token, kind: kind_of(item), seq: seq as u32, last: seq + 1 == n, bytes: chunk.to_vec() });
        }
    }
    out
}

/// Puts one copy back together from frames, refusing anything that does not match its announce.
#[derive(Default)]
pub struct Assembler {
    token: Option<u64>,
    plan: Vec<ClipAnnounce>,
    /// Index into `plan` of the item being received.
    at: usize,
    next_seq: u32,
    buf: Vec<u8>,
    done: Vec<(ClipKind, Vec<u8>)>,
}

/// Sizes only: a copy never prints.
impl std::fmt::Debug for Assembler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Assembler({} items planned, {} bytes held)", self.plan.len(), self.held())
    }
}

impl Assembler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one frame. `Ok(Some(items))` when the copy is complete (the assembler is then empty again).
    pub fn push(&mut self, msg: StreamMsg) -> Result<Option<Vec<ClipItem>>, ClipError> {
        match msg {
            StreamMsg::ClipboardAnnounce { token, items } => {
                *self = Self::default();
                if items.is_empty() || items.len() > MAX_ITEMS {
                    return Err(ClipError::Malformed("item count"));
                }
                let mut total: u64 = 0;
                for i in &items {
                    total = total.saturating_add(i.bytes);
                    if i.bytes == 0 || i.bytes > CAP as u64 || total > CAP as u64 {
                        return Err(ClipError::TooBig);
                    }
                }
                self.token = Some(token);
                self.plan = items;
                Ok(None)
            }
            StreamMsg::ClipboardChunk { token, kind, seq, last, bytes } => {
                if self.token != Some(token) {
                    return Err(ClipError::Malformed("chunk without announce"));
                }
                let Some(want) = self.plan.get(self.at) else {
                    return Err(ClipError::Malformed("more data than announced"));
                };
                if want.kind != kind || seq != self.next_seq || bytes.len() > CHUNK {
                    return Err(ClipError::Malformed("chunk out of order"));
                }
                // What really arrived, never the number the sender claimed.
                if (self.buf.len() + bytes.len()) as u64 > want.bytes {
                    return Err(ClipError::TooBig);
                }
                self.buf.extend_from_slice(&bytes);
                self.next_seq += 1;
                if last {
                    if self.buf.len() as u64 != want.bytes {
                        return Err(ClipError::Malformed("short item"));
                    }
                    let item = std::mem::take(&mut self.buf);
                    self.done.push((kind, item));
                    self.at += 1;
                    self.next_seq = 0;
                    if self.at == self.plan.len() {
                        let done = std::mem::take(&mut self.done);
                        *self = Self::default();
                        return Ok(Some(finish(done)?));
                    }
                }
                Ok(None)
            }
            _ => Err(ClipError::Malformed("not a clipboard frame")),
        }
    }

    /// Bytes held right now (bounded by `CAP`).
    pub fn held(&self) -> usize {
        self.buf.len() + self.done.iter().map(|(_, b)| b.len()).sum::<usize>()
    }
}

fn finish(done: Vec<(ClipKind, Vec<u8>)>) -> Result<Vec<ClipItem>, ClipError> {
    let mut out = Vec::new();
    for (kind, bytes) in done {
        match kind {
            ClipKind::Text => out.push(ClipItem::Text(String::from_utf8(bytes).map_err(|_| ClipError::Malformed("text is not utf-8"))?)),
            ClipKind::Png => {
                if !bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
                    return Err(ClipError::Malformed("not a png"));
                }
                out.push(ClipItem::Png(bytes));
            }
            // Rich text, web pages and file lists are not carried yet; they are skipped, not applied half-way.
            ClipKind::Html | ClipKind::Rtf | ClipKind::Files => {}
        }
    }
    Ok(out)
}
