// Behaviour from desktoplab (github.com/v7ren/desktoplab), commit 6e5360e, MIT.
//! Screen list in points, origins can be negative (a screen left of or above the main one). `scale` is the
//! backing scale (2 on a Retina screen), so a UI can still show pixels.

use super::ffi::{self, CGDirectDisplayID};
use crate::desk::host::DisplayInfo;

const MAX_DISPLAYS: usize = 32;

pub fn list() -> Result<Vec<DisplayInfo>, String> {
    let mut ids: [CGDirectDisplayID; MAX_DISPLAYS] = [0; MAX_DISPLAYS];
    let mut count: u32 = 0;
    // SAFETY: `ids` has room for MAX_DISPLAYS entries and `count` is a valid out pointer.
    let err = unsafe { ffi::CGGetActiveDisplayList(MAX_DISPLAYS as u32, ids.as_mut_ptr(), &mut count) };
    if err != ffi::CG_SUCCESS {
        return Err(format!("CGGetActiveDisplayList failed ({err})"));
    }
    // SAFETY: plain call.
    let main = unsafe { ffi::CGMainDisplayID() };
    Ok(ids[..count as usize].iter().map(|&id| info(id, id == main)).collect())
}

fn info(id: CGDirectDisplayID, primary: bool) -> DisplayInfo {
    // SAFETY: plain calls. The display mode is a copy, so it is released here.
    let (bounds, pixel_width) = unsafe {
        let bounds = ffi::CGDisplayBounds(id);
        let mode = ffi::CGDisplayCopyDisplayMode(id);
        let pixel_width = if mode.is_null() {
            0
        } else {
            ffi::CGDisplayModeGetPixelWidth(mode)
        };
        if !mode.is_null() {
            ffi::CGDisplayModeRelease(mode);
        }
        (bounds, pixel_width)
    };
    let scale = if bounds.size.width > 0.0 && pixel_width > 0 {
        pixel_width as f64 / bounds.size.width
    } else {
        1.0
    };
    DisplayInfo {
        id: id.to_string(),
        name: if primary {
            "Main display".to_string()
        } else {
            format!("Display {id}")
        },
        x: bounds.origin.x.round() as i32,
        y: bounds.origin.y.round() as i32,
        width: bounds.size.width.round().max(0.0) as u32,
        height: bounds.size.height.round().max(0.0) as u32,
        scale,
        primary,
    }
}
