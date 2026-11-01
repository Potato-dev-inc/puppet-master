// Behaviour from desktoplab (github.com/v7ren/desktoplab), commit 6e5360e, MIT.
// (hop-engine clipboard sync on crossing; PLAN.md N2.2 and N2.3: its own stream, chunked.)
//! The clipboard capability on a link of its own (`pm/desk-clipboard/1`). The control link never waits for it.
//!
//! - **Sending**: when the pointer leaves for another computer (and when it comes back), the local copy is read and,
//!   if it is new, streamed to that computer in 64 KiB chunks. Needs the person's "Clipboard" switch for that computer.
//! - **Receiving**: one copy at a time per computer, checked against its announce, and written to the clipboard only
//!   when complete and only while the switch is still on.
//! - Contents are never logged; nothing here formats a copy into text.

use std::sync::Arc;
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::time::timeout;

use crate::p2p::capability;
use crate::p2p::endpoint::{Link, NodeId};

use super::clipboard::{fingerprint, frames, prepare, Assembler, ClipError};
use super::remote::ClipItem;
use super::wire::{read_frame, read_kind, write_frame, write_kind, CLIP_FRAME_MAX, KIND_CLIPBOARD};
use super::{Inner, CLOSE_PROTOCOL};

/// A stalled sender or receiver is given up on after this long without a byte.
const STEP_TIMEOUT: Duration = Duration::from_secs(30);

/// Runs a host call that may block (the clipboard can be locked by another program for a moment) off the async
/// threads. In tests the call is inline: the mock host never blocks, and it keeps simulated time deterministic.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    #[cfg(test)]
    {
        Some(f())
    }
    #[cfg(not(test))]
    {
        tokio::task::spawn_blocking(f).await.ok()
    }
}

/// The only log line the clipboard writes: what happened, how many items, how many bytes. Never a copy.
fn log_sizes(what: &'static str, items: usize, bytes: usize) {
    tracing::debug!(items, bytes, what, "desk: clipboard");
    #[cfg(test)]
    TEST_LOG.with(|l| l.borrow_mut().push(format!("desk: clipboard {what} items={items} bytes={bytes}")));
}

#[cfg(test)]
thread_local! {
    static TEST_LOG: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// What `log_sizes` wrote on this thread (the tests run on one thread each).
#[cfg(test)]
pub(super) fn take_test_log() -> Vec<String> {
    TEST_LOG.with(|l| std::mem::take(&mut *l.borrow_mut()))
}

pub(super) fn spawn_send(inner: &Arc<Inner>, peer: NodeId) {
    if !inner.clipboard_allowed(&peer) {
        return;
    }
    let inner = inner.clone();
    tokio::spawn(async move { send_current(inner, peer).await });
}

/// What the copy looks like to the person, if it could not be sent.
fn note(inner: &Inner, text: &str) {
    inner.set_clip_note(Some(text.to_string()));
}

pub(super) async fn send_current(inner: Arc<Inner>, peer: NodeId) {
    if !inner.clipboard_allowed(&peer) {
        return;
    }
    let host = inner.host.clone();
    let Some(Ok(items)) = blocking(move || host.clipboard_read()).await else { return };
    let items = match prepare(items) {
        Ok(items) if !items.is_empty() => items,
        Ok(_) => return,
        Err(ClipError::TooBig) => return note(&inner, "That copy is too big to share between computers (32 MB limit)."),
        Err(_) => return,
    };
    let fp = fingerprint(&items);
    if !inner.clip_is_new(&peer, fp) {
        return;
    }
    log_sizes("going out", items.len(), items.iter().map(ClipItem::len).sum::<usize>());
    match timeout(STEP_TIMEOUT, push(&inner, peer, &items)).await {
        Ok(true) => {
            inner.clip_sent(&peer, fp);
            inner.set_clip_note(None);
        }
        _ => note(&inner, "Could not copy to the other computer."),
    }
}

async fn push(inner: &Arc<Inner>, peer: NodeId, items: &[ClipItem]) -> bool {
    let Some(link) = clip_link(inner, peer).await else { return false };
    let Ok(mut stream) = link.open_bi().await else {
        inner.forget_clip_link(&peer);
        return false;
    };
    if write_kind(&mut stream.send, KIND_CLIPBOARD).await.is_err() {
        return false;
    }
    for msg in frames(inner.next_token(), items) {
        if write_frame(&mut stream.send, &msg).await.is_err() {
            return false;
        }
    }
    stream.send.flush().await.is_ok() && stream.send.shutdown().await.is_ok()
}

/// The clipboard link to a computer: the one already open, or a new one.
async fn clip_link(inner: &Arc<Inner>, peer: NodeId) -> Option<Arc<dyn Link>> {
    if let Some(l) = inner.cached_clip_link(&peer) {
        return Some(l);
    }
    let addr = inner.p2p.addrs().peer(peer);
    let link = inner.p2p.dial(&addr, capability::DESK_CLIPBOARD).await.ok()?;
    inner.cache_clip_link(peer, link.clone());
    Some(link)
}

/// An incoming clipboard link, until it ends.
pub(super) async fn serve_incoming(inner: Arc<Inner>, peer: NodeId, link: Arc<dyn Link>) {
    while let Ok(mut stream) = link.accept_bi().await {
        if !inner.clipboard_allowed(&peer) {
            link.close(super::CLOSE_STOPPED, "bye");
            return;
        }
        let Ok(Ok(KIND_CLIPBOARD)) = timeout(STEP_TIMEOUT, read_kind(&mut stream.recv)).await else {
            // Not a clipboard stream: dropped.
            continue;
        };
        let mut asm = Assembler::new();
        loop {
            let frame = match timeout(STEP_TIMEOUT, read_frame(&mut stream.recv, CLIP_FRAME_MAX)).await {
                Ok(Ok(Some(f))) => f,
                Ok(Ok(None)) => break,
                _ => {
                    link.close(CLOSE_PROTOCOL, "bye");
                    return;
                }
            };
            match asm.push(frame) {
                Ok(None) => {}
                Ok(Some(items)) => apply(&inner, &peer, items).await,
                Err(_) => {
                    // A copy that does not match its announce is thrown away whole, and the link with it.
                    link.close(CLOSE_PROTOCOL, "bye");
                    return;
                }
            }
        }
    }
}

async fn apply(inner: &Arc<Inner>, peer: &NodeId, items: Vec<ClipItem>) {
    // The switch is looked at again after the last byte: turning it off mid-copy means nothing is written.
    if items.is_empty() || !inner.clipboard_allowed(peer) {
        return;
    }
    let fp = fingerprint(&items);
    log_sizes("arrived", items.len(), items.iter().map(ClipItem::len).sum::<usize>());
    let host = inner.host.clone();
    let wrote = blocking(move || host.clipboard_write(&items)).await;
    if matches!(wrote, Some(Ok(()))) {
        inner.clip_applied(fp);
    }
}
