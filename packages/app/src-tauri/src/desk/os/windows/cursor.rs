// Behaviour from desktoplab (github.com/v7ren/desktoplab), commit 6e5360e, MIT.
//! Cursor position and move. DevHop dropped the result of `SetCursorPos`; failures are reported here.

use windows::Win32::Foundation::POINT;
use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, SetCursorPos};

use super::dpi::DpiScope;

pub fn position() -> Result<(i32, i32), String> {
    let _dpi = DpiScope::new();
    let mut pt = POINT::default();
    // SAFETY: `pt` is a valid out pointer for the call.
    unsafe { GetCursorPos(&mut pt) }.map_err(|e| format!("GetCursorPos failed: {e}"))?;
    Ok((pt.x, pt.y))
}

pub fn warp(x: i32, y: i32) -> Result<(), String> {
    let _dpi = DpiScope::new();
    // SAFETY: plain call.
    unsafe { SetCursorPos(x, y) }.map_err(|e| format!("SetCursorPos failed: {e}"))
}
