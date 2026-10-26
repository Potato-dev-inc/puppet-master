//! Reads a pane's screen from the pane registry, for the watcher.

use parking_lot::Mutex;

use super::ScreenRead;
use crate::pty::PaneRegistry;

/// The pane's text with its scrollback and its status. `known`: the output position of the text read last; when the
/// pane has taken in no output (and has not been resized) since, the answer carries no text and nothing is read.
/// `None` when the pane is gone.
pub fn read_pane(registry: &Mutex<PaneRegistry>, pane_id: &str, known: Option<u64>) -> Option<ScreenRead> {
    let (status, size) = {
        let registry = registry.lock();
        let pane = registry.get(pane_id)?;
        let size = pane.screen.lock().screen().size();
        let status = pane.status.lock().as_str().to_string();
        (status, size)
    };
    // Output bytes the emulator has taken in, with the grid size mixed in: a resize rewraps the text.
    let position = crate::pty::stream_pos::get(pane_id).map(|pos| {
        pos.processed.load(std::sync::atomic::Ordering::SeqCst) ^ (u64::from(size.0) << 48) ^ (u64::from(size.1) << 32)
    });
    let text = if position.is_some() && position == known { None } else { crate::pty::registry_read_full_text(registry, pane_id).ok() };
    Some(ScreenRead { status, position, text })
}
