// Behaviour from desktoplab (github.com/v7ren/desktoplab), commit 6e5360e, MIT.
//! Cursor position and move, in points (the space macOS uses for the cursor and the screens).

use std::ptr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::ffi::{self, CFTypeRef, CGPoint};

/// Where this host last put the pointer, and when. Reading the position from a fresh event can lag behind a warp or
/// a posted move, so a move that follows one within `TRACKED_MAX_AGE` starts from where that one ended.
static TRACKED: Mutex<Option<((i32, i32), Instant)>> = Mutex::new(None);
const TRACKED_MAX_AGE: Duration = Duration::from_millis(150);

/// Remember that the pointer was just put at `(x, y)`.
pub fn note(x: i32, y: i32) {
    *TRACKED.lock().unwrap_or_else(|p| p.into_inner()) = Some(((x, y), Instant::now()));
}

/// The remembered position while it is fresh, otherwise what the system says.
fn pick(tracked: Option<((i32, i32), Duration)>, system: impl FnOnce() -> Result<(i32, i32), String>) -> Result<(i32, i32), String> {
    match tracked {
        Some((at, age)) if age <= TRACKED_MAX_AGE => Ok(at),
        _ => system(),
    }
}

pub fn position() -> Result<(i32, i32), String> {
    let tracked = TRACKED.lock().unwrap_or_else(|p| p.into_inner()).map(|(at, when)| (at, when.elapsed()));
    pick(tracked, system_position)
}

fn system_position() -> Result<(i32, i32), String> {
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
    note(x, y);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_remembered_position_wins_over_the_system_one() {
        let got = pick(Some(((5, 6), Duration::from_millis(10))), || Ok((1, 1)));
        assert_eq!(got, Ok((5, 6)));
    }

    #[test]
    fn a_stale_or_missing_position_falls_back_to_the_system() {
        assert_eq!(pick(Some(((5, 6), Duration::from_millis(500))), || Ok((1, 1))), Ok((1, 1)));
        assert_eq!(pick(None, || Ok((2, 2))), Ok((2, 2)));
    }
}
