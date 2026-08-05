use std::process::Child as StdChild;
use std::sync::{Arc, Mutex};

#[derive(Debug)]
pub struct OpenCodeLink {
    pub base_url: String,
    pub session_id: String,
    serve_child: Arc<Mutex<Option<StdChild>>>,
}

impl OpenCodeLink {
    pub fn new(base_url: String, session_id: String, child: StdChild) -> Self {
        Self {
            base_url,
            session_id,
            serve_child: Arc::new(Mutex::new(Some(child))),
        }
    }

    pub fn serve_handle(&self) -> Arc<Mutex<Option<StdChild>>> {
        Arc::clone(&self.serve_child)
    }

    pub fn kill_serve(&self) {
        if let Ok(mut guard) = self.serve_child.lock() {
            if let Some(mut child) = guard.take() {
                let _ = child.kill();
            }
        }
    }
}
