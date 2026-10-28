//! Tests for point/shot.rs: crop, outline, save, prune, copy_into, thumb_data_url.

use crate::point::shot::{copy_into, crop, prune, save, shots_dir, thumb_data_url};
use crate::point::{Rect, Shot, Viewport};
use base64::Engine;
use std::io::Cursor;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

const GREEN: [u8; 4] = [0, 128, 0, 255];
const DARK: [u8; 4] = [17, 17, 17, 255];
const LIGHT: [u8; 4] = [255, 255, 255, 255];

/// A scratch folder that only this test creates. It is removed when the test ends.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("pm-point-shot-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn encode(w: u32, h: u32, color: png::ColorType, depth: png::BitDepth, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, w, h);
    enc.set_color(color);
    enc.set_depth(depth);
    let mut writer = enc.write_header().unwrap();
    writer.write_image_data(data).unwrap();
    writer.finish().unwrap();
    out
}

/// A solid RGBA8 PNG of the given size.
fn solid_png(w: u32, h: u32, fill: [u8; 4]) -> Vec<u8> {
    let data: Vec<u8> = (0..w * h).flat_map(|_| fill).collect();
    encode(w, h, png::ColorType::Rgba, png::BitDepth::Eight, &data)
}

/// Decode a PNG we made: (width, height, RGBA8 pixels).
fn decode_rgba(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
    let mut reader = png::Decoder::new(Cursor::new(bytes)).read_info().unwrap();
    assert_eq!(reader.output_color_type().0, png::ColorType::Rgba);
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    buf.truncate(info.buffer_size());
    let (w, h) = reader.info().size();
    (w, h, buf)
}

fn px(buf: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * w + x) * 4) as usize;
    [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
}

fn vp(w: f64, h: f64, dpr: f64) -> Viewport {
    Viewport { w, h, dpr }
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> Rect {
    Rect { x, y, w, h }
}

fn set_mtime(path: &PathBuf, secs: u64) {
    let when = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000 + secs);
    std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(when)
        .unwrap();
}

#[test]
fn crop_size_follows_the_device_pixel_ratio() {
    // The same 100 x 50 CSS px viewport at three zoom levels. Pad is 24 CSS px.
    // Element 20 x 10 CSS px at (40, 10): x 16..84 and y 0..44 in CSS px (y is clamped at 0).
    let r = rect(40.0, 10.0, 20.0, 10.0);
    let (out, w, h) = crop(&solid_png(100, 50, GREEN), r.clone(), vp(100.0, 50.0, 1.0), 24.0).unwrap();
    assert_eq!((w, h), (68, 44));
    let (dw, dh, _) = decode_rgba(&out);
    assert_eq!((dw, dh), (68, 44));

    let (_, w, h) = crop(&solid_png(150, 75, GREEN), r.clone(), vp(100.0, 50.0, 1.5), 24.0).unwrap();
    assert_eq!((w, h), (102, 66));

    let (_, w, h) = crop(&solid_png(200, 100, GREEN), r, vp(100.0, 50.0, 2.0), 24.0).unwrap();
    assert_eq!((w, h), (136, 88));
}

#[test]
fn crop_at_the_edge_is_clamped_to_the_picture() {
    // Element at x 90..100 of a 100 px wide viewport: the right pad is cut at the picture edge.
    let (_, w, h) = crop(&solid_png(100, 50, GREEN), rect(90.0, 10.0, 10.0, 10.0), vp(100.0, 50.0, 1.0), 24.0)
        .unwrap();
    assert_eq!((w, h), (34, 44));
}

