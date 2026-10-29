//! A Chat tab over a fake pane: the real screen reader (`session_chat::screen`) on the real pane registry, with
//! a viewer that merges what it is sent the way the UI does (an item sent again under its id is replaced in place,
//! a reset drops everything shown before).

use parking_lot::Mutex;
use serde_json::Value;
use std::sync::Arc;

use crate::pty::PaneRegistry;
use crate::session_chat::{read_pane, ChatItem, PaneFacts, PaneLookup, ScreenRead, Sink, Watchers};

/// What the viewer holds, and how it got there.
#[derive(Default)]
struct Viewer {
    items: Vec<ChatItem>,
    batches: usize,
    resets: usize,
}

impl Sink for Mutex<Viewer> {
    fn send(&self, _pane_id: &str, items: Vec<ChatItem>, reset: bool) {
        let mut viewer = self.lock();
        viewer.batches += 1;
        if reset {
            viewer.resets += 1;
            viewer.items = items;
            return;
        }
        for item in items {
            match viewer.items.iter().position(|known| known.id() == item.id()) {
                Some(at) => viewer.items[at] = item,
                None => viewer.items.push(item),
            }
        }
    }
}

struct Screens(Arc<Mutex<PaneRegistry>>);

impl PaneLookup for Screens {
    fn facts(&self, _pane_id: &str) -> Option<PaneFacts> {
        None
    }

    fn screen(&self, pane_id: &str, known: Option<u64>) -> Option<ScreenRead> {
        read_pane(&self.0, pane_id, known)
    }
}

/// An open chat over one pane. Dropping it stops its reader.
pub struct ChatProbe {
    watchers: Watchers,
    viewer: Arc<Mutex<Viewer>>,
}

impl ChatProbe {
    pub(super) fn open(registry: Arc<Mutex<PaneRegistry>>, pane_id: &str, agent_type: &str) -> Self {
        let viewer = Arc::new(Mutex::new(Viewer::default()));
        let watchers = Watchers::new(viewer.clone(), Arc::new(Screens(registry)));
        let first = watchers.open_screen(pane_id, "w", agent_type);
        {
            let mut shown = viewer.lock();
            if shown.batches == 0 {
                shown.items = first;
            }
        }
        ChatProbe { watchers, viewer }
    }

    /// The Chat tab of a pane that writes a session file: the app's own pane lookup (`RegistryPanes`, the facts a Linux
    /// pane carries) and the session source of `source_agent` (the real agent whose file format the fake writes).
    pub(super) fn open_files(registry: Arc<Mutex<PaneRegistry>>, pane_id: &str, source_agent: &str) -> Self {
        let source = crate::tui::profile_for(source_agent)
            .session_chat()
            .unwrap_or_else(|| panic!("{source_agent} has no session file source"));
        let viewer = Arc::new(Mutex::new(Viewer::default()));
        let watchers = Watchers::new(viewer.clone(), Arc::new(crate::commands::RegistryPanes(registry)));
        let first = watchers.open(pane_id, "w", source);
        {
            let mut shown = viewer.lock();
            if shown.batches == 0 {
                shown.items = first;
            }
        }
        ChatProbe { watchers, viewer }
    }

    /// The items the viewer shows now, as the UI receives them (camelCase JSON).
    pub fn items(&self) -> Vec<Value> {
        self.viewer.lock().items.iter().map(|item| serde_json::to_value(item).expect("an item serialises")).collect()
    }

    /// The ids of the items shown, in order.
    pub fn ids(&self) -> Vec<String> {
        self.viewer.lock().items.iter().map(|item| item.id().to_string()).collect()
    }

    /// Batches sent after the first look, and how many of them were resets.
    pub fn sent(&self) -> (usize, usize) {
        let viewer = self.viewer.lock();
        (viewer.batches, viewer.resets)
    }
}

impl Drop for ChatProbe {
    fn drop(&mut self) {
        self.watchers.close_all();
    }
}
