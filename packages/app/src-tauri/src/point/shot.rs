//! Element screenshots: crop, outline, save, prune, copy, thumbnail (PF-5).
//!
//! Every file name is made here (`pick-<yyyymmdd-hhmmss>-<n>.png`), never taken from the page.
//! Files go only into `<project>/.puppet-master/shots/`. `prune` touches only `pick-*.png` there.

use super::{Rect, Shot, Viewport};
use base64::Engine;
use std::io::{self, Cursor, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// A thumbnail PNG above this many bytes is not sent to the Point box.
const MAX_THUMB_BYTES: usize = 600 * 1024;
/// The outline: a dark 2 px frame on the element edge, with a 1 px light halo just outside it.
/// Both colours read on light and dark pages.
const DARK: [u8; 4] = [17, 17, 17, 255];
const LIGHT: [u8; 4] = [255, 255, 255, 255];
const FRAME_PX: i64 = 2;
const PREFIX: &str = "pick-";
const EXT: &str = ".png";

/// Crop a viewport PNG to `rect` (CSS px) plus `pad_css` on every side, and draw the outline.
/// The scale is the PNG width over the viewport width, so zoom and DPI are handled.
/// Returns the new PNG bytes and their size in picture pixels.
pub fn crop(png: &[u8], rect: Rect, viewport: Viewport, pad_css: f64) -> Result<(Vec<u8>, u32, u32), String> {
    const BAD_RECT: &str = "The element has no size on screen.";
    let finite = [rect.x, rect.y, rect.w, rect.h, viewport.w, viewport.h, pad_css]
        .iter()
        .all(|v| v.is_finite());
    if !finite || viewport.w <= 0.0 || rect.w <= 0.0 || rect.h <= 0.0 || pad_css < 0.0 {
        return Err(BAD_RECT.to_string());
    }
    let pic = decode(png)?;
    let scale = f64::from(pic.w) / viewport.w;
    let pad = pad_css * scale;
    // Element edges and crop edges, in picture pixels.
    let ex0 = rect.x * scale;
    let ey0 = rect.y * scale;
    let ex1 = (rect.x + rect.w) * scale;
    let ey1 = (rect.y + rect.h) * scale;
    let cx0 = clamp_px((ex0 - pad).floor(), pic.w);
    let cy0 = clamp_px((ey0 - pad).floor(), pic.h);
    let cx1 = clamp_px((ex1 + pad).ceil(), pic.w);
    let cy1 = clamp_px((ey1 + pad).ceil(), pic.h);
    if cx1 <= cx0 || cy1 <= cy0 {
        return Err("The element is not on screen any more.".to_string());
    }
    let (out_w, out_h) = (cx1 - cx0, cy1 - cy0);
    let mut out = Vec::with_capacity((out_w as usize) * (out_h as usize) * 4);
    for y in cy0..cy1 {
        let start = ((y * pic.w + cx0) * 4) as usize;
        let end = ((y * pic.w + cx1) * 4) as usize;
        out.extend_from_slice(&pic.rgba[start..end]);
    }
    // The element box inside the crop (inclusive pixel edges).
    let l = (ex0.round() as i64) - i64::from(cx0);
    let t = (ey0.round() as i64) - i64::from(cy0);
    let r = (ex1.round() as i64) - i64::from(cx0) - 1;
    let b = (ey1.round() as i64) - i64::from(cy0) - 1;
    let (w, h) = (i64::from(out_w), i64::from(out_h));
    frame(&mut out, w, h, (l - 1, t - 1, r + 1, b + 1), 1, LIGHT);
    frame(&mut out, w, h, (l, t, r, b), FRAME_PX, DARK);
    let bytes = encode(out_w, out_h, &out)?;
    Ok((bytes, out_w, out_h))
}

/// Paint `thick` pixels along the border of the inclusive box `(l, t, r, b)`, clipped to `w` x `h`.
fn frame(px: &mut [u8], w: i64, h: i64, bx: (i64, i64, i64, i64), thick: i64, color: [u8; 4]) {
    let (l, t, r, b) = bx;
    for y in t.max(0)..=b.min(h - 1) {
        for x in l.max(0)..=r.min(w - 1) {
            let on_border = x < l + thick || x > r - thick || y < t + thick || y > b - thick;
            if on_border {
                let i = ((y * w + x) * 4) as usize;
                px[i..i + 4].copy_from_slice(&color);
            }
        }
    }
}

fn clamp_px(v: f64, max: u32) -> u32 {
    if v.is_nan() || v <= 0.0 {
        0
    } else if v >= f64::from(max) {
        max
    } else {
        v as u32
    }
}

/// A decoded picture: size and RGBA8 pixels.
struct Pixels {
    w: u32,
    h: u32,
    rgba: Vec<u8>,
}

fn decode(png: &[u8]) -> Result<Pixels, String> {
    const NOT_PNG: &str = "The screenshot is not a PNG picture.";
    const BAD_FORMAT: &str = "Only 8-bit RGB or RGBA screenshots are supported.";
    let mut reader = png::Decoder::new(Cursor::new(png))
        .read_info()
        .map_err(|_| NOT_PNG.to_string())?;
    let (color, depth) = reader.output_color_type();
    if depth != png::BitDepth::Eight {
        return Err(BAD_FORMAT.to_string());
    }
    let size = reader.output_buffer_size().ok_or_else(|| NOT_PNG.to_string())?;
    let mut buf = vec![0; size];
    let info = reader.next_frame(&mut buf).map_err(|_| NOT_PNG.to_string())?;
    buf.truncate(info.buffer_size());
    let (w, h) = reader.info().size();
    let rgba = match color {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        _ => return Err(BAD_FORMAT.to_string()),
    };
    if rgba.len() != (w as usize) * (h as usize) * 4 {
        return Err(NOT_PNG.to_string());
    }
    Ok(Pixels { w, h, rgba })
}

fn encode(w: u32, h: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc
        .write_header()
        .map_err(|_| "The screenshot could not be encoded.".to_string())?;
    writer
        .write_image_data(rgba)
        .map_err(|_| "The screenshot could not be encoded.".to_string())?;
    writer
        .finish()
        .map_err(|_| "The screenshot could not be encoded.".to_string())?;
    Ok(out)
}

/// `<project>/.puppet-master/shots`.
pub fn shots_dir(project: &Path) -> PathBuf {
    project.join(".puppet-master").join("shots")
}

/// Write one screenshot into the project's shots folder. Returns (absolute path, relative path).
/// The relative path uses `/` and is the one the agent message shows.
pub fn save(project: &Path, png: &[u8], now_ms: u64, seq: u32) -> io::Result<(PathBuf, String)> {
    crate::git_exclude::ensure_project_storage_excluded(project);
    let dir = shots_dir(project);
    std::fs::create_dir_all(&dir)?;
    let name = format!("{PREFIX}{}-{seq}{EXT}", stamp(now_ms));
    write_atomic(&dir, &name, png)?;
    Ok((dir.join(&name), rel_of(&name)))
}

/// Keep the newest `keep` shots in `dir`: delete the oldest `pick-*.png` files. Other files are
/// never touched. Returns how many files were deleted. A missing folder gives 0.
pub fn prune(dir: &Path, keep: usize) -> usize {
    let Ok(read) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut shots: Vec<(SystemTime, String, PathBuf)> = read
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if !is_shot_name(&name) {
                return None;
            }
            let mtime = e.metadata().and_then(|m| m.modified()).unwrap_or(UNIX_EPOCH);
            Some((mtime, name, e.path()))
        })
        .collect();
    if shots.len() <= keep {
        return 0;
    }
    // Oldest first (by modified time, then by name).
    shots.sort();
    let extra = shots.len() - keep;
    shots
        .into_iter()
        .take(extra)
        .filter(|(_, _, path)| std::fs::remove_file(path).is_ok())
        .count()
}

