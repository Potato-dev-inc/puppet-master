// Behaviour from desktoplab (github.com/v7ren/desktoplab), commit 6e5360e, MIT.
//! The real Windows side of the desk link: input injection, capture, clipboard. Never constructed in a test build
//! (`link::remote::default_remote`).
//!
//! - **Injection** is `SendInput` for keys, buttons and the wheel; pointer moves are `SetCursorPos` from the current
//!   position, so the other computer's acceleration is not applied on top of this one's. Every key and button
//!   pressed is remembered and released by `release_injected`.
//! - **UIPI**: Windows drops injected input aimed at a window that runs as admin. Nothing can be done about it from a
//!   normal process; the page says so ("Can't control windows that run as admin.").
//! - **Capture** (`capture.rs`) holds the pointer still in the middle of the screen and reports what the person does.

use std::collections::BTreeSet;
use std::mem::size_of;
use std::sync::Mutex;

use desk_proto::hid_to_vk;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP,
    MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_RIGHTDOWN,
    MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_WHEEL, MOUSEINPUT, VIRTUAL_KEY,
};

use super::{capture, clipboard, cursor, display};
use crate::desk::host::DisplayInfo;
use crate::desk::link::remote::{CaptureSink, ClipItem, Inject, RemoteHost};

#[derive(Default)]
struct Held {
    keys: BTreeSet<u16>,
    buttons: BTreeSet<u8>,
}

pub struct WindowsRemote {
    capture: Mutex<Option<capture::CaptureHook>>,
    held: Mutex<Held>,
}

impl WindowsRemote {
    /// No side effects: no hook, no key press, no clipboard access.
    pub fn new() -> Self {
        Self { capture: Mutex::new(None), held: Mutex::new(Held::default()) }
    }
}

impl Default for WindowsRemote {
    fn default() -> Self {
        Self::new()
    }
}

fn is_extended(vk: u16) -> bool {
    matches!(vk, 0x21..=0x28 | 0x2D | 0x2E | 0x5B | 0x5C | 0xA3 | 0xA5 | 0x6F)
}

fn send(inputs: &[INPUT]) -> Result<(), String> {
    // SAFETY: `inputs` is a valid slice of fully initialised INPUT structures.
    let sent = unsafe { SendInput(inputs, size_of::<INPUT>() as i32) };
    if sent as usize == inputs.len() {
        Ok(())
    } else {
        Err("Windows refused the input (a window that runs as admin?)".into())
    }
}

fn key_input(vk: u16, down: bool) -> INPUT {
    let mut flags = if down { Default::default() } else { KEYEVENTF_KEYUP };
    if is_extended(vk) {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VIRTUAL_KEY(vk), wScan: 0, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
    }
}

fn mouse_input(flags: windows::Win32::UI::Input::KeyboardAndMouse::MOUSE_EVENT_FLAGS, data: i32) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 { mi: MOUSEINPUT { dx: 0, dy: 0, mouseData: data as u32, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
    }
}

fn center_of(displays: &[DisplayInfo], pos: (i32, i32)) -> (i32, i32) {
    let d = displays
        .iter()
        .find(|d| pos.0 >= d.x && pos.0 < d.x + d.width as i32 && pos.1 >= d.y && pos.1 < d.y + d.height as i32)
        .or_else(|| displays.first());
    d.map_or(pos, |d| (d.x + d.width as i32 / 2, d.y + d.height as i32 / 2))
}

impl RemoteHost for WindowsRemote {
    fn displays(&self) -> Result<Vec<DisplayInfo>, String> {
        display::list()
    }

    fn cursor_pos(&self) -> Result<(i32, i32), String> {
        cursor::position()
    }

    fn move_cursor(&self, x: i32, y: i32) -> Result<(), String> {
        cursor::warp(x, y)
    }

    fn buttons_held(&self) -> bool {
        // VK_LBUTTON, VK_RBUTTON, VK_MBUTTON
        // SAFETY: plain calls.
        [0x01, 0x02, 0x04].iter().any(|vk| unsafe { GetAsyncKeyState(*vk) } < 0)
    }

    fn inject(&self, event: Inject) -> Result<(), String> {
        let mut held = self.held.lock().unwrap_or_else(|p| p.into_inner());
        match event {
            Inject::Key { hid, down } => {
                let vk = hid_to_vk(hid).ok_or("a key this computer does not have")?;
                send(&[key_input(vk, down)])?;
                if down {
                    held.keys.insert(hid);
                } else {
                    held.keys.remove(&hid);
                }
            }
            Inject::Button { button, down } => {
                let flag = match (button, down) {
                    (0, true) => MOUSEEVENTF_LEFTDOWN,
                    (0, false) => MOUSEEVENTF_LEFTUP,
                    (1, true) => MOUSEEVENTF_RIGHTDOWN,
                    (1, false) => MOUSEEVENTF_RIGHTUP,
                    (_, true) => MOUSEEVENTF_MIDDLEDOWN,
                    (_, false) => MOUSEEVENTF_MIDDLEUP,
                };
                send(&[mouse_input(flag, 0)])?;
                if down {
                    held.buttons.insert(button);
                } else {
                    held.buttons.remove(&button);
                }
            }
            Inject::Move { dx, dy } => {
                let (x, y) = cursor::position()?;
                cursor::warp(x.saturating_add(dx), y.saturating_add(dy))?;
            }
            Inject::Wheel { dx, dy } => {
                if dy != 0 {
                    send(&[mouse_input(MOUSEEVENTF_WHEEL, dy)])?;
                }
                if dx != 0 {
                    send(&[mouse_input(MOUSEEVENTF_HWHEEL, dx)])?;
                }
            }
        }
        Ok(())
    }

    fn release_injected(&self) {
        let (keys, buttons) = {
            let mut held = self.held.lock().unwrap_or_else(|p| p.into_inner());
            (std::mem::take(&mut held.keys), std::mem::take(&mut held.buttons))
        };
        for hid in keys {
            if let Some(vk) = hid_to_vk(hid) {
                let _ = send(&[key_input(vk, false)]);
            }
        }
        for button in buttons {
            let flag = match button {
                0 => MOUSEEVENTF_LEFTUP,
                1 => MOUSEEVENTF_RIGHTUP,
                _ => MOUSEEVENTF_MIDDLEUP,
            };
            let _ = send(&[mouse_input(flag, 0)]);
        }
    }

    fn start_capture(&self, sink: CaptureSink) -> Result<(), String> {
        let mut slot = self.capture.lock().unwrap_or_else(|p| p.into_inner());
        if slot.is_some() {
            return Ok(());
        }
        let displays = display::list()?;
        let park = center_of(&displays, cursor::position()?);
        cursor::warp(park.0, park.1)?;
        *slot = Some(capture::start(sink, park)?);
        Ok(())
    }

    fn stop_capture(&self) {
        let hook = self.capture.lock().unwrap_or_else(|p| p.into_inner()).take();
        if let Some(hook) = hook {
            // Keys that were down when the pointer left and were let go meanwhile: tell this computer.
            for vk in hook.stop() {
                let _ = send(&[key_input(vk, false)]);
            }
        }
    }

    fn clipboard_read(&self) -> Result<Vec<ClipItem>, String> {
        clipboard::read()
    }

    fn clipboard_write(&self, items: &[ClipItem]) -> Result<(), String> {
        clipboard::write(items)
    }
}
