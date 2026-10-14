//! `FolderTransport`: the v0 wire. Every event is one file `<ulid>.json` in a folder all teammates
//! can reach (a shared or synced folder, or one folder for two profiles on one computer). A
//! `notify` watcher wakes the reader; a slow rescan catches anything a network share's watcher
//! misses. No server, no crypto: it is a demo and test transport, not the v1 product wire.
//!
//! Files are written to a dot-prefixed temp name and renamed into place, so a reader never sees
//! half an event. Names that are not `<ULID>.json`, files over `MAX_EVENT_BYTES` and files that do
//! not parse are skipped (once) without failing the others.

use super::{Inbound, TeamTransport, TransportError};
use crate::team::event::{is_event_id, TeamEvent, MAX_EVENT_BYTES};
use notify::{RecursiveMode, Watcher};
use parking_lot::Mutex;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::Duration;

/// How often the folder is rescanned even when the watcher stays quiet.
const RESCAN: Duration = Duration::from_secs(3);

pub struct FolderTransport {
    folder: PathBuf,
    stops: Mutex<Vec<Arc<AtomicBool>>>,
}

impl FolderTransport {
    pub fn new(folder: impl Into<PathBuf>) -> Self {
        Self { folder: folder.into(), stops: Mutex::new(Vec::new()) }
    }

    fn file_name(id: &str) -> String {
        format!("{id}.json")
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// The event id a file name carries, when it is an event file.
fn event_id_of(name: &str) -> Option<&str> {
    let id = name.strip_suffix(".json")?;
    is_event_id(id).then_some(id)
}

/// Read every not-yet-seen event file in `folder` and send it on `tx`. Returns false once the
/// receiver is gone.
fn scan(folder: &Path, seen: &mut HashSet<String>, tx: &Sender<Inbound>) -> bool {
    let Ok(read) = std::fs::read_dir(folder) else {
        return true; // the folder may appear later (a share that mounts late)
    };
    let mut names: Vec<String> = read
        .filter_map(Result::ok)
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|name| event_id_of(name).is_some() && !seen.contains(name))
        .collect();
    names.sort(); // ULIDs sort by time: oldest first
    for name in names {
        let path = folder.join(&name);
        let too_big = std::fs::metadata(&path).map(|m| m.len() as usize > MAX_EVENT_BYTES).unwrap_or(true);
        seen.insert(name.clone());
        if too_big {
            tracing::debug!(file = %name, "team: skipped an event file that is too large");
            continue;
        }
        let parsed = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<TeamEvent>(&bytes).ok());
        let Some(event) = parsed else {
            tracing::debug!(file = %name, "team: skipped an event file that does not parse");
            continue;
        };
        // The name must match the id inside, so one file cannot pose as another event.
        if event_id_of(&name) != Some(event.id.as_str()) {
            continue;
        }
        if tx.send(Inbound { event, received_at_ms: now_ms() }).is_err() {
            return false;
        }
    }
    true
}

impl TeamTransport for FolderTransport {
    fn kind(&self) -> &'static str {
        "folder"
    }

    fn send(&self, event: &TeamEvent) -> Result<(), TransportError> {
        if !is_event_id(&event.id) {
            return Err(TransportError("the event has no valid id".into()));
        }
        let json = serde_json::to_vec_pretty(event).map_err(|e| TransportError(e.to_string()))?;
        if json.len() > MAX_EVENT_BYTES {
            return Err(TransportError("the event is too large to send".into()));
        }
        std::fs::create_dir_all(&self.folder)
            .map_err(|e| TransportError(format!("could not open the team folder {}: {e}", self.folder.display())))?;
        let name = Self::file_name(&event.id);
        let tmp = self.folder.join(format!(".{name}.tmp"));
        std::fs::write(&tmp, &json)
            .map_err(|e| TransportError(format!("could not write to the team folder: {e}")))?;
        std::fs::rename(&tmp, self.folder.join(&name)).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            TransportError(format!("could not write to the team folder: {e}"))
        })
    }

    fn subscribe(&self) -> Result<Receiver<Inbound>, TransportError> {
        let (tx, rx) = mpsc::channel::<Inbound>();
        let stop = Arc::new(AtomicBool::new(false));
        self.stops.lock().push(stop.clone());
        let folder = self.folder.clone();
        std::thread::Builder::new()
            .name("team-folder".into())
            .spawn(move || {
                let (wake_tx, wake_rx) = mpsc::channel::<()>();
                // The watcher is best effort: without it the rescan still delivers within RESCAN.
                let mut watcher = notify::recommended_watcher(move |_res: notify::Result<notify::Event>| {
                    let _ = wake_tx.send(());
                })
                .ok();
                let mut watching = false;
                let mut seen = HashSet::new();
                loop {
                    if stop.load(Ordering::SeqCst) {
                        break;
                    }
                    if !watching && folder.is_dir() {
                        if let Some(w) = watcher.as_mut() {
                            watching = w.watch(&folder, RecursiveMode::NonRecursive).is_ok();
                        }
                    }
                    if !scan(&folder, &mut seen, &tx) {
                        break;
                    }
                    match wake_rx.recv_timeout(RESCAN) {
                        Ok(()) => {
                            // Coalesce a burst of file events into one scan.
                            std::thread::sleep(Duration::from_millis(60));
                            while wake_rx.try_recv().is_ok() {}
                        }
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => std::thread::sleep(RESCAN),
                    }
                }
            })
            .map_err(|e| TransportError(format!("could not start the team folder reader: {e}")))?;
        Ok(rx)
    }

    fn stop(&self) {
        for stop in self.stops.lock().drain(..) {
            stop.store(true, Ordering::SeqCst);
        }
    }
}

impl Drop for FolderTransport {
    fn drop(&mut self) {
        self.stop();
    }
}
