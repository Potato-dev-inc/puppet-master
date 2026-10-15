//! Rasteriser for the tray glyph and the taskbar count badge. Pure std, no dependencies.
//!
//! `render_rgba` draws a 3x3 grid of dots (alphas from `glyph::levels`) onto a transparent square.
//! `badge_rgba` draws a white disc with a dark count on it, for the taskbar overlay.

use super::glyph::Levels;

/// Colour of the dots: White for a dark taskbar, Black for a light one.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ink {
    White,
    Black,
}

#[allow(dead_code)]
impl Ink {
    pub fn rgb(self) -> [u8; 3] {
        match self {
            Ink::White => [255, 255, 255],
            Ink::Black => [0, 0, 0],
        }
    }
}

/// Grid geometry for a square of `size` px: (pitch, margin, dot radius).
fn geometry(size: u32) -> (f32, f32, f32) {
    let pitch = size as f32 / 3.4;
    (pitch, (size as f32 - 3.0 * pitch) / 2.0, pitch * 0.36)
}

/// Centre coordinate of grid index `i` (0..3) along one axis.
fn centre(i: usize, pitch: f32, margin: f32) -> f32 {
    (i as f32 + 0.5) * pitch + margin
}

/// Grid index whose centre is nearest to coordinate `v`.
fn nearest(v: f32, pitch: f32, margin: f32) -> usize {
    (0..3)
        .min_by(|&a, &b| (v - centre(a, pitch, margin)).abs().total_cmp(&(v - centre(b, pitch, margin)).abs()))
        .unwrap_or(0)
}

/// `size`x`size` RGBA, row-major, straight alpha. Background is transparent (a = 0).
#[allow(dead_code)]
pub fn render_rgba(levels: &Levels, size: u32, ink: Ink) -> Vec<u8> {
    const SS: usize = 4; // supersampling per axis
    let s = size as usize;
    let [r, g, b] = ink.rgb();
    let (pitch, margin, rad) = geometry(size);
    let mut out = vec![0u8; s * s * 4];
    for y in 0..s {
        for x in 0..s {
            let row = nearest(y as f32 + 0.5, pitch, margin);
            let col = nearest(x as f32 + 0.5, pitch, margin);
            let (cx, cy) = (centre(col, pitch, margin), centre(row, pitch, margin));
            let mut hit = 0u32;
            for j in 0..SS {
                for i in 0..SS {
                    let dx = x as f32 + (i as f32 + 0.5) / SS as f32 - cx;
                    let dy = y as f32 + (j as f32 + 0.5) / SS as f32 - cy;
                    hit += (dx * dx + dy * dy <= rad * rad) as u32;
                }
            }
            let cov = hit as f32 / (SS * SS) as f32;
            let a = (255.0 * cov * levels[row][col]).round().clamp(0.0, 255.0) as u8;
            let o = (y * s + x) * 4;
            out[o..o + 4].copy_from_slice(&[r, g, b, a]);
        }
    }
    out
}

/// 3x5 pixel font, `#` = on. Digits 0-9.
const FONT: [[&str; 5]; 10] = [
    ["###", "#.#", "#.#", "#.#", "###"],
    [".#.", "##.", ".#.", ".#.", "###"],
    ["###", "..#", "###", "#..", "###"],
    ["###", "..#", "###", "..#", "###"],
    ["#.#", "#.#", "###", "..#", "..#"],
    ["###", "#..", "###", "..#", "###"],
    ["###", "#..", "###", "#.#", "###"],
    ["###", "..#", "..#", "..#", "..#"],
    ["###", "#.#", "###", "#.#", "###"],
    ["###", "#.#", "###", "..#", "###"],
];