#[test]
fn outline_is_drawn_on_the_element_edge_and_the_inside_is_kept() {
    // Crop origin is (16, 0). The element sits at (24, 10) inside the crop, 20 x 10 px.
    let r = rect(40.0, 10.0, 20.0, 10.0);
    let (out, w, _) = crop(&solid_png(100, 50, GREEN), r, vp(100.0, 50.0, 1.0), 24.0).unwrap();
    let (cw, _, buf) = decode_rgba(&out);
    assert_eq!(cw, w);
    // Dark 2 px frame on the element edge.
    assert_eq!(px(&buf, w, 24, 15), DARK);
    assert_eq!(px(&buf, w, 25, 15), DARK);
    assert_eq!(px(&buf, w, 40, 10), DARK);
    // Light 1 px halo just outside it.
    assert_eq!(px(&buf, w, 23, 15), LIGHT);
    assert_eq!(px(&buf, w, 40, 9), LIGHT);
    // The inside of the element and the padding stay as the page drew them.
    assert_eq!(px(&buf, w, 30, 15), GREEN);
    assert_eq!(px(&buf, w, 5, 5), GREEN);
}

#[test]
fn rgb_png_is_accepted_and_comes_back_as_rgba() {
    let data: Vec<u8> = (0..100 * 50).flat_map(|_| [0u8, 128, 0]).collect();
    let png = encode(100, 50, png::ColorType::Rgb, png::BitDepth::Eight, &data);
    let (out, w, h) = crop(&png, rect(40.0, 10.0, 20.0, 10.0), vp(100.0, 50.0, 1.0), 24.0).unwrap();
    assert_eq!((w, h), (68, 44));
    let (dw, dh, buf) = decode_rgba(&out);
    assert_eq!((dw, dh), (68, 44));
    assert_eq!(px(&buf, dw, 30, 15), GREEN);
}

#[test]
fn bad_input_is_refused_with_a_sentence() {
    let ok_rect = rect(40.0, 10.0, 20.0, 10.0);
    let view = vp(100.0, 50.0, 1.0);
    // Not a PNG.
    assert!(crop(b"not a picture", ok_rect.clone(), view.clone(), 24.0).is_err());
    // 16-bit PNG is not supported.
    let data16: Vec<u8> = (0..100 * 50 * 4 * 2).map(|i| (i % 251) as u8).collect();
    let png16 = encode(100, 50, png::ColorType::Rgba, png::BitDepth::Sixteen, &data16);
    assert!(crop(&png16, ok_rect.clone(), view.clone(), 24.0).is_err());
    // No size, NaN, negative pad.
    assert!(crop(&solid_png(100, 50, GREEN), rect(40.0, 10.0, 0.0, 10.0), view.clone(), 24.0).is_err());
    assert!(crop(&solid_png(100, 50, GREEN), rect(f64::NAN, 10.0, 5.0, 5.0), view.clone(), 24.0).is_err());
    assert!(crop(&solid_png(100, 50, GREEN), ok_rect.clone(), view.clone(), -1.0).is_err());
    // Element entirely past the right edge.
    let err = crop(&solid_png(100, 50, GREEN), rect(200.0, 10.0, 10.0, 10.0), view, 24.0).unwrap_err();
    assert_eq!(err, "The element is not on screen any more.");
}

#[test]
fn save_writes_one_named_file_under_the_shots_folder() {
    let scratch = Scratch::new("save");
    let project = scratch.0.join("app");
    let bytes = solid_png(4, 3, GREEN);
    let (abs, rel) = save(&project, &bytes, 1_800_000_000_000, 7).unwrap();

    assert_eq!(abs.parent().unwrap(), shots_dir(&project));
    let name = rel.strip_prefix(".puppet-master/shots/").expect("relative path");
    assert!(name.starts_with("pick-"), "{name}");
    assert!(name.ends_with("-7.png"), "{name}");
    // pick-yyyymmdd-hhmmss-7.png: 5 + 8 + 1 + 6 + 2 + 4 characters.
    assert_eq!(name.len(), "pick-".len() + 15 + "-7.png".len());
    assert_eq!(std::fs::read(&abs).unwrap(), bytes);
    // No temp file is left behind.
    let entries: Vec<_> = std::fs::read_dir(shots_dir(&project)).unwrap().collect();
    assert_eq!(entries.len(), 1);
}

