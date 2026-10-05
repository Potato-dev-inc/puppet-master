//! Hand the latest value to a background writer, dropping the ones it never got to.
//!
//! The session table is persisted whenever its lock guard was mutably dereferenced. That used to
//! serialize and write the whole table (1.2MB, thousands of stale leases) while still holding the
//! process-wide lock: ~0.8s on the follow-up path and a stall for every other connection. Only the
//! newest snapshot ever matters, so the guard now just publishes a clone here.

use parking_lot::{Condvar, Mutex};
use std::sync::Arc;

pub(super) struct Coalescer<T> {
    slot: Mutex<Option<T>>,
    wake: Condvar,
}

impl<T: Send + 'static> Coalescer<T> {
    /// Starts the writer thread; `sink` receives each value it picks up, newest first-come.
    pub(super) fn spawn(name: &str, mut sink: impl FnMut(T) + Send + 'static) -> Arc<Self> {
        let shared = Arc::new(Self {
            slot: Mutex::new(None),
            wake: Condvar::new(),
        });
        let worker = shared.clone();
        let started = std::thread::Builder::new().name(name.to_string()).spawn(move || loop {
            let value = {
                let mut slot = worker.slot.lock();
                while slot.is_none() {
                    worker.wake.wait(&mut slot);
                }
                slot.take()
            };
            if let Some(value) = value {
                sink(value);
            }
        });
        if let Err(error) = started {
            tracing::warn!(%error, "could not start the {name} writer thread");
        }
        shared
    }

    /// Never blocks on the sink: replaces any value the writer has not picked up yet.
    pub(super) fn submit(&self, value: T) {
        *self.slot.lock() = Some(value);
        self.wake.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    #[test]
    fn a_slow_sink_never_blocks_the_submitter_and_ends_on_the_newest_value() {
        let written = Arc::new(Mutex::new(Vec::<u32>::new()));
        let sink_written = written.clone();
        let coalescer = Coalescer::spawn("test-writer", move |value: u32| {
            std::thread::sleep(Duration::from_millis(40));
            sink_written.lock().push(value);
        });
        let started = Instant::now();
        for value in 0..100u32 {
            coalescer.submit(value);
        }
        assert!(
            started.elapsed() < Duration::from_millis(30),
            "100 submits took {:?}",
            started.elapsed()
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while written.lock().last() != Some(&99) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        let written = written.lock();
        assert_eq!(written.last(), Some(&99), "the newest snapshot is the one that lands");
        assert!(written.len() < 20, "intermediate snapshots are dropped: {written:?}");
        assert!(written.windows(2).all(|pair| pair[0] < pair[1]), "never out of order: {written:?}");
    }

    #[test]
    fn every_idle_submit_is_written() {
        let count = Arc::new(AtomicUsize::new(0));
        let sink_count = count.clone();
        let coalescer = Coalescer::spawn("test-writer-idle", move |_: u8| {
            sink_count.fetch_add(1, Ordering::SeqCst);
        });
        for round in 1..=3usize {
            coalescer.submit(0);
            let deadline = Instant::now() + Duration::from_secs(5);
            while count.load(Ordering::SeqCst) < round && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
            assert_eq!(count.load(Ordering::SeqCst), round);
        }
    }
}
