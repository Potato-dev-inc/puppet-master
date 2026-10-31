//! The Point box window, next to the picked element (PF-10).
//!
//! A small local window (the Ask bar pattern): created hidden on first use, always on top, no taskbar
//! entry. It loads Puppet Master's own page (`index.html?pointbox=1`), so the person types here and never
//! in the previewed page. It is the only Point window with Tauri access (listed in the capabilities).

use super::session::PointState;
use tauri::{AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

pub const LABEL: &str = "pointbox";
pub const URL: &str = "index.html?pointbox=1";
/// Logical size when created. Tall enough for the full form with three picks (head, picks, To, text, hint, buttons);
/// the box page itself never resizes the window, so the pick list scrolls and the footer stays pinned if it is short.
pub const WIDTH: f64 = 420.0;
pub const HEIGHT: f64 = 440.0;
/// Space between the picked element and the box, in physical pixels.
const GAP: i32 = 12;

/// A rectangle in physical pixels: x, y, width, height.
pub type Rect4 = (i32, i32, u32, u32);

/// Where the box goes, as its top-left in physical pixels.
///
/// Right of the element when it fits on the monitor, else left of it, else inside the element's
/// bottom-right corner (an element that fills the preview). The top lines up with the element's top. The
/// box always stays on the monitor.
pub fn box_position(monitor: Rect4, element: Rect4, size: (u32, u32)) -> (i32, i32) {
    let (mx, my, mw, mh) = monitor;
    let (ex, ey, ew, eh) = element;
    let (bw, bh) = (size.0 as i32, size.1 as i32);
    let (mright, mbottom) = (mx + mw as i32, my + mh as i32);
    let right = ex + ew as i32 + GAP;
    let left = ex - GAP - bw;
    let (x, y) = if right + bw <= mright {
        (right, ey)
    } else if left >= mx {
        (left, ey)
    } else {
        (ex + ew as i32 - bw - GAP, ey + eh as i32 - bh - GAP)
    };
    (clamp(x, mx, mright - bw), clamp(y, my, mbottom - bh))
}

/// `value` inside `lo..=hi`. When the range is empty (the box is bigger than the monitor) `lo` wins.
fn clamp(value: i32, lo: i32, hi: i32) -> i32 {
    value.min(hi).max(lo)
}

/// The box's window, created hidden on first use.
pub fn ensure(app: &AppHandle) -> Result<WebviewWindow, String> {
    if let Some(window) = app.get_webview_window(LABEL) {
        return Ok(window);
    }
    WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App(URL.into()))
        .title("Point and Fix")
        .inner_size(WIDTH, HEIGHT)
        .decorations(false)
        .always_on_top(true)
        .resizable(false)
        .skip_taskbar(true)
        .shadow(true)
        .visible(false)
        .focused(false)
        .build()
        .map_err(|e| e.to_string())
}

/// Shows the box next to `screen_rect` (the element on screen, physical pixels) and focuses it.
pub fn show_near(app: &AppHandle, screen_rect: Rect4) {
    let Ok(window) = ensure(app) else { return };
    let center = (screen_rect.0 + (screen_rect.2 / 2) as i32, screen_rect.1 + (screen_rect.3 / 2) as i32);
    let monitor = app
        .monitor_from_point(f64::from(center.0), f64::from(center.1))
        .ok()
        .flatten()
        .or_else(|| window.current_monitor().ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten());
    if let Some(monitor) = monitor {
        let scale = monitor.scale_factor();
        let size = window
            .outer_size()
            .map(|s| (s.width, s.height))
            .unwrap_or(((WIDTH * scale) as u32, (HEIGHT * scale) as u32));
        let (x, y) = box_position(
            (monitor.position().x, monitor.position().y, monitor.size().width, monitor.size().height),
            screen_rect,
            size,
        );
        let _ = window.set_position(PhysicalPosition::new(x, y));
    }
    let _ = window.show();
    let _ = window.set_focus();
}

/// Hides the box. Its session stays until it ends or is discarded.
pub fn hide(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.hide();
    }
}

pub fn is_visible(app: &AppHandle) -> bool {
    app.get_webview_window(LABEL).and_then(|w| w.is_visible().ok()).unwrap_or(false)
}

/// Sends the state to the box (when its window exists).
pub fn emit_state(app: &AppHandle, state: &PointState) {
    if app.get_webview_window(LABEL).is_some() {
        let _ = app.emit_to(LABEL, "point://state", state);
    }
}

/// Back to the default size, for a fresh session.
pub fn reset_size(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.set_size(LogicalSize::new(WIDTH, HEIGHT));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MONITOR: Rect4 = (0, 0, 1920, 1080);
    const BOX: (u32, u32) = (420, 300);

    #[test]
    fn box_goes_right_of_the_element_with_a_gap() {
        let (x, y) = box_position(MONITOR, (600, 400, 180, 44), BOX);
        assert_eq!((x, y), (600 + 180 + GAP, 400));
    }

    #[test]
    fn box_goes_left_when_the_right_side_is_full() {
        let (x, y) = box_position(MONITOR, (1600, 400, 200, 44), BOX);
        assert_eq!((x, y), (1600 - GAP - 420, 400));
    }

    #[test]
    fn box_stays_on_the_monitor_vertically() {
        let (_, y) = box_position(MONITOR, (100, 1000, 50, 40), BOX);
        assert_eq!(y, 1080 - 300);
        let (_, y) = box_position(MONITOR, (100, -50, 50, 40), BOX);
        assert_eq!(y, 0);
    }

    #[test]
    fn box_sits_inside_a_huge_element() {
        let (x, y) = box_position(MONITOR, (0, 0, 1920, 1080), BOX);
        assert_eq!((x, y), (1920 - 420 - GAP, 1080 - 300 - GAP));
    }

    #[test]
    fn box_works_on_a_monitor_left_of_the_primary_one() {
        let monitor: Rect4 = (-1920, 100, 1920, 1080);
        let (x, y) = box_position(monitor, (-1500, 300, 100, 40), BOX);
        assert_eq!((x, y), (-1500 + 100 + GAP, 300));
        let (x, _) = box_position(monitor, (-150, 300, 100, 40), BOX);
        assert_eq!(x, -150 - GAP - 420);
    }

    #[test]
    fn box_bigger_than_the_monitor_is_pinned_to_its_corner() {
        let (x, y) = box_position((10, 20, 300, 200), (50, 50, 20, 20), BOX);
        assert_eq!((x, y), (10, 20));
    }

    #[test]
    fn the_box_is_tall_enough_for_three_picks() {
        // head + three picks (150 at most) + To + text + hint + buttons + padding come to about 420 logical pixels.
        // At 300 the Send button fell below the window's bottom edge with two picks.
        assert!(HEIGHT >= 420.0, "the Point box is {HEIGHT} high");
    }

    #[test]
    fn the_box_is_not_a_preview_window() {
        assert!(!LABEL.starts_with(super::super::window::PREVIEW_PREFIX));
    }
}
