// Behaviour from desktoplab (github.com/v7ren/desktoplab), commit 6e5360e, MIT.
//! The Windows clipboard: text and one picture. Rich text, web pages and file lists are left alone.
//!
//! Read: Unicode text, and a picture as PNG (the "PNG" format when a program offers it, otherwise the plain bitmap,
//! converted). Write: text, and a picture as both "PNG" and a plain bitmap, so Paint, Word, Slack and the browser can
//! all paste it. A locked clipboard (another program has it open for a moment) is retried briefly.
//!
//! Nothing here logs or formats contents. Errors carry only a reason, never data.

use std::io::Cursor;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::{CF_DIB, CF_UNICODETEXT};

use crate::desk::link::clipboard::CAP;
use crate::desk::link::remote::ClipItem;

const OPEN_TRIES: u32 = 10;

/// Open for the length of one call, however it ends.
struct Open;

impl Open {
    fn new() -> Result<Self, String> {
        for _ in 0..OPEN_TRIES {
            // SAFETY: plain call; closed in Drop.
            if unsafe { OpenClipboard(None) }.is_ok() {
                return Ok(Open);
            }
            std::thread::sleep(std::time::Duration::from_millis(15));
        }
        Err("the clipboard is in use by another program".into())
    }
}

impl Drop for Open {
    fn drop(&mut self) {
        // SAFETY: closes what `new` opened.
        unsafe {
            let _ = CloseClipboard();
        }
    }
}

fn png_format() -> u32 {
    let name: Vec<u16> = "PNG".encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: `name` is a valid nul-terminated string for the call.
    unsafe { RegisterClipboardFormatW(PCWSTR(name.as_ptr())) }
}

pub fn read() -> Result<Vec<ClipItem>, String> {
    let _open = Open::new()?;
    let mut items = Vec::new();
    // SAFETY: the clipboard is open; every handle is locked and unlocked inside the helpers.
    unsafe {
        if let Some(text) = read_text() {
            if !text.is_empty() {
                items.push(ClipItem::Text(text));
            }
        }
        let png = read_bytes(png_format()).or_else(|| read_bytes(CF_DIB.0 as u32).and_then(|dib| dib_to_png(&dib)));
        if let Some(png) = png {
            items.push(ClipItem::Png(png));
        }
    }
    Ok(items)
}

pub fn write(items: &[ClipItem]) -> Result<(), String> {
    let _open = Open::new()?;
    // SAFETY: the clipboard is open.
    unsafe {
        EmptyClipboard().map_err(|e| e.to_string())?;
        for item in items {
            match item {
                ClipItem::Text(t) => {
                    let mut wide: Vec<u16> = t.encode_utf16().collect();
                    wide.push(0);
                    let bytes = std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2);
                    set_raw(CF_UNICODETEXT.0 as u32, bytes)?;
                }
                ClipItem::Png(png) => {
                    set_raw(png_format(), png)?;
                    if let Some(dib) = png_to_dib(png) {
                        set_raw(CF_DIB.0 as u32, &dib)?;
                    }
                }
            }
        }
    }
    Ok(())
}

unsafe fn read_text() -> Option<String> {
    if IsClipboardFormatAvailable(CF_UNICODETEXT.0 as u32).is_err() {
        return None;
    }
    let handle = GetClipboardData(CF_UNICODETEXT.0 as u32).ok()?;
    let h = HGLOBAL(handle.0);
    let ptr = GlobalLock(h) as *const u16;
    if ptr.is_null() {
        return None;
    }
    let max = GlobalSize(h) / 2;
    let mut len = 0usize;
    while len < max && *ptr.add(len) != 0 {
        len += 1;
    }
    let text = String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len));
    let _ = GlobalUnlock(h);
    Some(text)
}

