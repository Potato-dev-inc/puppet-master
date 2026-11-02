// Behaviour from desktoplab (github.com/v7ren/desktoplab), commit 6e5360e, MIT.
//! The real macOS side of the desk link: input injection, capture, clipboard. Never constructed in a test build
//! (`link::remote::default_remote`).
//!
//! - **Injection** posts Quartz events (`CGEventPost`) for keys, buttons, the wheel and pointer moves, so apps see
//!   real hover and drag events. Every key and button pressed is remembered and released by `release_injected`.
//!   It needs Accessibility access; without it each call says what to switch on.
//! - **Capture** (`capture.rs`) swallows what the person does while the pointer is on another computer.
//! - **Clipboard** (`clipboard.rs`) is the general pasteboard: text and one PNG.
//!
//! Coordinates are points, as in the rest of this module.

use std::collections::BTreeSet;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use desk_proto::hid_to_mac;

use super::capture::{self, ACCESS_NEEDED, WHEEL_UNITS_PER_PIXEL};
use super::ffi::{self, CFTypeRef, CGEventRef, CGPoint};
use super::{clipboard, cursor, display, hook};
use crate::desk::host::DisplayInfo;
use crate::desk::link::remote::{CaptureSink, ClipItem, Inject, RemoteHost};

/// Two presses of one button this close in time and space are a double click.
const DOUBLE_CLICK: Duration = Duration::from_millis(500);
const DOUBLE_CLICK_SLOP: i32 = 4;

const FLAG_SHIFT: u64 = 0x0002_0000;
const FLAG_CONTROL: u64 = 0x0004_0000;
const FLAG_OPTION: u64 = 0x0008_0000;
const FLAG_COMMAND: u64 = 0x0010_0000;

#[derive(Default)]
struct Held {
    /// HID codes of keys this host pressed.
    keys: BTreeSet<u16>,
    buttons: BTreeSet<u8>,
    /// Button, when, where, and how many presses in a row.
    last_click: Option<(u8, Instant, (i32, i32), i64)>,
}

pub struct MacosRemote {
    capture: Mutex<Option<capture::CaptureHook>>,
    held: Mutex<Held>,
}

impl MacosRemote {
    /// No side effects: no tap, no key press, no clipboard access.
    pub fn new() -> Self {
        Self { capture: Mutex::new(None), held: Mutex::new(Held::default()) }
    }
}

impl Default for MacosRemote {
    fn default() -> Self {
        Self::new()
    }
}

fn require_access() -> Result<(), String> {
    // SAFETY: plain call.
    if unsafe { ffi::AXIsProcessTrusted() } {
        Ok(())
    } else {
        Err(ACCESS_NEEDED.into())
    }
}

/// Posts an event and releases it.
fn post(event: CGEventRef) -> Result<(), String> {
    if event.is_null() {
        return Err("macOS refused to make the input event".into());
    }
    // SAFETY: `event` is a valid event we own; it is released after posting.
    unsafe {
        ffi::CGEventPost(ffi::CG_HID_EVENT_TAP, event);
        ffi::CFRelease(event as CFTypeRef);
    }
    Ok(())
}

/// The modifier bits for the keys held (left and right keys share a bit, so both must go up to clear it).
fn flags_for(keys: &BTreeSet<u16>) -> u64 {
    keys.iter().filter_map(|h| hid_to_mac(*h)).fold(0, |acc, code| {
        acc | match code {
            0x37 | 0x36 => FLAG_COMMAND,
            0x38 | 0x3C => FLAG_SHIFT,
            0x3B | 0x3E => FLAG_CONTROL,
            0x3A | 0x3D => FLAG_OPTION,
            _ => 0,
        }
    })
}

fn key_event(code: u16, down: bool, flags: u64) -> Result<(), String> {
    // SAFETY: a null source is the default source.
    let event = unsafe { ffi::CGEventCreateKeyboardEvent(std::ptr::null(), code, down) };
    if event.is_null() {
        return Err("macOS refused to make the key event".into());
    }
    // SAFETY: valid event. A modifier key is announced as a flags change, which is what apps listen for.
    unsafe {
        ffi::CGEventSetFlags(event, flags);
        if hook::is_modifier(code) {
            ffi::CGEventSetType(event, ffi::CG_EVENT_FLAGS_CHANGED);
        }
    }
    post(event)
}

/// (down, up, dragged, CGMouseButton) for one of our button numbers (0 left, 1 right, anything else middle).
fn button_types(button: u8) -> (u32, u32, u32, u32) {
    match button {
        0 => (ffi::CG_EVENT_LEFT_MOUSE_DOWN, ffi::CG_EVENT_LEFT_MOUSE_UP, ffi::CG_EVENT_LEFT_MOUSE_DRAGGED, 0),
        1 => (ffi::CG_EVENT_RIGHT_MOUSE_DOWN, ffi::CG_EVENT_RIGHT_MOUSE_UP, ffi::CG_EVENT_RIGHT_MOUSE_DRAGGED, 1),
        _ => (ffi::CG_EVENT_OTHER_MOUSE_DOWN, ffi::CG_EVENT_OTHER_MOUSE_UP, ffi::CG_EVENT_OTHER_MOUSE_DRAGGED, 2),
    }
}

