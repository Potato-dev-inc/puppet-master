//! `FolderTransport`: a shared folder as the wire (a synced or network folder, or one folder for two
//! profiles on one computer). It is a relay without a server:
//!
//! ```text
//! <folder>/envelopes/<ULID>.json   sealed, signed envelopes (pruned after 7 days)
//! <folder>/state/<slot>.json       the signed `roster` and `keys` slots
//! <folder>/joins/<random>.json     sealed join requests
//! ```
//!
//! Since v1 everything in the folder is sealed and signed: a file someone drops in without a
//! team device key is refused by `secure::Session::open` before it can become a card (v0 let
//! anyone who could write the folder create one).
//!
//! Files are written to a dot-prefixed temp name and renamed into place, so a reader never sees
//! half a file. Names that do not fit, files over the size limit and files that do not parse are
//! skipped (once) without failing the others.

use super::{now_ms, Inbound, TeamTransport, TransportError};
use crate::team::event::is_event_id;
use notify::{RecursiveMode, Watcher};
use parking_lot::Mutex;
use pm_relay_protocol::{Envelope, JoinBox, SignedBlob, MAILBOX_TTL_MS, MAX_ENVELOPE_BYTES, MAX_JOIN_BYTES, MAX_STATE_BYTES, SLOTS};
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

    fn envelopes(&self) -> PathBuf {
        self.folder.join("envelopes")
    }
}

/// Write `bytes` to `dir/name` through a temp file and a rename.
fn write_atomic(dir: &Path, name: &str, bytes: &[u8]) -> Result<(), TransportError> {
    std::fs::create_dir_all(dir)
        .map_err(|e| TransportError(format!("could not open the team folder {}: {e}", dir.display())))?;
    let tmp = dir.join(format!(".{name}.{}.tmp", uuid::Uuid::new_v4().simple()));
    std::fs::write(&tmp, bytes).map_err(|e| TransportError(format!("could not write to the team folder: {e}")))?;
    std::fs::rename(&tmp, dir.join(name)).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        TransportError(format!("could not write to the team folder: {e}"))
    })
}

fn read_capped(path: &Path, cap: usize) -> Option<Vec<u8>> {
    let len = std::fs::metadata(path).ok()?.len() as usize;
    if len > cap {
        return None;
    }
    std::fs::read(path).ok()
}

/// The envelope id a file name carries.
fn envelope_id_of(name: &str) -> Option<&str> {
    let id = name.strip_suffix(".json")?;
    is_event_id(id).then_some(id)
}

/// Read every not-yet-seen envelope file and send it on `tx`; prune files past the mailbox TTL.
/// Returns false once the receiver is gone.
fn scan(dir: &Path, seen: &mut HashSet<String>, tx: &Sender<Inbound>) -> bool {
    let Ok(read) = std::fs::read_dir(dir) else {
        return true; // the folder may appear later (a share that mounts late)
    };
    let now = now_ms();
    let mut names: Vec<String> = read
        .filter_map(Result::ok)
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|name| envelope_id_of(name).is_some() && !seen.contains(name))
        .collect();
    names.sort(); // ULIDs sort by time: oldest first
    for name in names {
        let path = dir.join(&name);
        seen.insert(name.clone());
        let old = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age.as_millis() as u64 > MAILBOX_TTL_MS);
        if old {
            let _ = std::fs::remove_file(&path);
            continue;
        }
        let Some(envelope) = read_capped(&path, MAX_ENVELOPE_BYTES).and_then(|b| serde_json::from_slice::<Envelope>(&b).ok())
        else {
            tracing::debug!(file = %name, "team: skipped a folder file that is not an envelope");
            continue;
        };
        // The name must match the id inside, so one file cannot pose as another.
        if envelope_id_of(&name) != Some(envelope.id.as_str()) {
            continue;
        }
        if tx.send(Inbound { envelope, received_at_ms: now, cursor: 0, reset: false }).is_err() {
            return false;
        }
    }
    true
}

impl TeamTransport for FolderTransport {
    fn kind(&self) -> &'static str {
        "folder"
    }

    fn send(&self, envelope: &Envelope) -> Result<(), TransportError> {
        envelope.check_shape().map_err(TransportError::new)?;
        let json = serde_json::to_vec(envelope).map_err(|e| TransportError(e.to_string()))?;
        if json.len() > MAX_ENVELOPE_BYTES {
            return Err(TransportError::new("the update is too large to send"));
        }
        write_atomic(&self.envelopes(), &format!("{}.json", envelope.id), &json)
    }

    fn subscribe(&self, _after: u64) -> Result<Receiver<Inbound>, TransportError> {
        let (tx, rx) = mpsc::channel::<Inbound>();
        let stop = Arc::new(AtomicBool::new(false));
        self.stops.lock().push(stop.clone());
        let dir = self.envelopes();
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
                    if !watching && dir.is_dir() {
                        if let Some(w) = watcher.as_mut() {
                            watching = w.watch(&dir, RecursiveMode::NonRecursive).is_ok();
                        }
                    }
                    if !scan(&dir, &mut seen, &tx) {
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

    fn put_state(&self, blob: &SignedBlob) -> Result<(), TransportError> {
        if !SLOTS.contains(&blob.slot.as_str()) {
            return Err(TransportError::new("unknown state slot"));
        }
        // Never go back: an older version does not replace a newer one.
        if let Ok(Some(current)) = self.get_state(&blob.slot) {
            if current.version > blob.version {
                return Err(TransportError::new("a newer team list is already there"));
            }
        }
        let json = serde_json::to_vec(blob).map_err(|e| TransportError(e.to_string()))?;
        write_atomic(&self.folder.join("state"), &format!("{}.json", blob.slot), &json)
    }

    fn get_state(&self, slot: &str) -> Result<Option<SignedBlob>, TransportError> {
        if !SLOTS.contains(&slot) {
            return Err(TransportError::new("unknown state slot"));
        }
        let path = self.folder.join("state").join(format!("{slot}.json"));
        if !path.exists() {
            return Ok(None);
        }
        let bytes = read_capped(&path, MAX_STATE_BYTES).ok_or_else(|| TransportError::new("the team list is too large"))?;
        serde_json::from_slice(&bytes).map(Some).map_err(|_| TransportError::new("the team list in the folder is damaged"))
    }

    fn post_join(&self, join: &JoinBox) -> Result<(), TransportError> {
        join.check_shape().map_err(TransportError::new)?;
        let json = serde_json::to_vec(join).map_err(|e| TransportError(e.to_string()))?;
        if json.len() > MAX_JOIN_BYTES {
            return Err(TransportError::new("the request is too large"));
        }
        write_atomic(&self.folder.join("joins"), &format!("{}.json", uuid::Uuid::new_v4().simple()), &json)
    }

    fn joins(&self) -> Result<Vec<JoinBox>, TransportError> {
        let dir = self.folder.join("joins");
        let Ok(read) = std::fs::read_dir(&dir) else { return Ok(Vec::new()) };
        let now = now_ms();
        let mut out = Vec::new();
        for entry in read.filter_map(Result::ok) {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let Some(join) = read_capped(&path, MAX_JOIN_BYTES).and_then(|b| serde_json::from_slice::<JoinBox>(&b).ok()) else {
                continue;
            };
            if join.at_ms + pm_relay_protocol::JOIN_TTL_MS < now {
                let _ = std::fs::remove_file(&path);
                continue;
            }
            out.push(join);
        }
        Ok(out)
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
