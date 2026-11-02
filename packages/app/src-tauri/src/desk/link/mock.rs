//! A pretend PC for the link tests. A cursor that is only a number pair, a list of what was "injected", a clipboard
//! that is a vector. Nothing here touches the operating system, a hook, the real clipboard or the network.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use tokio::time::Instant;

use super::super::host::DisplayInfo;
use super::geometry::union;
use super::remote::{Capture, CaptureSink, ClipItem, Inject, RemoteHost};

#[derive(Default)]
pub struct MockState {
    pub displays: Vec<DisplayInfo>,
    pub cursor: (i32, i32),
    pub moves: Vec<(i32, i32)>,
    pub log: Vec<Inject>,
    pub held_keys: BTreeSet<u16>,
    pub held_buttons: BTreeSet<u8>,
    /// When each key-up reached this computer (simulated time).
    pub key_up_at: Vec<(u16, Instant)>,
    pub capture: Option<CaptureSink>,
    pub captures_started: u32,
    pub captures_stopped: u32,
    pub clipboard: Vec<ClipItem>,
    pub clip_reads: u32,
    pub clip_writes: Vec<Vec<ClipItem>>,
    pub buttons_down: bool,
    pub fail_capture: bool,
    pub releases: u32,
    pub unsupported: bool,
    /// What `can_be_controlled` says (a Mac without Accessibility). `None`: it can be.
    pub refuse_control: Option<String>,
    /// What `inject` says while set (input that stopped working mid-way).
    pub fail_inject: Option<String>,
}

#[derive(Clone)]
pub struct MockRemote {
    pub st: Arc<Mutex<MockState>>,
}

pub fn display(id: &str, x: i32, y: i32, w: u32, h: u32) -> DisplayInfo {
    DisplayInfo { id: id.into(), name: id.into(), x, y, width: w, height: h, scale: 1.0, primary: x == 0 && y == 0 }
}

impl MockRemote {
    pub fn new(displays: Vec<DisplayInfo>, cursor: (i32, i32)) -> Self {
        Self { st: Arc::new(Mutex::new(MockState { displays, cursor, ..Default::default() })) }
    }

    /// A 1920x1080 computer with the pointer in the middle.
    pub fn standard() -> Self {
        Self::new(vec![display("main", 0, 0, 1920, 1080)], (960, 540))
    }

    pub fn with<R>(&self, f: impl FnOnce(&mut MockState) -> R) -> R {
        f(&mut self.st.lock().unwrap())
    }

    pub fn cursor(&self) -> (i32, i32) {
        self.with(|s| s.cursor)
    }

    /// The person moves the mouse to a place (not an injection).
    pub fn put_cursor(&self, x: i32, y: i32) {
        self.with(|s| s.cursor = (x, y));
    }

    pub fn capturing(&self) -> bool {
        self.with(|s| s.capture.is_some())
    }

    pub fn held_keys(&self) -> BTreeSet<u16> {
        self.with(|s| s.held_keys.clone())
    }

    /// The person does something while the pointer is away; delivered to the capture sink if one is installed.
    pub fn person(&self, c: Capture) -> bool {
        let sink = self.with(|s| s.capture.clone());
        match sink {
            Some(sink) => {
                sink(c);
                true
            }
            None => false,
        }
    }

    pub fn set_clipboard(&self, items: Vec<ClipItem>) {
        self.with(|s| s.clipboard = items);
    }

    pub fn clipboard(&self) -> Vec<ClipItem> {
        self.with(|s| s.clipboard.clone())
    }
}

impl RemoteHost for MockRemote {
    fn supported(&self) -> bool {
        !self.with(|s| s.unsupported)
    }

    fn displays(&self) -> Result<Vec<DisplayInfo>, String> {
        Ok(self.with(|s| s.displays.clone()))
    }

    fn cursor_pos(&self) -> Result<(i32, i32), String> {
        Ok(self.cursor())
    }

    fn move_cursor(&self, x: i32, y: i32) -> Result<(), String> {
        self.with(|s| {
            s.cursor = (x, y);
            s.moves.push((x, y));
        });
        Ok(())
    }

    fn buttons_held(&self) -> bool {
        self.with(|s| s.buttons_down)
    }

    fn can_be_controlled(&self) -> Result<(), String> {
        self.with(|s| s.refuse_control.clone()).map_or(Ok(()), Err)
    }

    fn inject(&self, event: Inject) -> Result<(), String> {
        if let Some(e) = self.with(|s| s.fail_inject.clone()) {
            return Err(e);
        }
        self.with(|s| {
            s.log.push(event);
            match event {
                Inject::Key { hid, down } => {
                    if down {
                        s.held_keys.insert(hid);
                    } else {
                        s.held_keys.remove(&hid);
                        s.key_up_at.push((hid, Instant::now()));
                    }
                }
                Inject::Button { button, down } => {
                    if down {
                        s.held_buttons.insert(button);
                    } else {
                        s.held_buttons.remove(&button);
                    }
                }
                Inject::Move { dx, dy } => {
                    // Like Windows: the pointer stays on the screens.
                    let u = union(&s.displays);
                    let (mut x, mut y) = (s.cursor.0 + dx, s.cursor.1 + dy);
                    if let Some(u) = u {
                        x = x.clamp(u.left, u.right - 1);
                        y = y.clamp(u.top, u.bottom - 1);
                    }
                    s.cursor = (x, y);
                }
                Inject::Wheel { .. } => {}
            }
        });
        Ok(())
    }

    fn release_injected(&self) {
        self.with(|s| {
            s.releases += 1;
            let keys: Vec<u16> = s.held_keys.iter().copied().collect();
            for hid in keys {
                s.held_keys.remove(&hid);
                s.key_up_at.push((hid, Instant::now()));
                s.log.push(Inject::Key { hid, down: false });
            }
            let buttons: Vec<u8> = s.held_buttons.iter().copied().collect();
            for button in buttons {
                s.held_buttons.remove(&button);
                s.log.push(Inject::Button { button, down: false });
            }
        });
    }

    fn start_capture(&self, sink: CaptureSink, _exit: (i32, i32)) -> Result<(), String> {
        self.with(|s| {
            if s.fail_capture {
                return Err("capture refused".to_string());
            }
            s.captures_started += 1;
            s.capture = Some(sink);
            Ok(())
        })
    }

    fn stop_capture(&self) {
        self.with(|s| {
            if s.capture.take().is_some() {
                s.captures_stopped += 1;
            }
        });
    }

    fn clipboard_read(&self) -> Result<Vec<ClipItem>, String> {
        Ok(self.with(|s| {
            s.clip_reads += 1;
            s.clipboard.clone()
        }))
    }

    fn clipboard_write(&self, items: &[ClipItem]) -> Result<(), String> {
        self.with(|s| {
            s.clipboard = items.to_vec();
            s.clip_writes.push(items.to_vec());
        });
        Ok(())
    }
}
