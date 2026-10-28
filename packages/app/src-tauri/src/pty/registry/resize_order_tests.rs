//! The flight recorder must see a resize BEFORE the PTY applies it (ConPTY repaints at the new size at once).

use super::*;
use crate::flight::ring::Kind;
use std::sync::atomic::{AtomicBool, Ordering};

/// A PTY that checks, inside `resize`, whether the recorder already holds the resize. It can also refuse.
struct SpyMaster {
    pane_id: String,
    recorded_first: Arc<AtomicBool>,
    refuse: bool,
}

impl MasterPty for SpyMaster {
    fn resize(&self, _size: PtySize) -> Result<(), anyhow::Error> {
        let ring = crate::flight::get(&self.pane_id).expect("ring");
        let seen = ring.snapshot().events.iter().any(|event| event.kind == Kind::Resize);
        self.recorded_first.store(seen, Ordering::SeqCst);
        if self.refuse {
            anyhow::bail!("refused");
        }
        Ok(())
    }
    fn get_size(&self) -> Result<PtySize, anyhow::Error> {
        Ok(PtySize { rows: 3, cols: 10, pixel_width: 0, pixel_height: 0 })
    }
    fn try_clone_reader(&self) -> Result<Box<dyn std::io::Read + Send>, anyhow::Error> {
        anyhow::bail!("not used")
    }
    fn take_writer(&self) -> Result<Box<dyn std::io::Write + Send>, anyhow::Error> {
        anyhow::bail!("not used")
    }
    #[cfg(unix)]
    fn process_group_leader(&self) -> Option<libc::pid_t> {
        None
    }
    #[cfg(unix)]
    fn as_raw_fd(&self) -> Option<portable_pty::unix::RawFd> {
        None
    }
}

fn pane_with_spy(id: &str, refuse: bool) -> (Mutex<PaneRegistry>, Arc<AtomicBool>) {
    let mut cmd = CommandBuilder::new(if cfg!(windows) { "ping" } else { "sh" });
    if cfg!(windows) {
        cmd.args(["-n", "60", "127.0.0.1"]);
    } else {
        cmd.args(["-c", "sleep 60"]);
    }
    let pane = PaneRegistry::test_pane_running(id, cmd);
    let flag = Arc::new(AtomicBool::new(false));
    *pane.master.lock() = Box::new(SpyMaster { pane_id: id.to_string(), recorded_first: flag.clone(), refuse });
    let registry = Mutex::new(PaneRegistry::default());
    registry.lock().panes.insert(id.into(), pane);
    crate::flight::start(id, 10, 3);
    (registry, flag)
}

fn resize_events(id: &str) -> Vec<(u16, u16)> {
    let ring = crate::flight::get(id).expect("ring");
    ring.snapshot()
        .events
        .iter()
        .filter(|event| event.kind == Kind::Resize)
        .map(|event| ((event.aux >> 16) as u16, (event.aux & 0xffff) as u16))
        .collect()
}

#[test]
fn the_recorder_holds_the_resize_before_the_pty_changes_size() {
    let id = "resize-order-ok";
    let (registry, recorded_first) = pane_with_spy(id, false);
    assert_eq!(resize(&registry, id, 40, 12), Ok(true));
    assert!(recorded_first.load(Ordering::SeqCst), "the PTY was resized before the recorder knew");
    assert_eq!(resize_events(id), vec![(40, 12)]);
    kill_pane_and_wait(&registry, id, Some("test")).unwrap();
}

#[test]
fn a_refused_resize_puts_the_old_grid_back_in_the_recording() {
    let id = "resize-order-refused";
    let (registry, recorded_first) = pane_with_spy(id, true);
    assert!(resize(&registry, id, 40, 12).is_err());
    assert!(recorded_first.load(Ordering::SeqCst));
    // New grid, then the old one again: a replay ends on the grid the PTY really has.
    assert_eq!(resize_events(id), vec![(40, 12), (10, 3)]);
    kill_pane_and_wait(&registry, id, Some("test")).unwrap();
}
