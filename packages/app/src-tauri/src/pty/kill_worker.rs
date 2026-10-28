//! A small background worker for slow clean-ups (stopping a pane's process tree).
//!
//! On Windows stopping a tree is a `taskkill` subprocess: 0.5 s normally, up to ~8 s under load.
//! Callers that do not need the tree gone before they continue (closing a pane from the UI,
//! disposing a pane after a run) hand the item here and return at once. The queue is bounded and
//! served by a fixed number of threads, never one thread per item. When the queue is full the
//! caller runs the item itself: that slows it down but never drops a kill and never grows memory.
//! Callers that must know the work is done keep a synchronous path or wait on the [`Ticket`].

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

type Handler<T> = Arc<dyn Fn(T) + Send + Sync>;

struct Job<T> {
    item: T,
    done: mpsc::Sender<()>,
}

/// Counts queued plus running items so [`KillWorker::drain`] can wait for all of them.
#[derive(Default)]
struct Pending {
    count: Mutex<usize>,
    idle: Condvar,
}

impl Pending {
    fn add(&self) {
        *self.count.lock().unwrap_or_else(|e| e.into_inner()) += 1;
    }

    fn finish(&self) {
        let mut count = self.count.lock().unwrap_or_else(|e| e.into_inner());
        *count = count.saturating_sub(1);
        if *count == 0 {
            self.idle.notify_all();
        }
    }
}

/// Resolves when the item it was made for has been handled.
pub struct Ticket(Option<Receiver<()>>);

impl Ticket {
    /// A ticket for work that already ran on the caller's thread.
    fn finished() -> Self {
        Ticket(None)
    }

    /// True once the item was handled, false if `timeout` passed first.
    pub fn wait(&self, timeout: Duration) -> bool {
        match &self.0 {
            None => true,
            // The sender is dropped after the handler returns (or panics), so a closed channel
            // means "done" as well.
            Some(rx) => !matches!(rx.recv_timeout(timeout), Err(mpsc::RecvTimeoutError::Timeout)),
        }
    }
}

pub struct KillWorker<T: Send + 'static> {
    tx: Option<SyncSender<Job<T>>>,
    handler: Handler<T>,
    pending: Arc<Pending>,
}

