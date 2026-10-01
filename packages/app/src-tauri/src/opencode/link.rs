use std::process::Child as StdChild;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use parking_lot::Mutex as PMutex;

use super::OpenCodeModelRef;

#[derive(Debug)]
pub struct OpenCodeLink {
    pub base_url: String,
    pub session_id: String,
    /// OpenCode project directory (must be passed as `?directory=` on API calls).
    pub directory: String,
    serve_child: Arc<Mutex<Option<StdChild>>>,
    /// When true, attach EOF must not tear down `opencode serve` (reattach in progress).
    reattaching: Arc<AtomicBool>,
    /// When false, attach EOF should kill serve (pane teardown).
    keep_serve_on_attach_exit: Arc<AtomicBool>,
    /// Bumped on each attach respawn so stale reader threads ignore EOF.
    attach_generation: Arc<AtomicU64>,
    /// Model switch in flight — cleared when `model_ready` wait predicate matches.
    pending_model: Arc<PMutex<Option<OpenCodeModelRef>>>,
}
impl OpenCodeLink {
    pub fn new(base_url: String, session_id: String, directory: String, child: StdChild) -> Self {
        Self {
            base_url,
            session_id,
            directory,
            serve_child: Arc::new(Mutex::new(Some(child))),
            reattaching: Arc::new(AtomicBool::new(false)),
            keep_serve_on_attach_exit: Arc::new(AtomicBool::new(true)),
            attach_generation: Arc::new(AtomicU64::new(0)),
            pending_model: Arc::new(PMutex::new(None)),
        }
    }

    pub fn pending_model(&self) -> Arc<PMutex<Option<OpenCodeModelRef>>> {
        Arc::clone(&self.pending_model)
    }

    pub fn serve_handle(&self) -> Arc<Mutex<Option<StdChild>>> {
        Arc::clone(&self.serve_child)
    }

    pub fn reattaching(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.reattaching)
    }

    pub fn keep_serve_on_attach_exit(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.keep_serve_on_attach_exit)
    }

    pub fn attach_generation(&self) -> Arc<AtomicU64> {
        Arc::clone(&self.attach_generation)
    }

    pub fn bump_attach_generation(&self) -> u64 {
        self.attach_generation.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub fn kill_serve(&self) {
        self.keep_serve_on_attach_exit
            .store(false, Ordering::SeqCst);
        if let Ok(mut guard) = self.serve_child.lock() {
            if let Some(mut child) = guard.take() {
                crate::pty::proc_tree::kill_process_tree(child.id());
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pty::proc_tree::test_support::*;

    #[test]
    fn kill_serve_terminates_serve_child_and_its_grandchild() {
        let (child, grandchild) = spawn_child_with_grandchild();
        let root = child.id();
        let link = OpenCodeLink::new("http://127.0.0.1:1".into(), "s".into(), ".".into(), child);
        link.kill_serve();
        assert!(wait_until_dead(root), "serve child still alive");
        assert!(wait_until_dead(grandchild), "serve grandchild leaked");
    }
}
