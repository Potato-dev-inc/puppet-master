//! The Quartz, CoreFoundation and Accessibility calls the My computers host makes on macOS. They are declared by
//! hand so the build needs no new crate. Every symbol comes from the ApplicationServices or CoreFoundation
//! framework, which the app already links through Tauri.
//!
//! Coordinates are points (the space macOS uses for screens and the cursor). A Retina screen has two pixels per
//! point; `CGDisplayModeGetPixelWidth` gives that ratio.

use std::ffi::c_void;

pub type CGEventRef = *mut c_void;
pub type CGEventSourceRef = *const c_void;
pub type CGEventTapProxy = *mut c_void;
pub type CGDisplayModeRef = *mut c_void;
pub type CGDirectDisplayID = u32;
pub type CGError = i32;
pub type CFMachPortRef = *mut c_void;
pub type CFRunLoopSourceRef = *mut c_void;
pub type CFStringRef = *const c_void;
pub type CFTypeRef = *const c_void;
pub type CFIndex = isize;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct CGPoint {
    pub x: f64,
    pub y: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct CGSize {
    pub width: f64,
    pub height: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct CGRect {
    pub origin: CGPoint,
    pub size: CGSize,
}

/// `CGEventTapCallBack`. Returning the event passes it on; returning null drops it.
pub type CGEventTapCallBack = unsafe extern "C" fn(CGEventTapProxy, u32, CGEventRef, *mut c_void) -> CGEventRef;

pub const CG_SESSION_EVENT_TAP: u32 = 1;
pub const CG_HEAD_INSERT_EVENT_TAP: u32 = 0;
/// An active tap: it may drop events (the default, as opposed to a listen-only tap).
pub const CG_EVENT_TAP_OPTION_DEFAULT: u32 = 0;
/// A listen-only tap: it sees events but can never drop or change one.
pub const CG_EVENT_TAP_OPTION_LISTEN_ONLY: u32 = 1;
pub const CG_EVENT_KEY_DOWN: u32 = 10;
pub const CG_EVENT_KEY_UP: u32 = 11;
pub const CG_EVENT_FLAGS_CHANGED: u32 = 12;
pub const CG_EVENT_TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
pub const CG_EVENT_TAP_DISABLED_BY_USER_INPUT: u32 = 0xFFFF_FFFF;
pub const CG_KEYBOARD_EVENT_KEYCODE: u32 = 9;
pub const CG_EVENT_SOURCE_STATE_COMBINED_SESSION: i32 = 0;
pub const CG_EVENT_SOURCE_STATE_HID_SYSTEM: i32 = 1;
pub const CG_SUCCESS: CGError = 0;
pub const CG_EVENT_LEFT_MOUSE_DOWN: u32 = 1;
pub const CG_EVENT_LEFT_MOUSE_UP: u32 = 2;
pub const CG_EVENT_RIGHT_MOUSE_DOWN: u32 = 3;
pub const CG_EVENT_RIGHT_MOUSE_UP: u32 = 4;
pub const CG_EVENT_MOUSE_MOVED: u32 = 5;
pub const CG_EVENT_LEFT_MOUSE_DRAGGED: u32 = 6;
pub const CG_EVENT_RIGHT_MOUSE_DRAGGED: u32 = 7;
pub const CG_EVENT_SCROLL_WHEEL: u32 = 22;
pub const CG_EVENT_OTHER_MOUSE_DOWN: u32 = 25;
pub const CG_EVENT_OTHER_MOUSE_UP: u32 = 26;
pub const CG_EVENT_OTHER_MOUSE_DRAGGED: u32 = 27;
pub const CG_MOUSE_EVENT_CLICK_STATE: u32 = 1;
pub const CG_MOUSE_EVENT_BUTTON_NUMBER: u32 = 3;
pub const CG_MOUSE_EVENT_DELTA_X: u32 = 4;
pub const CG_MOUSE_EVENT_DELTA_Y: u32 = 5;
pub const CG_SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_1: u32 = 96;
pub const CG_SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_2: u32 = 97;
/// The process that posted the event (ours, for everything `RemoteHost::inject` sends).
pub const CG_EVENT_SOURCE_UNIX_PROCESS_ID: u32 = 41;
pub const CG_HID_EVENT_TAP: u32 = 0;
pub const CG_SCROLL_EVENT_UNIT_PIXEL: u32 = 0;
/// Mouse buttons, key flags and the scroll wheel, the events a capture tap swallows besides keys.
pub const POINTER_EVENTS_MASK: u64 = (1 << CG_EVENT_LEFT_MOUSE_DOWN)
    | (1 << CG_EVENT_LEFT_MOUSE_UP)
    | (1 << CG_EVENT_RIGHT_MOUSE_DOWN)
    | (1 << CG_EVENT_RIGHT_MOUSE_UP)
    | (1 << CG_EVENT_MOUSE_MOVED)
    | (1 << CG_EVENT_LEFT_MOUSE_DRAGGED)
    | (1 << CG_EVENT_RIGHT_MOUSE_DRAGGED)
    | (1 << CG_EVENT_SCROLL_WHEEL)
    | (1 << CG_EVENT_OTHER_MOUSE_DOWN)
    | (1 << CG_EVENT_OTHER_MOUSE_UP)
    | (1 << CG_EVENT_OTHER_MOUSE_DRAGGED);
/// Key down, key up and flags changed (the modifier keys).
pub const KEY_EVENTS_MASK: u64 = (1 << CG_EVENT_KEY_DOWN) | (1 << CG_EVENT_KEY_UP) | (1 << CG_EVENT_FLAGS_CHANGED);
/// Plain move and the three drag types: every change of the local pointer position (`pointer_watch.rs`).
pub const MOVE_EVENTS_MASK: u64 = (1 << CG_EVENT_MOUSE_MOVED)
    | (1 << CG_EVENT_LEFT_MOUSE_DRAGGED)
    | (1 << CG_EVENT_RIGHT_MOUSE_DRAGGED)
    | (1 << CG_EVENT_OTHER_MOUSE_DRAGGED);

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    pub fn AXIsProcessTrusted() -> bool;
    pub fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        events_of_interest: u64,
        callback: CGEventTapCallBack,
        user_info: *mut c_void,
    ) -> CFMachPortRef;
    pub fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
    pub fn CGEventGetIntegerValueField(event: CGEventRef, field: u32) -> i64;
    pub fn CGEventGetFlags(event: CGEventRef) -> u64;
    /// Owned: the caller releases it (`CFRelease`) or keeps it for the life of the process.
    pub fn CGEventSourceCreate(state_id: i32) -> CGEventSourceRef;
    pub fn CGEventSourceFlagsState(state_id: i32) -> u64;
    pub fn CGEventSourceKeyState(state_id: i32, key: u16) -> bool;
    pub fn CGEventCreateMouseEvent(source: CGEventSourceRef, kind: u32, at: CGPoint, button: u32) -> CGEventRef;
    pub fn CGEventCreateKeyboardEvent(source: CGEventSourceRef, keycode: u16, down: bool) -> CGEventRef;
    pub fn CGEventCreateScrollWheelEvent2(
        source: CGEventSourceRef,
        units: u32,
        wheel_count: u32,
        wheel1: i32,
        wheel2: i32,
        wheel3: i32,
    ) -> CGEventRef;
    pub fn CGEventPost(tap: u32, event: CGEventRef);
    pub fn CGEventSetFlags(event: CGEventRef, flags: u64);
    pub fn CGEventSetType(event: CGEventRef, kind: u32);
    pub fn CGEventSetIntegerValueField(event: CGEventRef, field: u32, value: i64);
    pub fn CGEventSourceButtonState(state_id: i32, button: u32) -> bool;
    pub fn CGEventCreate(source: CGEventSourceRef) -> CGEventRef;
    pub fn CGEventGetLocation(event: CGEventRef) -> CGPoint;
    pub fn CGWarpMouseCursorPosition(point: CGPoint) -> CGError;
    pub fn CGGetActiveDisplayList(max: u32, displays: *mut CGDirectDisplayID, count: *mut u32) -> CGError;
    pub fn CGMainDisplayID() -> CGDirectDisplayID;
    pub fn CGDisplayBounds(display: CGDirectDisplayID) -> CGRect;
    pub fn CGDisplayCopyDisplayMode(display: CGDirectDisplayID) -> CGDisplayModeRef;
    pub fn CGDisplayModeGetPixelWidth(mode: CGDisplayModeRef) -> usize;
    pub fn CGDisplayModeRelease(mode: CGDisplayModeRef);
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    pub static kCFRunLoopCommonModes: CFStringRef;
    pub static kCFRunLoopDefaultMode: CFStringRef;
    pub fn CFMachPortCreateRunLoopSource(
        allocator: *const c_void,
        port: CFMachPortRef,
        order: CFIndex,
    ) -> CFRunLoopSourceRef;
    pub fn CFMachPortInvalidate(port: CFMachPortRef);
    pub fn CFRunLoopGetCurrent() -> *mut c_void;
    pub fn CFRunLoopAddSource(run_loop: *mut c_void, source: CFRunLoopSourceRef, mode: CFStringRef);
    /// Runs the loop for at most `seconds`, then returns.
    pub fn CFRunLoopRunInMode(mode: CFStringRef, seconds: f64, return_after_source_handled: u8) -> i32;
    pub fn CFRelease(cf: CFTypeRef);
}

extern "C" {
    pub fn getpid() -> i32;
}

// The Objective-C runtime, for the one place that needs it: the pasteboard (`clipboard.rs`).
#[link(name = "objc")]
extern "C" {
    pub fn objc_getClass(name: *const std::os::raw::c_char) -> *mut c_void;
    pub fn sel_registerName(name: *const std::os::raw::c_char) -> *mut c_void;
    /// Declared without arguments on purpose: each call site casts it to the exact function type it needs.
    pub fn objc_msgSend();
}

#[link(name = "Foundation", kind = "framework")]
#[link(name = "AppKit", kind = "framework")]
extern "C" {}