/// Taskbar overlay badge, `size`x`size` RGBA. count 0 is fully transparent. Counts above 99 show "99".
#[allow(dead_code)]
pub fn badge_rgba(count: u32, size: u32) -> Vec<u8> {
    let s = size as i32;
    let mut out = vec![0u8; (s * s * 4) as usize];
    if count == 0 {
        return out;
    }
    let mut put = |x: i32, y: i32, px: [u8; 4]| {
        if x >= 0 && y >= 0 && x < s && y < s {
            let o = ((y * s + x) * 4) as usize;
            out[o..o + 4].copy_from_slice(&px);
        }
    };

    // White disc with a one-pixel anti-aliased edge.
    let c = size as f32 / 2.0;
    let rad = c - 0.5;
    for y in 0..s {
        for x in 0..s {
            let d = ((x as f32 + 0.5 - c).powi(2) + (y as f32 + 0.5 - c).powi(2)).sqrt();
            let a = ((rad + 0.5 - d).clamp(0.0, 1.0) * 255.0).round() as u8;
            if a > 0 {
                put(x, y, [255, 255, 255, a]);
            }
        }
    }

    // Dark digits, centred, integer scale k, one k-wide gap between glyphs.
    let digits: Vec<usize> = match count {
        1..=9 => vec![count as usize],
        10..=99 => vec![(count / 10) as usize, (count % 10) as usize],
        _ => vec![9, 9],
    };
    let k = (s / 16).max(1);
    let n = digits.len() as i32;
    let (w, h) = (n * 3 * k + (n - 1) * k, 5 * k);
    let (x0, y0) = ((s - w) / 2, (s - h) / 2);
    for (i, &dg) in digits.iter().enumerate() {
        let gx = x0 + i as i32 * 4 * k;
        for (row, line) in FONT[dg].iter().enumerate() {
            for (col, _) in line.bytes().enumerate().filter(|(_, ch)| *ch == b'#') {
                for dy in 0..k {
                    for dx in 0..k {
                        put(gx + col as i32 * k + dx, y0 + row as i32 * k + dy, [20, 20, 20, 255]);
                    }
                }
            }
        }
    }
    out
}

/// Minimal PNG encoder for tests: RGBA, 8-bit, filter 0, zlib stored blocks.
#[cfg(test)]
pub(crate) fn png_bytes(rgba: &[u8], w: u32, h: u32) -> Vec<u8> {
    assert_eq!(rgba.len(), (w * h * 4) as usize);
    let stride = w as usize * 4;
    let mut raw = Vec::with_capacity((stride + 1) * h as usize);
    for row in rgba.chunks(stride) {
        raw.push(0);
        raw.extend_from_slice(row);
    }

    let mut zlib = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65535).collect();
    for (i, blk) in blocks.iter().enumerate() {
        zlib.push((i + 1 == blocks.len()) as u8); // BFINAL, BTYPE = 00 (stored)
        let len = blk.len() as u16;
        zlib.extend_from_slice(&len.to_le_bytes());
        zlib.extend_from_slice(&(!len).to_le_bytes());
        zlib.extend_from_slice(blk);
    }
    zlib.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // depth 8, RGBA, deflate, adaptive filter, no interlace

    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    png_chunk(&mut out, b"IHDR", &ihdr);
    png_chunk(&mut out, b"IDAT", &zlib);
    png_chunk(&mut out, b"IEND", &[]);
    out
}

#[cfg(test)]
fn png_chunk(out: &mut Vec<u8>, ty: &[u8; 4], data: &[u8]) {
    let body: Vec<u8> = ty.iter().chain(data).copied().collect();
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(&body);
    out.extend_from_slice(&crc32(&body).to_be_bytes());
}

#[cfg(test)]
fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

