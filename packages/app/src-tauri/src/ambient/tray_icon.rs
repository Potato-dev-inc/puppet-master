//! Runtime half of the ambient tray icon: a small thread that turns pane statuses into the 3x3
//! dot-matrix glyph, the tooltip and the taskbar badge.
//!
//! Driven by the status flow (`bridge::push_pane_status_sse` calls [`notify_changed`], and the
//! `pty://panes-changed` event does too), never by the PTY reader. The thread sleeps on a
//! condvar: 250 ms per frame only while a sweep animates, otherwise until the next change (with a
//! slow 30 s safety refresh that also re-reads the Windows light/dark taskbar setting).

use super::glyph::{self, Aggregate, Ticker};
use super::raster::{self, Ink};
use crate::AppState;
use once_cell::sync::Lazy;
use parking_lot::{Condvar, Mutex};
use std::time::Duration;
use tauri::image::Image;
use tauri::{AppHandle, Manager};

const TRAY_ID: &str = "puppet-master-tray";
const ICON_PX: u32 = 32;
#[cfg(windows)]
const BADGE_PX: u32 = 32;
const SAFETY_REFRESH: Duration = Duration::from_secs(30);

static DIRTY: Lazy<(Mutex<bool>, Condvar)> = Lazy::new(|| (Mutex::new(false), Condvar::new()));

/// Something that may change the aggregate happened (pane status, pane list, setting).
pub fn notify_changed() {
    let (flag, cv) = &*DIRTY;
    *flag.lock() = true;
    cv.notify_all();
}

/// Is the Windows taskbar light? Black dots are drawn on a light taskbar.
#[cfg(windows)]
pub fn taskbar_is_light() -> bool {
    #[link(name = "advapi32")]
    extern "system" {
        fn RegGetValueW(
            hkey: isize,
            subkey: *const u16,
            value: *const u16,
            flags: u32,
            kind: *mut u32,
            data: *mut core::ffi::c_void,
            size: *mut u32,
        ) -> i32;
    }
    const HKEY_CURRENT_USER: isize = 0x8000_0001_u32 as i32 as isize;
    const RRF_RT_REG_DWORD: u32 = 0x10;
    let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let subkey = wide(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
    let value = wide("SystemUsesLightTheme");
    let mut data: u32 = 0;
    let mut size: u32 = 4;
    // SAFETY: all pointers are valid for the call; RegGetValueW writes at most `size` bytes.
    let rc = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&mut data as *mut u32).cast(),
            &mut size,
        )
    };
    rc == 0 && data == 1
}

#[cfg(not(windows))]
pub fn taskbar_is_light() -> bool {
    false
}

fn ink_for(light_taskbar: bool) -> Ink {
    if light_taskbar {
        Ink::Black
    } else {
        Ink::White
    }
}

fn read_aggregate(app: &AppHandle) -> Aggregate {
    let Some(state) = app.try_state::<AppState>() else { return Aggregate::default() };
    let panes = state.registry.lock().list();
    let live: Vec<String> = panes.iter().map(|p| p.id.clone()).collect();
    super::store::retain_live(&live);
    // Warm standby panes are invisible plumbing, not the user's work.
    let statuses = panes.iter().filter(|p| !crate::pty::hidden::is_hidden(&p.id)).map(|p| p.status.as_str());
    aggregate_for(statuses, crate::attention::counts().blocking)
}

/// Pane statuses for the glyph pattern, plus the one attention count for the badge and tooltip.
pub(crate) fn aggregate_for<'a>(statuses: impl IntoIterator<Item = &'a str>, blocking: u32) -> Aggregate {
    Aggregate::from_statuses(statuses).with_blocking(blocking)
}

/// Everything drawn for one moment; compared to skip redundant OS calls.
#[derive(PartialEq)]
struct Look {
    pattern: glyph::Pattern,
    ink: Ink,
    tooltip: String,
    badge: u32,
}

fn apply(app: &AppHandle, look: &Look) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let rgba = raster::render_rgba(&glyph::levels(look.pattern), ICON_PX, look.ink);
        let _ = tray.set_icon(Some(Image::new_owned(rgba, ICON_PX, ICON_PX)));
        let _ = tray.set_tooltip(Some(look.tooltip.as_str()));
    }
    set_badge(app, look.badge);
}

/// Taskbar overlay badge on the cockpit window: needs-you + error count. Uses Tauri's
/// `set_overlay_icon`, which is `ITaskbarList3::SetOverlayIcon` on Windows; other platforms ignore it.
fn set_badge(app: &AppHandle, count: u32) {
    #[cfg(windows)]
    if let Some(window) = app.get_webview_window(crate::app_windows::COCKPIT_LABEL) {
        let icon = (count > 0).then(|| Image::new_owned(raster::badge_rgba(count, BADGE_PX), BADGE_PX, BADGE_PX));
        let _ = window.set_overlay_icon(icon);
    }
    #[cfg(not(windows))]
    let _ = (app, count);
}

/// Start the ticker thread (once, after the tray exists).
pub fn start(app: &AppHandle) {
    let app = app.clone();
    let _ = std::thread::Builder::new().name("ambient-tray".into()).spawn(move || run(app));
}

fn run(app: AppHandle) {
    let mut ticker = Ticker::new(super::prefs::animated());
    let mut ink = ink_for(taskbar_is_light());
    let mut last: Option<Look> = None;
    let mut last_cockpit = false;
    notify_changed();
    loop {
        let timed_out;
        {
            let (flag, cv) = &*DIRTY;
            let mut dirty = flag.lock();
            if !*dirty {
                let wait = ticker.next_wait().unwrap_or(SAFETY_REFRESH);
                cv.wait_for(&mut dirty, wait);
            }
            timed_out = !*dirty;
            *dirty = false;
        }
        if timed_out && ticker.next_wait().is_some() {
            ticker.tick();
        } else {
            // A change, or the slow safety refresh: re-read everything, including the taskbar theme.
            ink = ink_for(taskbar_is_light());
            ticker.update(read_aggregate(&app), super::prefs::animated());
        }
        let agg = ticker.aggregate();
        let look = Look { pattern: ticker.pattern(), ink, tooltip: glyph::tooltip(&agg), badge: agg.attention() };
        let cockpit_present = app.get_webview_window(crate::app_windows::COCKPIT_LABEL).is_some();
        if last.as_ref() != Some(&look) || cockpit_present != last_cockpit {
            apply(&app, &look);
            last_cockpit = cockpit_present;
            last = Some(look);
        }
    }
}
