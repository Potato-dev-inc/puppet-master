//! Plan tokens, the staged video and the one-time save for Clip It.
//!
//! `Plans::issue` hands out a token for one turn. `stage` keeps the video the webview drew under that token, but only
//! when the bytes look like the kind of video the token asked for. `save` writes the staged bytes to a path the save
//! dialog gave, checked by `check_dest_with` for the same extension. A refused path or a failed write keeps the bytes
//! so the person can pick another place; a good save uses them up (the plan stays, so "Save another copy" only
//! stages again). A plan lasts 30 minutes, and at most four are kept.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use parking_lot::Mutex;

use super::model::{MAX_STAGE_BYTES, PLAN_TTL};
use crate::session_chat::export_save::check_dest_with;

/// Plans kept at once (the oldest goes first).
const KEEP_PLANS: usize = 4;

const GONE: &str = "This clip is no longer available. Make the clip again.";
const NOTHING_STAGED: &str = "The video is not ready yet. Wait for it to finish drawing.";
const NOT_VIDEO_TYPE: &str = "Only mp4 and webm videos can be saved.";
const TOO_BIG: &str = "This video is too big to keep. Make a shorter clip.";

/// The video kinds a clip can be. Returns the static name when `ext` is one of them.
fn video_ext(ext: &str) -> Option<&'static str> {
    match ext {
        "mp4" => Some("mp4"),
        "webm" => Some("webm"),
        _ => None,
    }
}

/// Whether `bytes` start like a video of kind `ext`: an mp4 starts with a sane `ftyp` box, a webm with the EBML
/// header. Nothing else about the file is checked here; the player decides the rest.
fn looks_like(ext: &str, bytes: &[u8]) -> bool {
    match ext {
        "mp4" => {
            if bytes.len() < 8 || &bytes[4..8] != b"ftyp" {
                return false;
            }
            let size = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
            (8..=bytes.len()).contains(&size)
        }
        "webm" => bytes.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]),
        _ => false,
    }
}

fn need_message(ext: &str) -> String {
    format!("Choose a file name that ends in .{ext}.")
}

/// A plan is usable for `PLAN_TTL` after it was issued.
fn is_live(plan: &Plan, now: Instant) -> bool {
    now.saturating_duration_since(plan.issued) < PLAN_TTL
}

fn find_live<'a>(held: &'a mut Vec<Plan>, token: &str, now: Instant) -> Result<&'a mut Plan, String> {
    held.iter_mut().find(|plan| plan.token == token && is_live(plan, now)).ok_or_else(|| GONE.to_string())
}

struct Staged {
    ext: &'static str,
    bytes: Vec<u8>,
}

struct Plan {
    token: String,
    pane_id: String,
    issued: Instant,
    staged: Option<Staged>,
}

/// The plans issued and their staged videos.
#[derive(Default)]
pub struct Plans {
    held: Mutex<Vec<Plan>>,
}

/// What the clip commands share. Managed by `lib.rs`.
#[derive(Default)]
pub struct ClipState {
    pub plans: Plans,
}

impl Plans {
    /// Issues a token for a plan about `pane_id`. Expired plans are dropped first; then the oldest go once more than
    /// `KEEP_PLANS` are held.
    pub fn issue(&self, pane_id: &str) -> String {
        self.issue_at(pane_id, Instant::now())
    }

    fn issue_at(&self, pane_id: &str, now: Instant) -> String {
        let token = uuid::Uuid::new_v4().to_string();
        let mut held = self.held.lock();
        held.retain(|plan| is_live(plan, now));
        held.push(Plan { token: token.clone(), pane_id: pane_id.to_string(), issued: now, staged: None });
        while held.len() > KEEP_PLANS {
            held.remove(0);
        }
        token
    }

    /// Keeps `bytes` as the video of the plan `token`, replacing any video staged before. Returns the byte count.
    /// A refused video leaves the earlier staged bytes as they were.
    pub fn stage(&self, token: &str, ext: &str, bytes: Vec<u8>) -> Result<u64, String> {
        self.stage_at(token, ext, bytes, Instant::now())
    }

    fn stage_at(&self, token: &str, ext: &str, bytes: Vec<u8>, now: Instant) -> Result<u64, String> {
        let mut held = self.held.lock();
        find_live(&mut held, token, now)?;
        let ext = video_ext(ext).ok_or_else(|| NOT_VIDEO_TYPE.to_string())?;
        if bytes.len() > MAX_STAGE_BYTES {
            return Err(TOO_BIG.to_string());
        }
        if !looks_like(ext, &bytes) {
            return Err(format!("The video data is not a valid {ext} file."));
        }
        let len = bytes.len() as u64;
        let plan = find_live(&mut held, token, now)?;
        plan.staged = Some(Staged { ext, bytes });
        Ok(len)
    }

    /// Writes the staged video of `token` to `path`. The path must end in the staged kind's extension. The staged
    /// bytes are used up only when the file is written; a refused path or a failed write keeps them.
    pub fn save(&self, token: &str, path: &str) -> Result<PathBuf, String> {
        self.save_at(token, path, Instant::now())
    }

    fn save_at(&self, token: &str, path: &str, now: Instant) -> Result<PathBuf, String> {
        let (dest, staged) = {
            let mut held = self.held.lock();
            let plan = find_live(&mut held, token, now)?;
            let ext = plan.staged.as_ref().map(|staged| staged.ext).ok_or_else(|| NOTHING_STAGED.to_string())?;
            let dest = check_dest_with(path, &[ext], &need_message(ext))?;
            let staged = plan.staged.take().ok_or_else(|| NOTHING_STAGED.to_string())?;
            (dest, staged)
        };
        match write_file(&dest, &staged.bytes) {
            Ok(()) => Ok(dest),
            Err(error) => {
                let mut held = self.held.lock();
                if let Ok(plan) = find_live(&mut held, token, now) {
                    if plan.staged.is_none() {
                        plan.staged = Some(staged);
                    }
                }
                Err(format!("Could not write the file: {error}"))
            }
        }
    }

    #[cfg(test)]
    fn count(&self) -> usize {
        self.held.lock().len()
    }

    #[cfg(test)]
    fn staged_len(&self, token: &str) -> Option<usize> {
        self.held.lock().iter().find(|plan| plan.token == token).and_then(|plan| plan.staged.as_ref().map(|s| s.bytes.len()))
    }

    #[cfg(test)]
    fn pane_of(&self, token: &str) -> Option<String> {
        self.held.lock().iter().find(|plan| plan.token == token).map(|plan| plan.pane_id.clone())
    }
}

/// An existing plain file is replaced; a missing one is created, and `create_new` refuses a name that appeared since
/// the check. The size cap is the stage cap, so this has no cap of its own.
fn write_file(dest: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true);
    if dest.exists() {
        options.truncate(true);
    } else {
        options.create_new(true);
    }
    let mut file = options.open(dest)?;
    file.write_all(bytes)?;
    file.flush()
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
