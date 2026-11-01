//! The queue in front of the adapter's stdin.
//!
//! One writer thread owns the pipe and writes whole lines, so two messages can never interleave. The
//! queue has two lanes:
//!
//! * **normal**: prompts, config changes. Bounded (count and bytes); a full queue is an error to the
//!   caller, never unbounded memory.
//! * **priority**: `session/cancel` and answers to the agent's requests (permission replies). Small, and
//!   drained first, so a cancel never waits behind queued prompts and never fails because the normal
//!   lane is full.
//!
//! While the pipe itself is blocked (the agent is not reading) nothing can be written, which is what the
//! stdin write timeout in `conn.rs` is for.

use std::collections::VecDeque;

use parking_lot::{Condvar, Mutex};

use super::error::AcpError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lane {
    Normal,
    Priority,
}

#[derive(Debug)]
pub struct WriteQueue {
    max_normal: usize,
    max_normal_bytes: usize,
    max_priority: usize,
    state: Mutex<State>,
    wake: Condvar,
}

#[derive(Debug, Default)]
struct State {
    normal: VecDeque<String>,
    normal_bytes: usize,
    priority: VecDeque<String>,
    closed: bool,
    max_normal_seen: usize,
}

impl WriteQueue {
    pub fn new(max_normal: usize, max_normal_bytes: usize, max_priority: usize) -> Self {
        Self {
            max_normal: max_normal.max(1),
            max_normal_bytes: max_normal_bytes.max(1024),
            max_priority: max_priority.max(1),
            state: Mutex::new(State::default()),
            wake: Condvar::new(),
        }
    }

    pub fn push(&self, lane: Lane, line: String) -> Result<(), AcpError> {
        let mut state = self.state.lock();
        if state.closed {
            return Err(AcpError::Closed);
        }
        match lane {
            Lane::Priority => {
                if state.priority.len() >= self.max_priority {
                    return Err(AcpError::QueueFull);
                }
                state.priority.push_back(line);
            }
            Lane::Normal => {
                if state.normal.len() >= self.max_normal || state.normal_bytes + line.len() > self.max_normal_bytes {
                    return Err(AcpError::QueueFull);
                }
                state.normal_bytes += line.len();
                state.normal.push_back(line);
                state.max_normal_seen = state.max_normal_seen.max(state.normal.len());
            }
        }
        drop(state);
        self.wake.notify_one();
        Ok(())
    }

    /// Blocks for the next line to write (priority first). `None` once the queue is closed: lines still
    /// queued are dropped, the connection is going away.
    pub fn pop_blocking(&self) -> Option<String> {
        let mut state = self.state.lock();
        loop {
            if state.closed {
                return None;
            }
            if let Some(line) = state.priority.pop_front() {
                return Some(line);
            }
            if let Some(line) = state.normal.pop_front() {
                state.normal_bytes = state.normal_bytes.saturating_sub(line.len());
                return Some(line);
            }
            self.wake.wait(&mut state);
        }
    }

    pub fn close(&self) {
        let mut state = self.state.lock();
        state.closed = true;
        state.normal.clear();
        state.priority.clear();
        state.normal_bytes = 0;
        drop(state);
        self.wake.notify_all();
    }

    /// `(normal, priority)` lines waiting.
    pub fn depth(&self) -> (usize, usize) {
        let state = self.state.lock();
        (state.normal.len(), state.priority.len())
    }

    /// The most normal-lane lines ever waiting at once.
    pub fn max_normal_depth(&self) -> usize {
        self.state.lock().max_normal_seen
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn priority_lines_jump_the_normal_lane_but_keep_their_own_order() {
        let queue = WriteQueue::new(10, 10_000, 10);
        queue.push(Lane::Normal, "n1".into()).unwrap();
        queue.push(Lane::Normal, "n2".into()).unwrap();
        queue.push(Lane::Priority, "p1".into()).unwrap();
        queue.push(Lane::Priority, "p2".into()).unwrap();
        let order: Vec<String> = (0..4).filter_map(|_| queue.pop_blocking()).collect();
        assert_eq!(order, vec!["p1", "p2", "n1", "n2"]);
    }

    #[test]
    fn a_full_normal_lane_refuses_but_never_blocks_the_priority_lane() {
        let queue = WriteQueue::new(2, 10_000, 4);
        queue.push(Lane::Normal, "a".into()).unwrap();
        queue.push(Lane::Normal, "b".into()).unwrap();
        assert!(matches!(queue.push(Lane::Normal, "c".into()), Err(AcpError::QueueFull)));
        queue.push(Lane::Priority, "cancel".into()).unwrap();
        assert_eq!(queue.pop_blocking().as_deref(), Some("cancel"));
        assert_eq!(queue.depth(), (2, 0));
    }

    #[test]
    fn the_normal_lane_is_bounded_in_bytes_too() {
        let queue = WriteQueue::new(100, 2048, 4);
        queue.push(Lane::Normal, "x".repeat(1500)).unwrap();
        assert!(matches!(queue.push(Lane::Normal, "y".repeat(1500)), Err(AcpError::QueueFull)));
        assert_eq!(queue.pop_blocking().unwrap().len(), 1500);
        queue.push(Lane::Normal, "y".repeat(1500)).unwrap();
    }

    #[test]
    fn closing_wakes_the_writer_and_refuses_new_lines() {
        let queue = std::sync::Arc::new(WriteQueue::new(4, 10_000, 4));
        let waiter = {
            let queue = queue.clone();
            std::thread::spawn(move || queue.pop_blocking())
        };
        std::thread::sleep(std::time::Duration::from_millis(50));
        queue.close();
        assert_eq!(waiter.join().unwrap(), None);
        assert!(matches!(queue.push(Lane::Normal, "late".into()), Err(AcpError::Closed)));
        assert!(matches!(queue.push(Lane::Priority, "late".into()), Err(AcpError::Closed)));
    }
}
