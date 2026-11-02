// Behaviour from desktoplab (github.com/v7ren/desktoplab), commit 6e5360e, MIT.
//! Cursor position and move, in points (the space macOS uses for the cursor and the screens).

use std::ptr;

use super::ffi::{self, CFTypeRef, CGPoint};

pub fn position() -> Result<(i32, i32), String> {
    // SAFETY: a null source asks for a new event that carries the current cursor location. It is released below.
    let event = unsafe { ffi::CGEventCreate(ptr::null()) };
    if event.is_null() {
        return Err("could not read the cursor position".into());
    }
    // SAFETY: `event` is a valid event from the call above.
    let at = unsafe { ffi::CGEventGetLocation(event) };
    // SAFETY: releases the event we own.
    unsafe { ffi::CFRelease(event as CFTypeRef) };
    Ok((at.x.round() as i32, at.y.round() as i32))
}

pub fn warp(x: i32, y: i32) -> Result<(), String> {
    let point = CGPoint {
        x: f64::from(x),
        y: f64::from(y),
    };
    // SAFETY: plain call.
    let err = unsafe { ffi::CGWarpMouseCursorPosition(point) };
    if err == ffi::CG_SUCCESS {
        Ok(())
    } else {
        Err(format!("CGWarpMouseCursorPosition failed ({err})"))
    }
}