/// Copy a shot into the target folder's shots folder, so the path in the message is relative to
/// the agent's own folder. A shot already there is returned as it is.
pub fn copy_into(shot: &Shot, project: &Path) -> io::Result<Shot> {
    let name = shot
        .abs_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if !is_shot_name(&name) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Not a screenshot file."));
    }
    crate::git_exclude::ensure_project_storage_excluded(project);
    let dir = shots_dir(project);
    let dest = dir.join(&name);
    if shot.abs_path == dest {
        return Ok(shot.clone());
    }
    std::fs::create_dir_all(&dir)?;
    let bytes = std::fs::read(&shot.abs_path)?;
    write_atomic(&dir, &name, &bytes)?;
    Ok(Shot {
        abs_path: dest,
        rel_path: rel_of(&name),
        width: shot.width,
        height: shot.height,
    })
}

/// A `data:` URL for the Point box thumbnail. `None` above 600 KB of PNG.
pub fn thumb_data_url(png: &[u8]) -> Option<String> {
    if png.is_empty() || png.len() > MAX_THUMB_BYTES {
        return None;
    }
    let b64 = base64::engine::general_purpose::STANDARD.encode(png);
    Some(format!("data:image/png;base64,{b64}"))
}

/// True for the only names PM writes: `pick-*.png`, one path component, no separators.
fn is_shot_name(name: &str) -> bool {
    name.starts_with(PREFIX)
        && name.ends_with(EXT)
        && name.len() > PREFIX.len() + EXT.len()
        && !name.contains(['/', '\\'])
}

fn rel_of(name: &str) -> String {
    format!(".puppet-master/shots/{name}")
}

/// Local time as `yyyymmdd-hhmmss`. Falls back to zeros if the clock value is out of range.
fn stamp(now_ms: u64) -> String {
    use chrono::TimeZone;
    match chrono::Local.timestamp_millis_opt(now_ms as i64).single() {
        Some(t) => t.format("%Y%m%d-%H%M%S").to_string(),
        None => "00000000-000000".to_string(),
    }
}

/// Write to a dot-named temp file, then rename it over the final name.
fn write_atomic(dir: &Path, name: &str, bytes: &[u8]) -> io::Result<()> {
    let tmp = dir.join(format!(".{name}.tmp"));
    {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.flush()?;
    }
    std::fs::rename(&tmp, dir.join(name)).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}
