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
pub const CG_EVENT_KEY_DOWN: u32 = 10;
pub const CG_EVENT_KEY_UP: u32 = 11;
pub const CG_EVENT_FLAGS_CHANGED: u32 = 12;
pub const CG_EVENT_TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
pub const CG_EVENT_TAP_DISABLED_BY_USER_INPUT: u32 = 0xFFFF_FFFF;
pub const CG_KEYBOARD_EVENT_KEYCODE: u32 = 9;
pub const CG_EVENT_SOURCE_STATE_COMBINED_SESSION: i32 = 0;
pub const CG_SUCCESS: CGError = 0;
/// Key down, key up and flags changed (the modifier keys).
pub const KEY_EVENTS_MASK: u64 = (1 << CG_EVENT_KEY_DOWN) | (1 << CG_EVENT_KEY_UP) | (1 << CG_EVENT_FLAGS_CHANGED);

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
    pub fn CGEventSourceFlagsState(state_id: i32) -> u64;
    pub fn CGEventSourceKeyState(state_id: i32, key: u16) -> bool;
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
