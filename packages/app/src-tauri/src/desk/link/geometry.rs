// Behaviour from desktoplab (github.com/v7ren/desktoplab), commit 6e5360e, MIT.
// (hop-core `edge.rs`, reduced to two computers side by side; the full layout editor is DL-8.)
//! Where the pointer leaves this computer and where it appears on the other one. Pure: no host, no link.
//!
//! The layout is one fact per paired computer: on which side of this computer it sits. A computer is the union of
//! its screens. The pointer crosses when it rests on the outer edge of the union that faces the other computer, and
//! appears on the facing edge of the other one, at the same relative height (or width, for above/below).

use desk_core::Side;

use super::super::host::DisplayInfo;

/// How far inside the edge the pointer appears, so a small jiggle does not send it straight back.
pub const INSET: i32 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bounds {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Bounds {
    pub fn width(&self) -> i32 {
        self.right - self.left
    }
    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }
}

fn rect(d: &DisplayInfo) -> Bounds {
    Bounds { left: d.x, top: d.y, right: d.x + d.width as i32, bottom: d.y + d.height as i32 }
}

pub fn union(displays: &[DisplayInfo]) -> Option<Bounds> {
    let mut it = displays.iter().map(rect);
    let first = it.next()?;
    Some(it.fold(first, |a, b| Bounds {
        left: a.left.min(b.left),
        top: a.top.min(b.top),
        right: a.right.max(b.right),
        bottom: a.bottom.max(b.bottom),
    }))
}

/// The wire name of a side (the receiver's entry edge travels as `Enter.monitor_id`).
pub fn side_name(side: Side) -> &'static str {
    match side {
        Side::Left => "left",
        Side::Right => "right",
        Side::Top => "top",
        Side::Bottom => "bottom",
    }
}

pub fn parse_side(name: &str) -> Option<Side> {
    match name {
        "left" => Some(Side::Left),
        "right" => Some(Side::Right),
        "top" => Some(Side::Top),
        "bottom" => Some(Side::Bottom),
        _ => None,
    }
}

/// The edge of the union that faces `side`, where `pos` rests: how far along it (0.0..=1.0), if the pointer is
/// there. "There" means on the last pixel row or column of a screen that touches the outer edge.
pub fn edge_hit(displays: &[DisplayInfo], pos: (i32, i32), side: Side) -> Option<f64> {
    let u = union(displays)?;
    let (x, y) = pos;
    let on_screen = displays.iter().map(rect).find(|r| x >= r.left && x < r.right && y >= r.top && y < r.bottom)?;
    match side {
        Side::Right if x == on_screen.right - 1 && on_screen.right == u.right => Some(along(y, u.top, u.height())),
        Side::Left if x == on_screen.left && on_screen.left == u.left => Some(along(y, u.top, u.height())),
        Side::Bottom if y == on_screen.bottom - 1 && on_screen.bottom == u.bottom => Some(along(x, u.left, u.width())),
        Side::Top if y == on_screen.top && on_screen.top == u.top => Some(along(x, u.left, u.width())),
        _ => None,
    }
}

fn along(v: i32, start: i32, len: i32) -> f64 {
    if len <= 1 {
        return 0.5;
    }
    (f64::from(v - start) / f64::from(len - 1)).clamp(0.0, 1.0)
}

/// Where the pointer appears when it enters through `entry` (an edge of THIS computer) at `t` along it.
/// `INSET` pixels inside, on a real screen.
pub fn entry_point(displays: &[DisplayInfo], entry: Side, t: f64) -> Option<(i32, i32)> {
    let u = union(displays)?;
    let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.5 };
    let (x, y) = match entry {
        Side::Left => (u.left + INSET, u.top + (t * f64::from(u.height() - 1)).round() as i32),
        Side::Right => (u.right - 1 - INSET, u.top + (t * f64::from(u.height() - 1)).round() as i32),
        Side::Top => (u.left + (t * f64::from(u.width() - 1)).round() as i32, u.top + INSET),
        Side::Bottom => (u.left + (t * f64::from(u.width() - 1)).round() as i32, u.bottom - 1 - INSET),
    };
    Some(snap_to_screen(displays, (x, y), entry))
}

/// The point, moved onto the nearest screen when the union has a hole there (screens of different sizes).
fn snap_to_screen(displays: &[DisplayInfo], p: (i32, i32), entry: Side) -> (i32, i32) {
    if displays.iter().map(rect).any(|r| p.0 >= r.left && p.0 < r.right && p.1 >= r.top && p.1 < r.bottom) {
        return p;
    }
    let mut best: Option<((i32, i32), i64)> = None;
    for r in displays.iter().map(rect) {
        let cx = p.0.clamp(r.left, r.right - 1);
        let cy = p.1.clamp(r.top, r.bottom - 1);
        // Prefer a screen on the entry edge; otherwise the closest point.
        let d = i64::from(cx - p.0).pow(2) + i64::from(cy - p.1).pow(2);
        let on_edge = match entry {
            Side::Left => r.left == union(displays).map_or(r.left, |u| u.left),
            Side::Right => r.right == union(displays).map_or(r.right, |u| u.right),
            Side::Top => r.top == union(displays).map_or(r.top, |u| u.top),
            Side::Bottom => r.bottom == union(displays).map_or(r.bottom, |u| u.bottom),
        };
        let score = if on_edge { d } else { d + 1_000_000_000 };
        if best.is_none_or(|(_, s)| score < s) {
            best = Some(((cx, cy), score));
        }
    }
    best.map_or(p, |(q, _)| q)
}