fn post_mouse(
    kind: u32,
    at: (i32, i32),
    button: u32,
    click_state: Option<i64>,
    delta: Option<(i32, i32)>,
) -> Result<(), String> {
    let point = CGPoint { x: f64::from(at.0), y: f64::from(at.1) };
    // SAFETY: a null source is the default source.
    let event = unsafe { ffi::CGEventCreateMouseEvent(std::ptr::null(), kind, point, button) };
    if event.is_null() {
        return Err("macOS refused to make the mouse event".into());
    }
    // SAFETY: valid event.
    unsafe {
        if let Some(n) = click_state {
            ffi::CGEventSetIntegerValueField(event, ffi::CG_MOUSE_EVENT_CLICK_STATE, n);
        }
        if let Some((dx, dy)) = delta {
            ffi::CGEventSetIntegerValueField(event, ffi::CG_MOUSE_EVENT_DELTA_X, i64::from(dx));
            ffi::CGEventSetIntegerValueField(event, ffi::CG_MOUSE_EVENT_DELTA_Y, i64::from(dy));
        }
    }
    post(event)
}

/// The nearest point on any screen, so a move cannot carry the pointer into the gap between screens.
fn clamp_to_screens(displays: &[DisplayInfo], p: (i32, i32)) -> (i32, i32) {
    displays
        .iter()
        .map(|d| (p.0.clamp(d.x, d.x + d.width as i32 - 1), p.1.clamp(d.y, d.y + d.height as i32 - 1)))
        .min_by_key(|c| {
            let (dx, dy) = (i64::from(c.0 - p.0), i64::from(c.1 - p.1));
            dx * dx + dy * dy
        })
        .unwrap_or(p)
}

impl RemoteHost for MacosRemote {
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
        // SAFETY: plain calls. Buttons 0, 1, 2 are left, right, middle.
        (0..3).any(|b| unsafe { ffi::CGEventSourceButtonState(ffi::CG_EVENT_SOURCE_STATE_COMBINED_SESSION, b) })
    }

    fn inject(&self, event: Inject) -> Result<(), String> {
        require_access()?;
        let mut held = self.held.lock().unwrap_or_else(|p| p.into_inner());
        match event {
            Inject::Key { hid, down } => {
                let code = hid_to_mac(hid).ok_or("a key this computer does not have")?;
                let mut keys = held.keys.clone();
                if down {
                    keys.insert(hid);
                } else {
                    keys.remove(&hid);
                }
                key_event(code, down, flags_for(&keys))?;
                held.keys = keys;
            }
            Inject::Button { button, down } => {
                let at = cursor::position()?;
                let (down_type, up_type, _, number) = button_types(button);
                let click = if down {
                    let n = match held.last_click {
                        Some((b, when, pos, n))
                            if b == button
                                && when.elapsed() <= DOUBLE_CLICK
                                && (pos.0 - at.0).abs() <= DOUBLE_CLICK_SLOP
                                && (pos.1 - at.1).abs() <= DOUBLE_CLICK_SLOP =>
                        {
                            n + 1
                        }
                        _ => 1,
                    };
                    held.last_click = Some((button, Instant::now(), at, n));
                    n
                } else {
                    held.last_click.filter(|(b, ..)| *b == button).map_or(1, |(.., n)| n)
                };
                post_mouse(if down { down_type } else { up_type }, at, number, Some(click), None)?;
                if down {
                    held.buttons.insert(button);
                } else {
                    held.buttons.remove(&button);
                }
            }
            Inject::Move { dx, dy } => {
                let (x, y) = cursor::position()?;
                let target = clamp_to_screens(&display::list()?, (x.saturating_add(dx), y.saturating_add(dy)));
                // A move with a button down is a drag; apps ignore a plain move then.
                let (kind, number) = match held.buttons.iter().next() {
                    Some(b) => {
                        let (_, _, dragged, number) = button_types(*b);
                        (dragged, number)
                    }
                    None => (ffi::CG_EVENT_MOUSE_MOVED, 0),
                };
                post_mouse(kind, target, number, None, Some((target.0 - x, target.1 - y)))?;
            }
            Inject::Wheel { dx, dy } => {
                if dx == 0 && dy == 0 {
                    return Ok(());
                }
                // Positive vertical is up on both systems. Positive horizontal is right on Windows and left here.
                let (v, h) = (dy / WHEEL_UNITS_PER_PIXEL, -(dx / WHEEL_UNITS_PER_PIXEL));
                // SAFETY: a null source is the default source.
                let e = unsafe {
                    ffi::CGEventCreateScrollWheelEvent2(std::ptr::null(), ffi::CG_SCROLL_EVENT_UNIT_PIXEL, 2, v, h, 0)
                };
                post(e)?;
            }
        }
        Ok(())
    }

    fn release_injected(&self) {
        let (keys, buttons) = {
            let mut held = self.held.lock().unwrap_or_else(|p| p.into_inner());
            held.last_click = None;
            (std::mem::take(&mut held.keys), std::mem::take(&mut held.buttons))
        };
        // Each key goes up while the others are still counted, so the modifier flags stay right on the way down.
        let mut remaining = keys.clone();
        for hid in keys {
            remaining.remove(&hid);
            if let Some(code) = hid_to_mac(hid) {
                let _ = key_event(code, false, flags_for(&remaining));
            }
        }
        if let Ok(at) = cursor::position() {
            for button in buttons {
                let (_, up, _, number) = button_types(button);
                let _ = post_mouse(up, at, number, Some(1), None);
            }
        }
    }

    fn start_capture(&self, sink: CaptureSink) -> Result<(), String> {
        let mut slot = self.capture.lock().unwrap_or_else(|p| p.into_inner());
        if slot.is_some() {
            return Ok(());
        }
        *slot = Some(capture::start(sink)?);
        Ok(())
    }

    fn stop_capture(&self) {
        let hook = self.capture.lock().unwrap_or_else(|p| p.into_inner()).take();
        if let Some(hook) = hook {
            // Keys that were down when the pointer left and were let go meanwhile: tell this computer.
            for code in hook.stop() {
                let _ = key_event(code, false, 0);
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
