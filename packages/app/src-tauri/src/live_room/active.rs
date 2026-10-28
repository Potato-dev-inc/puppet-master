//! `active.json`: the one file that lets the app end a room after a crash (SPEC-n32 3.5, 4 #13).
//!
//! While a room is live the app keeps `{relay, room_id, signing_key_b64, ends_at_ms}` here, as a private file. It
//! holds no content key and no read token: the room key can only delete or post to that one room, which expires by
//! itself. At the next start the leftover room is deleted on the relay and the file is removed. A clean stop
//! removes it. Nothing here is logged: the room id and the key are parts of the link.

use std::path::{Path, PathBuf};
use std::time::Duration;

use ed25519_dalek::SigningKey;
use pm_relay_protocol::room::room_id;
use pm_relay_protocol::{b64, unb64};
use serde::{Deserialize, Serialize};

use super::client::{ClientError, RoomClient};

const DIR: &str = "live_room";
const FILE: &str = "active.json";
/// Calls to the relay at boot before a leftover room is left for the next start.
const CLEANUP_TRIES: u32 = 3;
const CLEANUP_PAUSE: Duration = Duration::from_secs(5);

/// What the file holds. Field names are the file's.
#[derive(Serialize, Deserialize)]
pub struct Active {
    pub relay: String,
    pub room_id: String,
    pub signing_key_b64: String,
    pub ends_at_ms: u64,
}

impl Active {
    pub fn new(relay: &str, key: &SigningKey, ends_at_ms: u64) -> Self {
        Self {
            relay: relay.to_string(),
            room_id: room_id(key.verifying_key().as_bytes()),
            signing_key_b64: b64(&key.to_bytes()),
            ends_at_ms,
        }
    }

    /// The room key the file holds, if it is a valid 32-byte key that names `room_id`.
    fn key(&self) -> Option<SigningKey> {
        let seed: [u8; 32] = unb64(&self.signing_key_b64)?.try_into().ok()?;
        let key = SigningKey::from_bytes(&seed);
        (room_id(key.verifying_key().as_bytes()) == self.room_id).then_some(key)
    }
}

/// `<app data>/live_room/active.json`.
pub fn path(data_dir: &Path) -> PathBuf {
    data_dir.join(DIR).join(FILE)
}

/// Writes the file (private, atomic). An error means the room must not start: without the file a crash could
/// leave a room nobody can end.
pub fn write(data_dir: &Path, active: &Active) -> Result<(), String> {
    let text = serde_json::to_string(active).map_err(|_| "The Live Room could not be prepared.".to_string())?;
    crate::secure_file::write_private(&path(data_dir), &text).map_err(|_| "The Live Room could not be prepared on this PC.".to_string())
}

fn read(data_dir: &Path) -> Option<Active> {
    let text = std::fs::read_to_string(path(data_dir)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Removes the file, but only when it still names `room`: a room that ends late must not remove the file of the
/// room that started after it.
pub fn remove_if(data_dir: &Path, room: &str) {
    if read(data_dir).is_some_and(|active| active.room_id == room) {
        let _ = std::fs::remove_file(path(data_dir));
    }
}

fn remove(data_dir: &Path) {
    let _ = std::fs::remove_file(path(data_dir));
}

/// What `cleanup_leftover` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cleanup {
    /// No file.
    Nothing,
    /// The file was damaged or already expired: it was removed without a call.
    Dropped,
    /// The relay was asked to end the room (or no longer knows it): the file was removed.
    Ended,
    /// The relay could not be reached: the file stays, so the next start tries again. The room ends by itself
    /// when its time is up.
    KeptForNextStart,
}

/// Ends a room a crash left behind. Run once at start-up, on its own thread (it may wait for the network).
pub fn cleanup_leftover(data_dir: &Path, now_ms: u64) -> Cleanup {
    cleanup_with(data_dir, now_ms, CLEANUP_TRIES, CLEANUP_PAUSE)
}

pub fn cleanup_with(data_dir: &Path, now_ms: u64, tries: u32, pause: Duration) -> Cleanup {
    if !path(data_dir).exists() {
        return Cleanup::Nothing;
    }
    let Some(active) = read(data_dir) else {
        remove(data_dir);
        return Cleanup::Dropped;
    };
    let Some(key) = active.key() else {
        remove(data_dir);
        return Cleanup::Dropped;
    };
    // A room past its time is already over on the relay.
    if active.ends_at_ms <= now_ms {
        remove(data_dir);
        return Cleanup::Dropped;
    }
    let Ok(client) = RoomClient::new(&active.relay, key) else {
        remove(data_dir);
        return Cleanup::Dropped;
    };
    for attempt in 0..tries.max(1) {
        match client.delete() {
            Ok(()) | Err(ClientError::Gone(_)) | Err(ClientError::Refused(_)) => {
                remove(data_dir);
                return Cleanup::Ended;
            }
            Err(ClientError::Network(_)) => {
                if attempt + 1 < tries {
                    std::thread::sleep(pause);
                }
            }
        }
    }
    Cleanup::KeptForNextStart
}

#[cfg(test)]
#[path = "active_tests.rs"]
mod tests;