/// The controlled computer: the pointer entered through `entry`; after moving by `(dx, dy)` it rests on that edge and
/// the motion points back out. Returns how far along the edge it was.
pub fn pushing_back(displays: &[DisplayInfo], entry: Side, pos: (i32, i32), delta: (i32, i32)) -> Option<f64> {
    let outward = match entry {
        Side::Left => delta.0 < 0,
        Side::Right => delta.0 > 0,
        Side::Top => delta.1 < 0,
        Side::Bottom => delta.1 > 0,
    };
    if !outward {
        return None;
    }
    edge_hit(displays, pos, entry)
}

/// `pos`, moved `INSET` pixels away from the edge `exit` (the edge the pointer left through).
pub fn inward(pos: (i32, i32), exit: Side) -> (i32, i32) {
    match exit {
        Side::Left => (pos.0 + INSET, pos.1),
        Side::Right => (pos.0 - INSET, pos.1),
        Side::Top => (pos.0, pos.1 + INSET),
        Side::Bottom => (pos.0, pos.1 - INSET),
    }
}

/// 0..1000 on the wire.
pub fn milli(t: f64) -> u16 {
    (t.clamp(0.0, 1.0) * 1000.0).round() as u16
}

pub fn from_milli(m: u16) -> f64 {
    f64::from(m.min(1000)) / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(id: &str, x: i32, y: i32, w: u32, h: u32) -> DisplayInfo {
        DisplayInfo { id: id.into(), name: id.into(), x, y, width: w, height: h, scale: 1.0, primary: x == 0 && y == 0 }
    }

    #[test]
    fn the_union_covers_every_screen() {
        let ds = [d("a", 0, 0, 1920, 1080), d("b", 1920, -200, 1280, 1024)];
        let u = union(&ds).unwrap();
        assert_eq!((u.left, u.top, u.right, u.bottom), (0, -200, 3200, 1080));
        assert!(union(&[]).is_none());
    }

    #[test]
    fn the_pointer_crosses_only_on_the_outer_edge_facing_the_other_computer() {
        let ds = [d("a", 0, 0, 1920, 1080), d("b", 1920, 0, 1920, 1080)];
        // Between the two screens of this computer is not an edge.
        assert_eq!(edge_hit(&ds, (1919, 500), Side::Right), None);
        // The far right edge is.
        let t = edge_hit(&ds, (3839, 540), Side::Right).unwrap();
        assert!((t - 540.0 / 1079.0).abs() < 1e-9);
        // Not on the edge, or on the wrong side.
        assert_eq!(edge_hit(&ds, (3838, 540), Side::Right), None);
        assert_eq!(edge_hit(&ds, (3839, 540), Side::Left), None);
        assert!(edge_hit(&ds, (0, 0), Side::Left).is_some());
        assert!(edge_hit(&ds, (0, 0), Side::Top).is_some());
        assert!(edge_hit(&ds, (100, 1079), Side::Bottom).is_some());
    }

    #[test]
    fn the_pointer_appears_at_the_same_relative_height_inside_the_facing_edge() {
        let theirs = [d("x", 0, 0, 1280, 800)];
        let (x, y) = entry_point(&theirs, Side::Left, 0.5).unwrap();
        assert_eq!(x, INSET);
        assert!((y - 400).abs() <= 1);
        let (x, _) = entry_point(&theirs, Side::Right, 0.0).unwrap();
        assert_eq!(x, 1280 - 1 - INSET);
        let (_, y) = entry_point(&theirs, Side::Top, 1.0).unwrap();
        assert_eq!(y, INSET);
    }

    #[test]
    fn a_hole_in_the_union_is_snapped_onto_a_screen() {
        // A small screen beside a tall one: entering at the bottom of the left edge lands where there is no screen.
        let theirs = [d("tall", 0, 0, 1000, 2000), d("small", 1000, 0, 1000, 500)];
        let (x, y) = entry_point(&theirs, Side::Right, 1.0).unwrap();
        assert!(theirs.iter().any(|s| x >= s.x && x < s.x + s.width as i32 && y >= s.y && y < s.y + s.height as i32), "{x},{y}");
    }

    #[test]
    fn a_jiggle_right_after_entering_does_not_send_the_pointer_back() {
        let theirs = [d("x", 0, 0, 1280, 800)];
        let at = entry_point(&theirs, Side::Left, 0.5).unwrap();
        // Entered INSET pixels inside: a small move outward does not rest on the edge.
        assert_eq!(pushing_back(&theirs, Side::Left, (at.0 - 2, at.1), (-2, 0)), None);
        // Pushing on until the edge does.
        assert!(pushing_back(&theirs, Side::Left, (0, at.1), (-5, 0)).is_some());
        // Moving inward on the edge is not a push back.
        assert_eq!(pushing_back(&theirs, Side::Left, (0, at.1), (5, 0)), None);
    }

    #[test]
    fn wire_numbers_stay_in_range() {
        assert_eq!(milli(-1.0), 0);
        assert_eq!(milli(2.0), 1000);
        assert_eq!(milli(0.5), 500);
        assert_eq!(from_milli(60_000), 1.0);
        assert!(entry_point(&[d("x", 0, 0, 100, 100)], Side::Left, f64::NAN).is_some());
    }

    #[test]
    fn side_names_round_trip() {
        for s in [Side::Left, Side::Right, Side::Top, Side::Bottom] {
            assert_eq!(parse_side(side_name(s)), Some(s));
        }
        assert_eq!(parse_side("diagonal"), None);
    }
}
