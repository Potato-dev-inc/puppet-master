// Behaviour from desktoplab (github.com/v7ren/desktoplab), commit 6e5360e, MIT.
//! Screen list in physical pixels. Origins can be negative (a screen left of or above the main one).

use std::mem::size_of;

use windows::core::BOOL;
use windows::Win32::Foundation::{LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};

use super::dpi::DpiScope;
use crate::desk::host::DisplayInfo;

pub fn list() -> Result<Vec<DisplayInfo>, String> {
    let _dpi = DpiScope::new();
    let mut out: Vec<DisplayInfo> = Vec::new();
    // SAFETY: `out` outlives the call; the callback only pushes to it through the pointer passed here.
    let ok = unsafe {
        let data: *mut Vec<DisplayInfo> = &mut out;
        EnumDisplayMonitors(None::<HDC>, None::<*const RECT>, Some(enum_proc), LPARAM(data as isize))
    };
    if !ok.as_bool() {
        return Err("EnumDisplayMonitors failed".into());
    }
    if !out.iter().any(|d| d.primary) {
        if let Some(first) = out.first_mut() {
            first.primary = true;
        }
    }
    Ok(out)
}

unsafe extern "system" fn enum_proc(monitor: HMONITOR, _hdc: HDC, _rect: *mut RECT, data: LPARAM) -> BOOL {
    let list = &mut *(data.0 as *mut Vec<DisplayInfo>);
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
    if GetMonitorInfoW(monitor, &mut info as *mut MONITORINFOEXW as *mut MONITORINFO).as_bool() {
        let rc = info.monitorInfo.rcMonitor;
        let (mut dpi_x, mut dpi_y) = (96u32, 96u32);
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
        let name = device_name(&info.szDevice);
        list.push(DisplayInfo {
            id: name.clone(),
            name,
            x: rc.left,
            y: rc.top,
            width: rc.right.saturating_sub(rc.left).max(0) as u32,
            height: rc.bottom.saturating_sub(rc.top).max(0) as u32,
            scale: f64::from(dpi_x) / 96.0,
            primary: info.monitorInfo.dwFlags & 1 != 0,
        });
    }
    true.into()
}

fn device_name(raw: &[u16]) -> String {
    let end = raw.iter().position(|c| *c == 0).unwrap_or(raw.len());
    String::from_utf16_lossy(&raw[..end])
}
