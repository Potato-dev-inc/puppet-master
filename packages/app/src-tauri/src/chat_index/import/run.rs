//! The worker thread `chat-import`: lists the chosen agents, then reads their chats newest first, one chat at a time.
//! A chat is written whole in one transaction (`ChatIndex::import_chat`) or not at all, so a Stop, a switched-off
//! index or a killed app leaves only whole chats. It sleeps a moment after every chunk so the screen and the live
//! writer always get their turn. It logs and reports counts and agent ids only: no path and no chat text.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::Ordering;

use super::plan::ChatDraft;
use super::progress::Phase;
use super::{now_ms, Shared, Source, MESSAGE_INDEX_OFF, YIELD_EVERY_CHUNK};
use crate::chat_index::ChatIndex;
use crate::session_chat::history::{valid_session_id, HistoryChat, HistoryEnv, ListLimits, Listing, ReadError, ReadLimits};

/// What the worker was asked to do.
pub(super) struct Job {
    pub(super) sources: Vec<Source>,
    pub(super) since_ms: Option<i64>,
    pub(super) env: HistoryEnv,
}

/// Index write errors in a row after which the run gives up (the index file is probably broken or full).
const MAX_WRITE_FAILURES: usize = 3;

/// How the run ended.
enum End {
    Done,
    Stopped,
    IndexOff,
    Failed(&'static str),
}

/// What to do after one chat.
enum Next {
    Go,
    End(End),
}

/// The thread body: runs the job, publishes how it ended, and frees the "running" flag even after a panic.
pub(super) fn run(index: &ChatIndex, shared: &Shared, job: Job) {
    let end = catch_unwind(AssertUnwindSafe(|| work(index, shared, &job))).unwrap_or(End::Failed("Reading stopped unexpectedly"));
    let (phase, message) = match end {
        End::Done => (Phase::Done, None),
        End::Stopped => (Phase::Cancelled, None),
        End::IndexOff => (Phase::Cancelled, Some(MESSAGE_INDEX_OFF.to_string())),
        End::Failed(message) => (Phase::Failed, Some(message.to_string())),
    };
    let progress = shared.reporter.get();
    tracing::info!(
        ?phase,
        chats = progress.chats_done,
        items = progress.items,
        unchanged = progress.unchanged,
        too_big = progress.too_big,
        unreadable = progress.unreadable,
        "chat import: ended"
    );
    shared.reporter.update(|p| {
        p.phase = phase;
        p.agent = None;
        p.message = message;
    });
    shared.running.store(false, Ordering::SeqCst);
}

/// Stop asked for, or the index was switched off: the reason to stop now.
fn stop_reason(index: &ChatIndex, shared: &Shared) -> Option<End> {
    if shared.cancel.load(Ordering::SeqCst) {
        Some(End::Stopped)
    } else if !index.enabled() {
        Some(End::IndexOff)
    } else {
        None
    }
}

fn work(index: &ChatIndex, shared: &Shared, job: &Job) -> End {
    // Looking: list the chosen agents only, so an agent that is switched off is never opened.
    let mut listed: Vec<(&Source, Listing)> = Vec::new();
    for source in &job.sources {
        if let Some(end) = stop_reason(index, shared) {
            return end;
        }
        shared.reporter.update(|p| p.agent = Some(source.id.clone()));
        let limits = ListLimits { since_ms: job.since_ms, ..ListLimits::default() };
        let listing = source.history.list(&job.env, &limits);
        shared.reporter.update(|p| {
            p.chats_total += listing.chats.len();
            p.too_big += listing.too_big;
            p.unreadable += listing.unreadable;
        });
        listed.push((source, listing));
    }
    shared.reporter.update(|p| p.phase = Phase::Reading);

    let mut write_failures = 0;
    for (source, listing) in &listed {
        shared.reporter.update(|p| p.agent = Some(source.id.clone()));
        for chat in &listing.chats {
            if let Some(end) = stop_reason(index, shared) {
                return end;
            }
            let next = one_chat(index, shared, job, source, chat, &mut write_failures);
            shared.reporter.update(|p| p.chats_done += 1);
            if let Next::End(end) = next {
                return end;
            }
        }
    }
    End::Done
}

/// Looks at one chat: skips it when it is unchanged or live, else reads it whole and writes it.
fn one_chat(index: &ChatIndex, shared: &Shared, job: &Job, source: &Source, chat: &HistoryChat, write_failures: &mut usize) -> Next {
    let bump = |change: fn(&mut super::Progress)| shared.reporter.update(change);
    // An id from a file name or a database is checked before it is used for anything.
    if !valid_session_id(&chat.session) {
        bump(|p| p.unreadable += 1);
        return Next::Go;
    }
    match index.imported_stamp(&source.id, &chat.session) {
        Ok(Some(stamp)) if stamp == chat.stamp => {
            bump(|p| p.unchanged += 1);
            return Next::Go;
        }
        Ok(_) => {}
        Err(_) => return store_failed(shared, write_failures),
    }
    match index.has_live(&source.id, &chat.session) {
        Ok(true) => {
            bump(|p| p.unchanged += 1);
            return Next::Go;
        }
        Ok(false) => {}
        Err(_) => return store_failed(shared, write_failures),
    }

    let mut draft = ChatDraft::new(&source.id, chat);
    let mut stop: Option<End> = None;
    let read = source.history.read(&job.env, &chat.session, &ReadLimits::default(), &mut |batch| {
        draft.push(&batch);
        // Between chunks: Stop, the switch, and a moment for everyone else.
        stop = stop_reason(index, shared);
        if stop.is_some() {
            return false;
        }
        std::thread::sleep(YIELD_EVERY_CHUNK);
        true
    });
    if let Some(end) = stop {
        // The chat in hand is dropped: only whole chats are written.
        return Next::End(end);
    }
    match read {
        Ok(outcome) => {
            let (row, entries) = draft.finish(outcome.cut_short, now_ms());
            match index.import_chat(&row, &entries) {
                Ok(()) => {
                    *write_failures = 0;
                    let kept = entries.len();
                    shared.reporter.update(|p| p.items += kept);
                    Next::Go
                }
                Err(error) if error == super::INDEX_OFF => Next::End(End::IndexOff),
                Err(_) => store_failed(shared, write_failures),
            }
        }
        Err(ReadError::TooBig) => {
            bump(|p| p.too_big += 1);
            Next::Go
        }
        Err(ReadError::Missing | ReadError::Unreadable | ReadError::Format) => {
            bump(|p| p.unreadable += 1);
            Next::Go
        }
    }
}

/// The index could not be read or written for this chat: counted, and the run gives up after a few in a row.
fn store_failed(shared: &Shared, write_failures: &mut usize) -> Next {
    shared.reporter.update(|p| p.unreadable += 1);
    *write_failures += 1;
    if *write_failures >= MAX_WRITE_FAILURES {
        Next::End(End::Failed("Could not write to the search index"))
    } else {
        Next::Go
    }
}
