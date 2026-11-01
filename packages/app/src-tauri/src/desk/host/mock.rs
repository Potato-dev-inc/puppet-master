//! A pretend PC for tests. It owns a list of screens and a cursor that is only a number pair, and runs a real
//! "hook thread" that feeds simulated keys through the same [`HotkeyMatcher`] the Windows hook uses. Nothing
//! here touches the operating system.

use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle, ThreadId};

use super::super::engine::hotkeys::{new_table, Binding, HotkeyMatcher, Table, Verdict};
use super::{DeskHost, DisplayInfo, HookHealth, HostEvent, HostSink};

type KeyMsg = (u16, bool, bool, Sender<Verdict>);

struct HookThread {
    id: ThreadId,
    tx: Sender<KeyMsg>,
    join: JoinHandle<()>,
}

#[derive(Default)]
pub struct MockState {
    pub displays: Vec<DisplayInfo>,
    pub cursor: (i32, i32),
    /// Every `move_cursor` that worked.
    pub moves: Vec<(i32, i32)>,
    /// The thread that last moved the cursor (the engine thread).
    pub move_thread: Option<ThreadId>,
    pub fail_move: bool,
    pub fail_install: bool,
    pub fail_displays: bool,
    pub installs: u32,
    pub removes: u32,
    pub releases: u32,
    /// The hook was "dropped by Windows": the thread is gone, nobody told the engine.
    pub dropped: bool,
    pub unsupported: bool,
    /// The last tables handed to `set_hotkeys`.
    pub tables_set: Vec<Vec<Binding>>,
    hook: Option<HookThread>,
}

#[derive(Clone)]
pub struct MockHost {
    pub state: Arc<Mutex<MockState>>,
    table: Table,
}

pub fn display(id: &str, x: i32, y: i32, w: u32, h: u32, scale: f64, primary: bool) -> DisplayInfo {
    DisplayInfo { id: id.into(), name: id.into(), x, y, width: w, height: h, scale, primary }
}

impl MockHost {
    pub fn new(displays: Vec<DisplayInfo>, cursor: (i32, i32)) -> Self {
        let state = MockState { displays, cursor, ..Default::default() };
        Self { state: Arc::new(Mutex::new(state)), table: new_table() }
    }

    pub fn with<R>(&self, f: impl FnOnce(&mut MockState) -> R) -> R {
        f(&mut self.state.lock().unwrap())
    }

    pub fn cursor(&self) -> (i32, i32) {
        self.with(|s| s.cursor)
    }

    pub fn hook_installed(&self) -> bool {
        self.with(|s| s.hook.is_some())
    }

    /// The id of the thread that runs the (pretend) hook, if one is installed.
    pub fn hook_thread_id(&self) -> Option<ThreadId> {
        self.with(|s| s.hook.as_ref().map(|h| h.id))
    }

    /// Press or release a key as the keyboard would. Returns what the hook decided.
    pub fn key(&self, hid: u16, down: bool, injected: bool) -> Verdict {
        let tx = self.with(|s| s.hook.as_ref().map(|h| h.tx.clone()));
        let Some(tx) = tx else { return Verdict::Pass };
        let (reply_tx, reply_rx) = mpsc::channel();
        if tx.send((hid, down, injected, reply_tx)).is_err() {
            return Verdict::Pass;
        }
        reply_rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap_or(Verdict::Pass)
    }

    /// Windows silently removes the hook: the thread ends, nobody is told.
    pub fn simulate_dropped_hook(&self) {
        let hook = self.with(|s| {
            s.dropped = true;
            s.hook.take()
        });
        if let Some(h) = hook {
            drop(h.tx);
            let _ = h.join.join();
        }
    }
}

impl DeskHost for MockHost {
    fn supported(&self) -> bool {
        !self.with(|s| s.unsupported)
    }

    fn displays(&self) -> Result<Vec<DisplayInfo>, String> {
        self.with(|s| if s.fail_displays { Err("display list failed".to_string()) } else { Ok(s.displays.clone()) })
    }

    fn cursor_pos(&self) -> Result<(i32, i32), String> {
        Ok(self.cursor())
    }

    fn move_cursor(&self, x: i32, y: i32) -> Result<(), String> {
        self.with(|s| {
            if s.fail_move {
                return Err("SetCursorPos failed".to_string());
            }
            s.cursor = (x, y);
            s.move_thread = Some(thread::current().id());
            s.moves.push((x, y));
            Ok(())
        })
    }

    fn install_hooks(&self, sink: HostSink) -> Result<(), String> {
        if self.with(|s| s.fail_install) {
            return Err("hook refused".into());
        }
        if self.hook_installed() {
            return Ok(());
        }
        let (tx, rx) = mpsc::channel::<KeyMsg>();
        let table = self.table.clone();
        let (id_tx, id_rx) = mpsc::channel();
        let join = thread::Builder::new()
            .name("desk-hook-mock".into())
            .spawn(move || {
                let _ = id_tx.send(thread::current().id());
                let mut matcher = HotkeyMatcher::new(table);
                while let Ok((hid, down, injected, reply)) = rx.recv() {
                    let verdict = matcher.on_key(hid, down, injected);
                    if let Verdict::Fire(action) = verdict {
                        sink(HostEvent::Hotkey(action));
                    }
                    let _ = reply.send(verdict);
                }
            })
            .map_err(|e| e.to_string())?;
        let id = id_rx.recv().map_err(|e| e.to_string())?;
        self.with(|s| {
            s.installs += 1;
            s.dropped = false;
            s.hook = Some(HookThread { id, tx, join });
        });
        Ok(())
    }

    fn remove_hooks(&self) {
        let hook = self.with(|s| {
            s.dropped = false;
            let h = s.hook.take();
            if h.is_some() {
                s.removes += 1;
            }
            h
        });
        if let Some(h) = hook {
            drop(h.tx);
            let _ = h.join.join();
        }
    }

    fn set_hotkeys(&self, bindings: Vec<Binding>) {
        self.with(|s| s.tables_set.push(bindings.clone()));
        self.table.store(Arc::new(bindings));
    }

    fn hook_health(&self) -> HookHealth {
        self.with(|s| {
            if s.dropped {
                HookHealth::Dropped
            } else if s.hook.is_some() {
                HookHealth::Healthy
            } else {
                HookHealth::NotInstalled
            }
        })
    }

    fn release_all(&self) {
        self.with(|s| s.releases += 1);
    }
}
