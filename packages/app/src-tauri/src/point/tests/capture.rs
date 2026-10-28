//! Tests for point/capture.rs: the pure helpers. The WebView2 call itself needs a live window
//! (checked in the live check); on other systems `viewport_png` returns a plain sentence.

use crate::point::capture::{
    check_png, png_size, wait_png, ERR_BAD_IMAGE, ERR_CLOSED, ERR_EMPTY, ERR_FAILED, ERR_TIMEOUT, MAX_PNG_BYTES,
};
use std::sync::mpsc;
use std::time::Duration;

/// A real PNG made with the `png` crate.
fn tiny_png(w: u32, h: u32) -> Vec<u8> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().unwrap();
    writer.write_image_data(&vec![200u8; (w * h * 4) as usize]).unwrap();
    writer.finish().unwrap();
    out
}

#[test]
fn png_size_reads_width_and_height() {
    assert_eq!(png_size(&tiny_png(3, 2)), Some((3, 2)));
    assert_eq!(png_size(&tiny_png(1920, 1080)), Some((1920, 1080)));
}

#[test]
fn png_size_refuses_non_png_and_short_input() {
    assert_eq!(png_size(b""), None);
    assert_eq!(png_size(b"GIF89a this is not a png at all, but long enough to pass the length check"), None);
    let mut cut = tiny_png(3, 2);
    cut.truncate(20);
    assert_eq!(png_size(&cut), None);
}

#[test]
fn png_size_refuses_zero_dimensions() {
    let mut bad = tiny_png(3, 2);
    bad[16..20].copy_from_slice(&0u32.to_be_bytes());
    assert_eq!(png_size(&bad), None);
}

#[test]
fn check_png_accepts_a_real_png_unchanged() {
    let png = tiny_png(4, 4);
    assert_eq!(check_png(png.clone()), Ok(png));
}

#[test]
fn check_png_refuses_empty_garbage_and_oversize() {
    assert_eq!(check_png(Vec::new()), Err(ERR_EMPTY));
    assert_eq!(check_png(vec![1, 2, 3, 4]), Err(ERR_BAD_IMAGE));
    let mut huge = tiny_png(2, 2);
    huge.resize(MAX_PNG_BYTES + 1, 0);
    assert_eq!(check_png(huge), Err(ERR_BAD_IMAGE));
}

#[test]
fn wait_png_returns_the_picture_when_the_callback_answers() {
    let (tx, rx) = mpsc::channel();
    let png = tiny_png(5, 5);
    tx.send(Ok(png.clone())).unwrap();
    assert_eq!(wait_png(&rx, Duration::from_millis(200)), Ok(png));
}

#[test]
fn wait_png_passes_a_capture_error_through() {
    let (tx, rx) = mpsc::channel();
    tx.send(Err(ERR_FAILED)).unwrap();
    assert_eq!(wait_png(&rx, Duration::from_millis(200)), Err(ERR_FAILED));
}

#[test]
fn wait_png_checks_what_the_callback_sent() {
    let (tx, rx) = mpsc::channel();
    tx.send(Ok(vec![0u8; 10])).unwrap();
    assert_eq!(wait_png(&rx, Duration::from_millis(200)), Err(ERR_BAD_IMAGE));
}

#[test]
fn wait_png_times_out_with_a_plain_sentence() {
    let (_tx, rx) = mpsc::channel::<Result<Vec<u8>, &'static str>>();
    assert_eq!(wait_png(&rx, Duration::from_millis(30)), Err(ERR_TIMEOUT));
    assert_eq!(ERR_TIMEOUT, "The screenshot did not finish in time.");
}

#[test]
fn wait_png_reports_closed_when_the_callback_never_ran() {
    let (tx, rx) = mpsc::channel::<Result<Vec<u8>, &'static str>>();
    drop(tx);
    assert_eq!(wait_png(&rx, Duration::from_millis(200)), Err(ERR_CLOSED));
}

#[test]
fn a_late_answer_after_the_timeout_is_dropped_without_panic() {
    // The WebView2 callback sends with `let _ =`; once the waiter gave up, the send just fails.
    let (tx, rx) = mpsc::channel::<Result<Vec<u8>, &'static str>>();
    assert_eq!(wait_png(&rx, Duration::from_millis(10)), Err(ERR_TIMEOUT));
    drop(rx);
    assert!(tx.send(Ok(tiny_png(2, 2))).is_err());
}

/// The picture is device pixels: crop's scale is png width over viewport width.
#[test]
fn png_width_over_viewport_width_gives_the_dpi_scale() {
    let (w, _) = png_size(&tiny_png(2560, 1440)).unwrap();
    assert_eq!(f64::from(w) / 1280.0, 2.0);
}

#[cfg(not(windows))]
#[test]
fn other_systems_get_the_windows_sentence() {
    assert_eq!(crate::point::capture::ERR_NO_WINDOWS, "Screenshots need Windows for now.");
}