#[test]
fn save_keeps_earlier_shots_when_the_sequence_moves_on() {
    let scratch = Scratch::new("seq");
    let project = scratch.0.join("app");
    let (first, _) = save(&project, &solid_png(2, 2, GREEN), 1_800_000_000_000, 1).unwrap();
    let (second, _) = save(&project, &solid_png(2, 2, LIGHT), 1_800_000_000_000, 2).unwrap();
    assert_ne!(first, second);
    assert!(first.exists() && second.exists());
}

#[test]
fn prune_keeps_the_newest_and_touches_only_pick_pngs() {
    let scratch = Scratch::new("prune");
    let dir = scratch.0.join("shots");
    std::fs::create_dir_all(&dir).unwrap();
    for i in 1..=5u64 {
        let path = dir.join(format!("pick-20261028-22310{i}-{i}.png"));
        std::fs::write(&path, b"x").unwrap();
        set_mtime(&path, i);
    }
    for other in ["notes.txt", "keep.png", "pick-x.txt"] {
        let path = dir.join(other);
        std::fs::write(&path, b"keep me").unwrap();
        set_mtime(&path, 0);
    }

    assert_eq!(prune(&dir, 3), 2);

    let mut left: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    assert_eq!(
        left,
        vec![
            "keep.png",
            "notes.txt",
            "pick-20261028-223103-3.png",
            "pick-20261028-223104-4.png",
            "pick-20261028-223105-5.png",
            "pick-x.txt",
        ]
    );
    // Under the cap nothing happens; a missing folder gives 0.
    assert_eq!(prune(&dir, 50), 0);
    assert_eq!(prune(&scratch.0.join("missing"), 3), 0);
}

#[test]
fn copy_into_puts_the_shot_in_the_other_folder_with_a_relative_path() {
    let scratch = Scratch::new("copy");
    let preview_app = scratch.0.join("preview-app");
    let agent_app = scratch.0.join("agent-app");
    let bytes = solid_png(20, 10, GREEN);
    let (abs, rel) = save(&preview_app, &bytes, 1_800_000_000_000, 3).unwrap();
    let shot = Shot { abs_path: abs.clone(), rel_path: rel.clone(), width: 20, height: 10 };

    let copied = copy_into(&shot, &agent_app).unwrap();
    assert_eq!(copied.abs_path.parent().unwrap(), shots_dir(&agent_app));
    assert_eq!(copied.rel_path, rel);
    assert_eq!((copied.width, copied.height), (20, 10));
    assert_eq!(std::fs::read(&copied.abs_path).unwrap(), bytes);

    // Already in the folder: the same shot comes back, nothing is written.
    let again = copy_into(&copied, &agent_app).unwrap();
    assert_eq!(again, copied);
}

#[test]
fn copy_into_refuses_a_file_that_is_not_a_pick_shot() {
    let scratch = Scratch::new("copy-refuse");
    let other = scratch.0.join("notes.png");
    std::fs::write(&other, b"not a pick").unwrap();
    let shot = Shot { abs_path: other, rel_path: "notes.png".into(), width: 1, height: 1 };
    let err = copy_into(&shot, &scratch.0.join("agent-app")).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    assert!(!shots_dir(&scratch.0.join("agent-app")).exists());
}

#[test]
fn thumbnail_is_a_png_data_url_up_to_600_kb_only() {
    let small = solid_png(4, 4, GREEN);
    let url = thumb_data_url(&small).expect("small thumbnail");
    let b64 = url.strip_prefix("data:image/png;base64,").expect("prefix");
    assert_eq!(base64::engine::general_purpose::STANDARD.decode(b64).unwrap(), small);

    assert!(thumb_data_url(&[]).is_none());
    assert!(thumb_data_url(&vec![0u8; 600 * 1024]).is_some());
    assert!(thumb_data_url(&vec![0u8; 600 * 1024 + 1]).is_none());
}
