//! Native screenshot of a preview window (WebView2 on Windows) (PF-9).
//!
//! The page never takes the picture: Puppet Master asks WebView2 for the viewport as a PNG
//! (`CapturePreview`). That shows only the preview's own page, never other windows or the desktop.
//! The PNG has device pixels (CSS size x device ratio x zoom); `shot::crop` divides by the viewport
//! width the picker reported, so DPI scaling is handled there and not here.
//!
//! Threads: `viewport_png` must be called from a worker thread (the pull thread), never from the
//! main thread. It hands the WebView2 call to the main thread with `with_webview`, which returns at
//! once; the WebView2 completion callback also runs on the main thread, sends the bytes over a
//! channel, and this function waits on that channel with a timeout.

use std::sync::mpsc;
use std::time::Duration;

pub const ERR_NO_WINDOWS: &str = "Screenshots need Windows for now.";
pub const ERR_TIMEOUT: &str = "The screenshot did not finish in time.";
pub const ERR_CLOSED: &str = "The preview window is closed.";
pub const ERR_FAILED: &str = "The screenshot could not be taken.";
pub const ERR_EMPTY: &str = "The screenshot came back empty.";
pub const ERR_BAD_IMAGE: &str = "The screenshot was not a valid picture.";

/// Largest PNG we accept from WebView2 (a 4K viewport is a few MB).
pub const MAX_PNG_BYTES: usize = 32 * 1024 * 1024;

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// Width and height of a PNG from its IHDR chunk, or None when the bytes are not a PNG.
pub fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 24 || bytes[..8] != PNG_SIGNATURE || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let w = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
    let h = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
    (w > 0 && h > 0).then_some((w, h))
}

/// Accepts the bytes WebView2 wrote only if they look like a real, sane PNG.
pub fn check_png(bytes: Vec<u8>) -> Result<Vec<u8>, &'static str> {
    if bytes.is_empty() {
        return Err(ERR_EMPTY);
    }
    if bytes.len() > MAX_PNG_BYTES || png_size(&bytes).is_none() {
        return Err(ERR_BAD_IMAGE);
    }
    Ok(bytes)
}

/// Waits for the capture result on the channel. A closed channel means the callback was dropped
/// without running (the window went away); a timeout means WebView2 never answered.
pub fn wait_png(rx: &mpsc::Receiver<Result<Vec<u8>, &'static str>>, timeout: Duration) -> Result<Vec<u8>, &'static str> {
    match rx.recv_timeout(timeout) {
        Ok(result) => result.and_then(check_png),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(ERR_TIMEOUT),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(ERR_CLOSED),
    }
}

/// PNG of the preview window's viewport. Call from a worker thread, never the main thread.
#[cfg(windows)]
pub fn viewport_png(window: &tauri::WebviewWindow, timeout: Duration) -> Result<Vec<u8>, &'static str> {
    let (tx, rx) = mpsc::channel::<Result<Vec<u8>, &'static str>>();
    let tx_start = tx.clone();
    // `with_webview` runs this closure on the main thread and returns; it does not wait for the picture.
    window
        .with_webview(move |pv| {
            if let Err(e) = windows_impl::start(&pv, tx) {
                let _ = tx_start.send(Err(e));
            }
        })
        .map_err(|_| ERR_CLOSED)?;
    wait_png(&rx, timeout)
}

#[cfg(not(windows))]
pub fn viewport_png(_window: &tauri::WebviewWindow, _timeout: Duration) -> Result<Vec<u8>, &'static str> {
    Err(ERR_NO_WINDOWS)
}

#[cfg(windows)]
mod windows_impl {
    use super::{ERR_FAILED, MAX_PNG_BYTES};
    use std::ffi::c_void;
    use std::sync::mpsc::Sender;
    use webview2_com::Microsoft::Web::WebView2::Win32::COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG;
    use webview2_com::CapturePreviewCompletedHandler;
    use windows::Win32::System::Com::{IStream, STREAM_SEEK_SET};
    use windows::Win32::UI::Shell::SHCreateMemStream;

    /// Main thread only. Starts the capture; the answer goes to `tx` from the completion callback.
    pub fn start(pv: &tauri::webview::PlatformWebview, tx: Sender<Result<Vec<u8>, &'static str>>) -> Result<(), &'static str> {
        // SAFETY: COM calls on the main (UI) thread, which owns the controller.
        unsafe {
            let core = pv.controller().CoreWebView2().map_err(|_| ERR_FAILED)?;
            let stream = SHCreateMemStream(None).ok_or(ERR_FAILED)?;
            let held = stream.clone();
            let handler = CapturePreviewCompletedHandler::create(Box::new(move |hr| {
                let out = match hr {
                    Ok(()) => read_all(&held),
                    Err(_) => Err(ERR_FAILED),
                };
                let _ = tx.send(out);
                Ok(())
            }));
            core.CapturePreview(COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG, &stream, &handler)
                .map_err(|_| ERR_FAILED)
        }
    }

    /// Reads the whole memory stream from the start, in chunks, up to the size cap.
    unsafe fn read_all(stream: &IStream) -> Result<Vec<u8>, &'static str> {
        stream.Seek(0, STREAM_SEEK_SET, None).map_err(|_| ERR_FAILED)?;
        let mut out = Vec::new();
        let mut chunk = vec![0u8; 64 * 1024];
        loop {
            let mut got = 0u32;
            let hr = stream.Read(chunk.as_mut_ptr() as *mut c_void, chunk.len() as u32, Some(&mut got));
            if hr.is_err() {
                return Err(ERR_FAILED);
            }
            if got == 0 {
                break;
            }
            out.extend_from_slice(&chunk[..got as usize]);
            if out.len() > MAX_PNG_BYTES {
                break; // check_png refuses it
            }
        }
        Ok(out)
    }
}