unsafe fn read_bytes(format: u32) -> Option<Vec<u8>> {
    if format == 0 || IsClipboardFormatAvailable(format).is_err() {
        return None;
    }
    let handle = GetClipboardData(format).ok()?;
    let h = HGLOBAL(handle.0);
    let size = GlobalSize(h);
    if size == 0 || size > CAP {
        return None;
    }
    let ptr = GlobalLock(h) as *const u8;
    if ptr.is_null() {
        return None;
    }
    let bytes = std::slice::from_raw_parts(ptr, size).to_vec();
    let _ = GlobalUnlock(h);
    Some(bytes)
}

unsafe fn set_raw(format: u32, bytes: &[u8]) -> Result<(), String> {
    if format == 0 {
        return Err("unknown clipboard format".into());
    }
    let h = GlobalAlloc(GMEM_MOVEABLE, bytes.len()).map_err(|e| e.to_string())?;
    let ptr = GlobalLock(h) as *mut u8;
    if ptr.is_null() {
        let _ = GlobalFree(Some(h));
        return Err("could not lock memory".into());
    }
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
    let _ = GlobalUnlock(h);
    if let Err(e) = SetClipboardData(format, Some(HANDLE(h.0))) {
        let _ = GlobalFree(Some(h));
        return Err(e.to_string());
    }
    Ok(())
}

/// An uncompressed 24 or 32 bit bitmap (what screenshots are) as PNG. Anything else is skipped.
fn dib_to_png(dib: &[u8]) -> Option<Vec<u8>> {
    if dib.len() < 40 {
        return None;
    }
    let u32_at = |o: usize| u32::from_le_bytes(dib[o..o + 4].try_into().unwrap());
    let header = u32_at(0) as usize;
    let width = i32::from_le_bytes(dib[4..8].try_into().ok()?);
    let height = i32::from_le_bytes(dib[8..12].try_into().ok()?);
    let bpp = u16::from_le_bytes(dib[14..16].try_into().ok()?);
    let compression = u32_at(16);
    if header < 40 || width <= 0 || height == 0 || !(bpp == 24 || bpp == 32) || !(compression == 0 || compression == 3) {
        return None;
    }
    let (w, h) = (width as usize, height.unsigned_abs() as usize);
    let top_down = height < 0;
    let masks = if compression == 3 && header == 40 { 12 } else { 0 };
    let start = header + masks;
    let stride = (w * usize::from(bpp)).div_ceil(32) * 4;
    if w.checked_mul(h)? > 100_000_000 || dib.len() < start + stride * h {
        return None;
    }
    let mut rgba = Vec::with_capacity(w * h * 4);
    for row in 0..h {
        let src_row = if top_down { row } else { h - 1 - row };
        let line = &dib[start + src_row * stride..start + src_row * stride + stride];
        for x in 0..w {
            let px = &line[x * usize::from(bpp / 8)..];
            rgba.extend_from_slice(&[px[2], px[1], px[0], 255]);
        }
    }
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(Cursor::new(&mut out), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().ok()?;
    writer.write_image_data(&rgba).ok()?;
    writer.finish().ok()?;
    Some(out)
}

/// A PNG as a bottom-up 32 bit bitmap (BITMAPINFOHEADER, no compression).
fn png_to_dib(data: &[u8]) -> Option<Vec<u8>> {
    let mut dec = png::Decoder::new(Cursor::new(data));
    dec.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = dec.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width as usize, info.height as usize);
    if w == 0 || h == 0 || w.checked_mul(h)? > 100_000_000 {
        return None;
    }
    let channels = match info.color_type {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => return None,
    };
    let mut out = Vec::with_capacity(40 + w * h * 4);
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(w as i32).to_le_bytes());
    out.extend_from_slice(&(h as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&[0; 24]);
    for row in (0..h).rev() {
        for x in 0..w {
            let p = &buf[(row * w + x) * channels..];
            let (r, g, b) = if channels >= 3 { (p[0], p[1], p[2]) } else { (p[0], p[0], p[0]) };
            out.extend_from_slice(&[b, g, r, 255]);
        }
    }
    Some(out)
}