impl<T: Send + 'static> KillWorker<T> {
    /// `threads` workers share one queue of `capacity` items; each item goes through `handler`.
    pub fn new(
        name: &str,
        threads: usize,
        capacity: usize,
        handler: impl Fn(T) + Send + Sync + 'static,
    ) -> Self {
        let handler: Handler<T> = Arc::new(handler);
        let pending = Arc::new(Pending::default());
        let (tx, rx) = mpsc::sync_channel::<Job<T>>(capacity.max(1));
        let rx = Arc::new(Mutex::new(rx));
        let mut started = 0;
        for index in 0..threads.max(1) {
            let (rx, handler, pending) = (rx.clone(), handler.clone(), pending.clone());
            let spawned = std::thread::Builder::new()
                .name(format!("{name}-{index}"))
                .spawn(move || loop {
                    // Hold the lock only while waiting for the next item, not while handling it.
                    let job = match rx.lock().unwrap_or_else(|e| e.into_inner()).recv() {
                        Ok(job) => job,
                        Err(_) => return,
                    };
                    let _ = catch_unwind(AssertUnwindSafe(|| handler(job.item)));
                    pending.finish();
                    let _ = job.done.send(());
                });
            if spawned.is_ok() {
                started += 1;
            }
        }
        // No thread could start: every item is then handled by its caller.
        Self { tx: (started > 0).then_some(tx), handler, pending }
    }

    /// Queue `item` and return at once. A full queue (or no worker thread) makes this call handle
    /// the item before it returns.
    pub fn submit(&self, item: T) -> Ticket {
        let Some(tx) = &self.tx else {
            (self.handler)(item);
            return Ticket::finished();
        };
        let (done, wait) = mpsc::channel();
        self.pending.add();
        match tx.try_send(Job { item, done }) {
            Ok(()) => Ticket(Some(wait)),
            Err(TrySendError::Full(job)) | Err(TrySendError::Disconnected(job)) => {
                self.pending.finish();
                let _ = catch_unwind(AssertUnwindSafe(|| (self.handler)(job.item)));
                Ticket::finished()
            }
        }
    }

    /// Wait until everything queued so far has been handled. False if `timeout` passed first.
    pub fn drain(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut count = self.pending.count.lock().unwrap_or_else(|e| e.into_inner());
        while *count > 0 {
            let Some(left) = deadline.checked_duration_since(Instant::now()) else { return false };
            let (guard, result) = self.pending.idle.wait_timeout(count, left).unwrap_or_else(|e| e.into_inner());
            count = guard;
            if result.timed_out() && *count > 0 {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    type Log = Arc<Mutex<Vec<u32>>>;

    /// A fake killer: records the pid, optionally after a pause or a gate the test opens.
    fn recording(log: Log, pause: Duration) -> impl Fn(u32) + Send + Sync + 'static {
        move |pid| {
            std::thread::sleep(pause);
            log.lock().unwrap().push(pid);
        }
    }

    #[test]
    fn submit_returns_before_the_slow_kill_finishes() {
        let log: Log = Arc::default();
        let worker = KillWorker::new("t-slow", 1, 8, recording(log.clone(), Duration::from_millis(400)));
        let started = Instant::now();
        let ticket = worker.submit(7);
        assert!(started.elapsed() < Duration::from_millis(200), "the caller waited for the kill");
        assert!(log.lock().unwrap().is_empty(), "the kill had not run yet");
        assert!(ticket.wait(Duration::from_secs(5)));
        assert_eq!(*log.lock().unwrap(), vec![7]);
    }

    #[test]
    fn one_thread_handles_items_in_submit_order() {
        let log: Log = Arc::default();
        let worker = KillWorker::new("t-order", 1, 16, recording(log.clone(), Duration::from_millis(5)));
        for pid in 1..=10 {
            worker.submit(pid);
        }
        assert!(worker.drain(Duration::from_secs(5)));
        assert_eq!(*log.lock().unwrap(), (1..=10).collect::<Vec<_>>());
    }

    #[test]
    fn a_full_queue_makes_the_caller_handle_the_item_and_loses_nothing() {
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let log: Log = Arc::default();
        let threads = Arc::new(Mutex::new(std::collections::HashSet::new()));
        let worker = {
            let (gate, log, threads) = (gate.clone(), log.clone(), threads.clone());
            KillWorker::new("t-full", 1, 2, move |pid: u32| {
                threads.lock().unwrap().insert(std::thread::current().id());
                // Item 1 holds the single worker until the gate opens.
                if pid == 1 {
                    let (open, cv) = &*gate;
                    let mut open = open.lock().unwrap();
                    while !*open {
                        open = cv.wait(open).unwrap();
                    }
                }
                log.lock().unwrap().push(pid);
            })
        };
        worker.submit(1);
        // Let the worker take item 1 off the queue (its handler is running when its thread id is recorded), then
        // fill the 2 free slots. Waiting for that, not for a fixed time: a starved worker thread may take
        // seconds to start, and until it does item 1 still takes one of the 2 slots.
        let began = Instant::now();
        while threads.lock().unwrap().is_empty() {
            assert!(began.elapsed() < Duration::from_secs(60), "the worker never took item 1");
            std::thread::sleep(Duration::from_millis(5));
        }
        worker.submit(2);
        worker.submit(3);
        // Slot count exhausted: this one runs here, so it also finishes before submit returns.
        // (It cannot be item 1's thread, which is parked on the gate.)
        let ticket = worker.submit(4);
        assert!(ticket.wait(Duration::ZERO), "an inline item is already handled");
        assert_eq!(*log.lock().unwrap(), vec![4]);
        assert_eq!(threads.lock().unwrap().len(), 2, "item 4 ran on the caller, not on the pool");
        {
            let (open, cv) = &*gate;
            *open.lock().unwrap() = true;
            cv.notify_all();
        }
        assert!(worker.drain(Duration::from_secs(60)));
        let mut seen = log.lock().unwrap().clone();
        seen.sort();
        assert_eq!(seen, vec![1, 2, 3, 4], "no kill was dropped");
    }

    #[test]
    fn the_pool_never_runs_more_threads_than_asked_for() {
        let live = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let worker = {
            let (live, peak) = (live.clone(), peak.clone());
            KillWorker::new("t-bound", 2, 64, move |_: u32| {
                let now = live.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(now, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(10));
                live.fetch_sub(1, Ordering::SeqCst);
            })
        };
        for pid in 0..40 {
            worker.submit(pid);
        }
        assert!(worker.drain(Duration::from_secs(10)));
        assert!(peak.load(Ordering::SeqCst) <= 2, "more handlers ran at once than threads");
    }

    #[test]
    fn drain_waits_for_a_running_item_and_times_out_while_one_is_stuck() {
        let log: Log = Arc::default();
        let worker = KillWorker::new("t-drain", 1, 4, recording(log.clone(), Duration::from_millis(300)));
        worker.submit(9);
        assert!(!worker.drain(Duration::from_millis(30)), "drain returned while the kill was running");
        assert!(worker.drain(Duration::from_secs(5)));
        assert_eq!(*log.lock().unwrap(), vec![9]);
        assert!(worker.drain(Duration::ZERO), "nothing pending: drain is immediate");
    }

    #[test]
    fn a_panicking_kill_does_not_stop_the_worker() {
        let log: Log = Arc::default();
        let worker = {
            let log = log.clone();
            KillWorker::new("t-panic", 1, 4, move |pid: u32| {
                if pid == 0 {
                    panic!("fake killer failed");
                }
                log.lock().unwrap().push(pid);
            })
        };
        worker.submit(0);
        worker.submit(5);
        assert!(worker.drain(Duration::from_secs(5)));
        assert_eq!(*log.lock().unwrap(), vec![5]);
    }
}