#[cfg(test)]
fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in data {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

#[cfg(test)]
mod tests {
    use super::super::glyph::{levels, Pattern};
    use super::*;

    const PATTERNS: [Pattern; 6] = [
        Pattern::NeedsYou,
        Pattern::Error,
        Pattern::Running(0),
        Pattern::Running(3),
        Pattern::RunningStatic,
        Pattern::Idle,
    ];

    fn pixel(img: &[u8], size: u32, x: u32, y: u32) -> [u8; 4] {
        let o = ((y * size + x) * 4) as usize;
        [img[o], img[o + 1], img[o + 2], img[o + 3]]
    }

    /// Pixel index nearest the centre of dot (r, c).
    fn dot_px(size: u32, r: usize, c: usize) -> (u32, u32) {
        let (pitch, margin, _) = geometry(size);
        (centre(c, pitch, margin).floor() as u32, centre(r, pitch, margin).floor() as u32)
    }

    /// Straight alpha over an opaque background, output opaque.
    fn composite(rgba: &[u8], bg: [u8; 3]) -> Vec<u8> {
        rgba.chunks_exact(4)
            .flat_map(|px| {
                let a = px[3] as f32 / 255.0;
                let mut o = [255u8; 4];
                for i in 0..3 {
                    o[i] = (px[i] as f32 * a + bg[i] as f32 * (1.0 - a)).round() as u8;
                }
                o
            })
            .collect()
    }

    #[test]
    fn each_pattern_renders_dots_where_expected() {
        for &size in &[16u32, 32] {
            let tol = if size == 16 { 40 } else { 12 };
            for &p in &PATTERNS {
                let lv = levels(p);
                let img = render_rgba(&lv, size, Ink::White);
                for r in 0..3 {
                    for c in 0..3 {
                        let (x, y) = dot_px(size, r, c);
                        let a = pixel(&img, size, x, y)[3] as i32;
                        let want = (255.0 * lv[r][c]).round() as i32;
                        assert!((a - want).abs() <= tol, "{p:?} size {size} dot ({r},{c}): {a} vs {want}");
                    }
                }
                // The outer 1px ring is background.
                for i in 0..size {
                    for (x, y) in [(i, 0), (i, size - 1), (0, i), (size - 1, i)] {
                        assert_eq!(pixel(&img, size, x, y)[3], 0, "{p:?} size {size} edge ({x},{y})");
                    }
                }
            }
        }
    }

    #[test]
    fn background_is_transparent_and_ink_colour_is_applied() {
        for (ink, want) in [(Ink::White, [255u8, 255, 255]), (Ink::Black, [0u8, 0, 0])] {
            for &p in &PATTERNS {
                let img = render_rgba(&levels(p), 32, ink);
                for px in img.chunks_exact(4).filter(|px| px[3] > 0) {
                    assert_eq!(&px[..3], &want, "{p:?} {ink:?}");
                }
            }
        }
    }

    #[test]
    fn needs_you_centre_is_brightest_and_corners_are_off() {
        let img = render_rgba(&levels(Pattern::NeedsYou), 32, Ink::White);
        let alpha = |r, c| {
            let (x, y) = dot_px(32, r, c);
            pixel(&img, 32, x, y)[3]
        };
        let (centre_a, arm_a, corner_a) = (alpha(1, 1), alpha(0, 1), alpha(0, 0));
        assert!(centre_a > arm_a && arm_a > corner_a, "centre {centre_a} arm {arm_a} corner {corner_a}");
    }

    #[test]
    fn golden_32px_idle_bitmap() {
        // Structural golden: four separate blobs of plausible size, mirror-symmetric in both axes.
        let size = 32usize;
        let img = render_rgba(&levels(Pattern::Idle), 32, Ink::White);
        let art: Vec<Vec<char>> = (0..size)
            .map(|y| {
                (0..size)
                    .map(|x| match img[(y * size + x) * 4 + 3] {
                        a if a < 30 => ' ',
                        a if a < 128 => '.',
                        _ => '#',
                    })
                    .collect()
            })
            .collect();
        let rows: String = art.iter().map(|r| r.iter().collect::<String>() + "\n").collect();

        // 4-neighbourhood flood fill over '#'.
        let mut seen = vec![vec![false; size]; size];
        let mut blobs = Vec::new();
        for y in 0..size {
            for x in 0..size {
                if art[y][x] != '#' || seen[y][x] {
                    continue;
                }
                seen[y][x] = true;
                let mut stack = vec![(x as isize, y as isize)];
                let mut n = 0usize;
                while let Some((cx, cy)) = stack.pop() {
                    n += 1;
                    for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                        let (nx, ny) = (cx + dx, cy + dy);
                        if nx < 0 || ny < 0 || nx >= size as isize || ny >= size as isize {
                            continue;
                        }
                        let (ux, uy) = (nx as usize, ny as usize);
                        if art[uy][ux] == '#' && !seen[uy][ux] {
                            seen[uy][ux] = true;
                            stack.push((nx, ny));
                        }
                    }
                }
                blobs.push(n);
            }
        }
        assert_eq!(blobs.len(), 4, "blob sizes {blobs:?}\n{rows}");
        for n in &blobs {
            assert!((20..=90).contains(n), "blob size {n}\n{rows}");
        }
        for y in 0..size {
            for x in 0..size {
                assert_eq!(art[y][x], art[y][size - 1 - x], "h-flip at ({x},{y})\n{rows}");
                assert_eq!(art[y][x], art[size - 1 - y][x], "v-flip at ({x},{y})\n{rows}");
            }
        }
    }

    #[test]
    fn badge_has_disc_and_digit() {
        let three = badge_rgba(3, 16);
        assert_eq!(pixel(&three, 16, 8, 8)[3], 255, "disc centre");
        let dark = three.chunks_exact(4).any(|px| px[3] == 255 && px[..3].iter().all(|&v| v < 60));
        assert!(dark, "no dark digit pixel");
        assert_eq!(pixel(&three, 16, 0, 0)[3], 0, "corner must be transparent");
        assert!(badge_rgba(0, 16).chunks_exact(4).all(|px| px[3] == 0));
        assert_ne!(badge_rgba(12, 16), three);
    }

    #[test]
    fn write_evidence_pngs() {
        let Ok(dir) = std::env::var("AMBIENT_EVIDENCE_DIR") else {
            return;
        };
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (dark, light) = ([28u8, 28, 30], [235u8, 235, 238]);
        let write = |name: &str, png: Vec<u8>| std::fs::write(dir.join(name), png).unwrap();

        let mut states: Vec<(String, Pattern)> = vec![
            ("idle".into(), Pattern::Idle),
            ("needs_you".into(), Pattern::NeedsYou),
            ("error".into(), Pattern::Error),
        ];
        states.extend((0..8u8).map(|f| (format!("running_{f}"), Pattern::Running(f))));
        states.push(("running_static".into(), Pattern::RunningStatic));

        for (name, p) in &states {
            let lv = levels(*p);
            let w32 = render_rgba(&lv, 32, Ink::White);
            write(&format!("{name}_32.png"), png_bytes(&composite(&w32, dark), 32, 32));
            let b32 = render_rgba(&lv, 32, Ink::Black);
            write(&format!("{name}_32_light.png"), png_bytes(&composite(&b32, light), 32, 32));
            let w16 = render_rgba(&lv, 16, Ink::White);
            write(&format!("{name}_16.png"), png_bytes(&composite(&w16, dark), 16, 16));
        }
        for n in [1u32, 3, 12] {
            write(&format!("badge_{n}.png"), png_bytes(&composite(&badge_rgba(n, 16), dark), 16, 16));
        }

        // Contact sheet: one 64px cell per state in a row, on the dark background.
        let cell = 64usize;
        let (sw, sh) = (cell * states.len(), cell);
        let mut sheet = vec![0u8; sw * sh * 4];
        for (i, (_, p)) in states.iter().enumerate() {
            let comp = composite(&render_rgba(&levels(*p), 64, Ink::White), dark);
            for y in 0..cell {
                let dst = (y * sw + i * cell) * 4;
                sheet[dst..dst + cell * 4].copy_from_slice(&comp[y * cell * 4..(y + 1) * cell * 4]);
            }
        }
        write("sheet.png", png_bytes(&sheet, sw as u32, sh as u32));
    }
}
